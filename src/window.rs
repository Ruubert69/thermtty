//! Borderless, draggable widget window with a close button and a right-click menu.

use std::mem::{transmute, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, MONITOR_DEFAULTTONULL, MonitorFromRect, RDW_FRAME, RDW_INVALIDATE,
    RedrawWindow, ScreenToClient,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{Config, OPACITIES, SIZES};
use crate::grid::{self, Grid, Painter};
use crate::monitor::{COLS, Monitor};
use crate::wide;

const TIMER_ID: usize = 1;
const ID_TOPMOST: usize = 1;
const ID_EXIT: usize = 3;
const ID_OPACITY: usize = 100;
const ID_SIZE: usize = 200;
/// From `Win32_UI_Controls`, which is too large a feature to enable for one constant.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Gap from the work-area corner for the default position.
const MARGIN: i32 = 16;

struct App {
    monitor: Monitor,
    grid: Grid,
    painter: Painter,
    cfg: Config,
    /// Cursor is over the close button.
    close_hot: bool,
}

/// Signed x/y packed in a mouse message's `lParam`.
fn point(lparam: LPARAM) -> POINT {
    POINT { x: (lparam & 0xFFFF) as i16 as i32, y: ((lparam >> 16) & 0xFFFF) as i16 as i32 }
}

unsafe fn in_close(hwnd: HWND, app: *const App, pt: POINT) -> bool {
    let mut rc: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };
    let r = unsafe { (*app).painter.close_rect(rc.right) };
    pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
}

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        dark_menus();
        let instance = GetModuleHandleW(null());
        let class = wide("thermtty");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        let cfg = Config::load();
        let (x, y) = cfg.pos.unwrap_or((0, 0));
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            x,
            y,
            1,
            1,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&raw const corner).cast(), 4);
        let border = grid::BORDER;
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&raw const border).cast(), 4);

        let mut monitor = Monitor::new();
        monitor.tick();
        let mut grid = Grid::new(COLS, monitor.rows());
        monitor.draw(&mut grid);
        let app = Box::into_raw(Box::new(App { monitor, grid, painter: Painter::new(), cfg, close_hot: false }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        {
            let app = &mut *app;
            app.apply_topmost(hwnd);
            app.apply_opacity(hwnd);
            app.layout(hwnd, GetDpiForWindow(hwnd), None);
            app.ensure_visible(hwnd);
        }
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_ID, 1000, None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

impl App {
    unsafe fn layout(&mut self, hwnd: HWND, dpi: u32, pos: Option<(i32, i32)>) {
        self.painter.set_font(self.cfg.size * dpi as i32 / 96, COLS);
        let (w, h) = self.painter.window_size(&self.grid);
        let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
        let (x, y) = pos.unwrap_or_else(|| {
            flags |= SWP_NOMOVE;
            (0, 0)
        });
        unsafe { SetWindowPos(hwnd, null_mut(), x, y, w, h, flags) };
    }

    /// Falls back to the top-right of the primary work area when there is no saved
    /// position or it is no longer on any monitor.
    unsafe fn ensure_visible(&mut self, hwnd: HWND) {
        unsafe {
            let mut r: RECT = zeroed();
            GetWindowRect(hwnd, &mut r);
            if self.cfg.pos.is_some() && !MonitorFromRect(&r, MONITOR_DEFAULTTONULL).is_null() {
                return;
            }
            let mut work: RECT = zeroed();
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut work).cast(), 0);
            let x = work.right - (r.right - r.left) - MARGIN;
            SetWindowPos(hwnd, null_mut(), x, work.top + MARGIN, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    unsafe fn apply_topmost(&self, hwnd: HWND) {
        let after = if self.cfg.topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
        unsafe { SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    unsafe fn apply_opacity(&self, hwnd: HWND) {
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if self.cfg.opacity < 100 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
                SetLayeredWindowAttributes(hwnd, 0, (u32::from(self.cfg.opacity) * 255 / 100) as u8, LWA_ALPHA);
            } else if ex & WS_EX_LAYERED as isize != 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
                RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_FRAME);
            }
        }
    }

    unsafe fn save_pos(&mut self, hwnd: HWND) {
        let mut r: RECT = unsafe { zeroed() };
        unsafe { GetWindowRect(hwnd, &mut r) };
        self.cfg.pos = Some((r.left, r.top));
        self.cfg.save();
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if app.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        match msg {
            WM_TIMER => {
                let app = &mut *app;
                app.monitor.tick();
                app.monitor.draw(&mut app.grid);
                InvalidateRect(hwnd, null(), 0);
                0
            }
            WM_PAINT => {
                let mut ps = zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc: RECT = zeroed();
                GetClientRect(hwnd, &mut rc);
                (*app).painter.paint(hdc, &(*app).grid, rc.right, rc.bottom, (*app).close_hot);
                EndPaint(hwnd, &ps);
                0
            }
            WM_ERASEBKGND => 1,
            // The close button is client area; everything else drags the window.
            WM_NCHITTEST => {
                let mut pt = point(lparam);
                ScreenToClient(hwnd, &mut pt);
                (if in_close(hwnd, app, pt) { HTCLIENT } else { HTCAPTION }) as LRESULT
            }
            // Client mouse messages only arrive over the close button.
            WM_MOUSEMOVE => {
                if !(*app).close_hot {
                    (*app).close_hot = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_MOUSELEAVE => {
                (*app).close_hot = false;
                InvalidateRect(hwnd, null(), 0);
                0
            }
            WM_LBUTTONUP => {
                if in_close(hwnd, app, point(lparam)) {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_NCLBUTTONDBLCLK => 0,
            WM_EXITSIZEMOVE => {
                (*app).save_pos(hwnd);
                0
            }
            WM_CONTEXTMENU => {
                show_menu(hwnd, app);
                0
            }
            WM_DPICHANGED => {
                let r = &*(lparam as *const RECT);
                (*app).layout(hwnd, (wparam & 0xFFFF) as u32, Some((r.left, r.top)));
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// `TrackPopupMenu` pumps messages (timer ticks re-enter `wndproc`), so no `&mut App`
/// is held across it.
unsafe fn show_menu(hwnd: HWND, app: *mut App) {
    unsafe {
        let (topmost, opacity, size) = ((*app).cfg.topmost, (*app).cfg.opacity, (*app).cfg.size);
        let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING | check(topmost), ID_TOPMOST, wide("Always on top").as_ptr());
        let opacity_menu = CreatePopupMenu();
        for (i, o) in OPACITIES.iter().enumerate() {
            AppendMenuW(opacity_menu, MF_STRING | check(*o == opacity), ID_OPACITY + i, wide(&format!("{o}%")).as_ptr());
        }
        AppendMenuW(menu, MF_POPUP, opacity_menu as usize, wide("Opacity").as_ptr());
        let size_menu = CreatePopupMenu();
        for (i, (px, label)) in SIZES.iter().enumerate() {
            AppendMenuW(size_menu, MF_STRING | check(*px == size), ID_SIZE + i, wide(label).as_ptr());
        }
        AppendMenuW(menu, MF_POPUP, size_menu as usize, wide("Text size").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, ID_EXIT, wide("Exit").as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, null()) as usize;
        DestroyMenu(menu);

        let app = &mut *app;
        match cmd {
            ID_TOPMOST => {
                app.cfg.topmost = !app.cfg.topmost;
                app.apply_topmost(hwnd);
            }
            ID_EXIT => {
                DestroyWindow(hwnd);
                return;
            }
            c if (ID_OPACITY..ID_OPACITY + OPACITIES.len()).contains(&c) => {
                app.cfg.opacity = OPACITIES[c - ID_OPACITY];
                app.apply_opacity(hwnd);
            }
            c if (ID_SIZE..ID_SIZE + SIZES.len()).contains(&c) => {
                app.cfg.size = SIZES[c - ID_SIZE].0;
                app.layout(hwnd, GetDpiForWindow(hwnd), None);
                InvalidateRect(hwnd, null(), 0);
            }
            _ => return,
        }
        app.save_pos(hwnd);
    }
}

/// Dark popup menus via uxtheme's SetPreferredAppMode (#135) and FlushMenuThemes (#136):
/// undocumented, but stable since Windows 10 1903. Missing exports leave light menus.
unsafe fn dark_menus() {
    unsafe {
        let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
        if lib.is_null() {
            return;
        }
        if let (Some(set), Some(flush)) = (GetProcAddress(lib, 135usize as *const u8), GetProcAddress(lib, 136usize as *const u8)) {
            let set: unsafe extern "system" fn(i32) -> i32 = transmute(set);
            let flush: unsafe extern "system" fn() = transmute(flush);
            set(2); // ForceDark
            flush();
        }
    }
}
