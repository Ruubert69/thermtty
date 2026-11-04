//! Borderless widget window plus the settings panel opened by right-clicking it.

use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
    MONITORINFO, MonitorFromRect, MonitorFromWindow, RDW_FRAME, RDW_INVALIDATE, RedrawWindow, ScreenToClient,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{Config, Layer, OPACITIES, REFRESH, SIZES};
use crate::grid::{self, Grid, Painter};
use crate::monitor::{COLS, Monitor};
use crate::settings::{self, Action, Hit, Panel, cycle};
use crate::wide;

const TIMER_ID: usize = 1;
/// From `Win32_UI_Controls`, which is too large a feature to enable for one constant.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Gap from the work-area corner for the default position.
const MARGIN: i32 = 16;
/// Gap between the widget and the settings panel.
const PANEL_GAP: i32 = 8;
/// A right-click this soon after the panel closed itself (by losing focus to that very
/// click) is the user toggling it off, not asking to reopen it.
const REOPEN_GUARD_MS: u32 = 400;
/// Posted by a second launch to the running widget: open the settings panel. Keeps the
/// settings reachable when the widget itself is hidden behind windows or by Win+D.
const WM_APP_SETTINGS: u32 = WM_APP + 1;

struct App {
    hwnd: HWND,
    monitor: Monitor,
    grid: Grid,
    painter: Painter,
    cfg: Config,
    /// Cursor is over the close button.
    close_hot: bool,
    panel: Option<Panel>,
    panel_closed_at: u32,
}

/// Signed x/y packed in a mouse message's `lParam`.
fn point(lparam: LPARAM) -> POINT {
    POINT { x: (lparam & 0xFFFF) as i16 as i32, y: ((lparam >> 16) & 0xFFFF) as i16 as i32 }
}

fn client_width(hwnd: HWND) -> i32 {
    let mut rc: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };
    rc.right
}

fn track_leave(hwnd: HWND) {
    let mut tme =
        TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
    unsafe { TrackMouseEvent(&mut tme) };
}

/// Rounded corners and the thin border shared by both windows.
fn style_frame(hwnd: HWND) {
    let corner = DWMWCP_ROUND;
    let border = grid::BORDER;
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&raw const corner).cast(), 4);
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&raw const border).cast(), 4);
    }
}

fn register_class(name: &[u16], proc: WNDPROC) {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: GetModuleHandleW(null()),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: name.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);
    }
}

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = wide("thermtty");
        let running = FindWindowW(class.as_ptr(), null());
        if !running.is_null() {
            // We were launched by the user, so we may hand foreground rights to the widget.
            AllowSetForegroundWindow(ASFW_ANY);
            PostMessageW(running, WM_APP_SETTINGS, 0, 0);
            return;
        }
        register_class(&class, Some(wndproc));
        register_class(&wide("thermtty-settings"), Some(panel_proc));
        let cfg = Config::load();
        let (x, y) = cfg.pos.unwrap_or((0, 0));
        // SetWindowPos(HWND_TOPMOST) is ignored while the process doesn't have focus (seen on
        // Windows build 26300), and at logon it never does, so topmost is set at creation.
        let topmost = if cfg.layer == Layer::Top { WS_EX_TOPMOST } else { 0 };
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | topmost,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            x,
            y,
            1,
            1,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        style_frame(hwnd);

        let mut monitor = Monitor::new();
        monitor.rows = cfg.rows;
        monitor.fahrenheit = cfg.fahrenheit;
        if monitor.rows() == 0 {
            monitor.rows[0] = true; // e.g. only "gpu" saved, on a PC without one
        }
        monitor.tick();
        let grid = Grid::new(COLS, monitor.rows());
        let app = Box::into_raw(Box::new(App {
            hwnd,
            monitor,
            grid,
            painter: Painter::new(),
            cfg,
            close_hot: false,
            panel: None,
            panel_closed_at: 0,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        {
            let app = &mut *app;
            app.monitor.draw(&mut app.grid);
            app.apply_layer();
            app.apply_opacity();
            app.layout(GetDpiForWindow(hwnd), None);
            app.ensure_visible();
            app.start_timer();
        }
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

impl App {
    /// Rebuilds the grid for the current rows and resizes the window to fit.
    unsafe fn layout(&mut self, dpi: u32, pos: Option<(i32, i32)>) {
        if self.grid.rows != self.monitor.rows() {
            self.grid = Grid::new(COLS, self.monitor.rows());
            self.monitor.draw(&mut self.grid);
        }
        self.painter.close_gutter = self.cfg.show_close;
        self.painter.set_font(self.cfg.size * dpi as i32 / 96, COLS);
        let (w, h) = self.painter.window_size(&self.grid);
        let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
        let (x, y) = pos.unwrap_or_else(|| {
            flags |= SWP_NOMOVE;
            (0, 0)
        });
        unsafe {
            SetWindowPos(self.hwnd, null_mut(), x, y, w, h, flags);
            InvalidateRect(self.hwnd, null(), 0);
        }
    }

    /// Falls back to the top-right of the primary work area when there is no saved
    /// position or it is no longer on any monitor.
    unsafe fn ensure_visible(&mut self) {
        unsafe {
            let mut r: RECT = zeroed();
            GetWindowRect(self.hwnd, &mut r);
            if self.cfg.pos.is_some() && !MonitorFromRect(&r, MONITOR_DEFAULTTONULL).is_null() {
                return;
            }
            let mut work: RECT = zeroed();
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut work).cast(), 0);
            let x = work.right - (r.right - r.left) - MARGIN;
            SetWindowPos(self.hwnd, null_mut(), x, work.top + MARGIN, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    unsafe fn apply_layer(&self) {
        // HWND_BOTTOM also drops topmost status; WM_WINDOWPOSCHANGING keeps it at the bottom.
        let after = match self.cfg.layer {
            Layer::Top => HWND_TOPMOST,
            Layer::Normal => HWND_NOTOPMOST,
            Layer::Desktop => HWND_BOTTOM,
        };
        unsafe { SetWindowPos(self.hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    unsafe fn apply_opacity(&self) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            if self.cfg.opacity < 100 {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
                SetLayeredWindowAttributes(self.hwnd, 0, (u32::from(self.cfg.opacity) * 255 / 100) as u8, LWA_ALPHA);
            } else if ex & WS_EX_LAYERED as isize != 0 {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
                RedrawWindow(self.hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_FRAME);
            }
        }
    }

    unsafe fn start_timer(&self) {
        unsafe { SetTimer(self.hwnd, TIMER_ID, self.cfg.refresh * 1000, None) };
    }

    unsafe fn save_pos(&mut self) {
        let mut r: RECT = unsafe { zeroed() };
        unsafe { GetWindowRect(self.hwnd, &mut r) };
        self.cfg.pos = Some((r.left, r.top));
        self.cfg.save();
    }

    fn in_close(&self, pt: POINT) -> bool {
        let r = self.painter.close_rect(client_width(self.hwnd));
        self.cfg.show_close && pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
    }

    unsafe fn apply(&mut self, action: Action) {
        let cfg = &mut self.cfg;
        unsafe {
            match action {
                Action::Layer(layer) => {
                    cfg.layer = layer;
                    self.apply_layer();
                }
                Action::CloseButton => {
                    cfg.show_close = !cfg.show_close;
                    self.close_hot = false;
                    self.layout(GetDpiForWindow(self.hwnd), None);
                }
                Action::Lock => cfg.locked = !cfg.locked,
                Action::Fahrenheit => {
                    cfg.fahrenheit = !cfg.fahrenheit;
                    self.monitor.fahrenheit = cfg.fahrenheit;
                    self.monitor.draw(&mut self.grid);
                    InvalidateRect(self.hwnd, null(), 0);
                }
                Action::Row(i) => {
                    cfg.rows[i] = !cfg.rows[i];
                    self.monitor.rows = cfg.rows;
                    self.layout(GetDpiForWindow(self.hwnd), None);
                }
                Action::Opacity(step) => {
                    cfg.opacity = cycle(&OPACITIES, cfg.opacity, step);
                    self.apply_opacity();
                }
                Action::Size(step) => {
                    let sizes = SIZES.map(|(px, _)| px);
                    cfg.size = cycle(&sizes, cfg.size, step);
                    self.layout(GetDpiForWindow(self.hwnd), None);
                }
                Action::Refresh(step) => {
                    cfg.refresh = cycle(&REFRESH, cfg.refresh, step);
                    self.start_timer();
                }
                Action::Quit => {
                    PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
                    return;
                }
            }
        }
        self.cfg.save();
        self.redraw_panel();
    }

    fn redraw_panel(&mut self) {
        let has_gpu = self.monitor.has_gpu();
        if let Some(panel) = &mut self.panel {
            panel.draw(&self.cfg, has_gpu);
            unsafe { InvalidateRect(panel.hwnd, null(), 0) };
        }
    }

    /// Opens the settings panel beside the widget, on whichever side has room.
    unsafe fn open_panel(&mut self) {
        unsafe {
            let class = wide("thermtty-settings");
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                class.as_ptr(),
                wide("thermtty settings").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            if hwnd.is_null() {
                return;
            }
            style_frame(hwnd);
            let mut panel = Panel::new(hwnd, GetDpiForWindow(self.hwnd));
            panel.draw(&self.cfg, self.monitor.has_gpu());
            let (w, h) = panel.size();
            self.panel = Some(panel);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut App as isize);

            let mut r: RECT = zeroed();
            GetWindowRect(self.hwnd, &mut r);
            let mut mi: MONITORINFO = zeroed();
            mi.cbSize = size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
            let work = mi.rcWork;
            let x = if r.left - PANEL_GAP - w >= work.left { r.left - PANEL_GAP - w } else { r.right + PANEL_GAP };
            let x = x.clamp(work.left, (work.right - w).max(work.left));
            let y = r.top.clamp(work.top, (work.bottom - h).max(work.top));
            SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, 0);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }

    unsafe fn close_panel(&mut self) {
        if let Some(panel) = self.panel.take() {
            self.panel_closed_at = unsafe { GetTickCount() };
            unsafe { DestroyWindow(panel.hwnd) };
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if app.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let app = &mut *app;
        match msg {
            WM_TIMER => {
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
                let close = app.cfg.show_close.then_some(app.close_hot);
                app.painter.paint(hdc, &app.grid, rc.right, rc.bottom, close);
                EndPaint(hwnd, &ps);
                0
            }
            WM_ERASEBKGND => 1,
            // HTCAPTION makes the surface drag the window; the close button and a locked
            // widget are client area instead.
            WM_NCHITTEST => {
                let mut pt = point(lparam);
                ScreenToClient(hwnd, &mut pt);
                (if app.cfg.locked || app.in_close(pt) { HTCLIENT } else { HTCAPTION }) as LRESULT
            }
            WM_MOUSEMOVE => {
                let hot = app.in_close(point(lparam));
                if hot != app.close_hot {
                    app.close_hot = hot;
                    InvalidateRect(hwnd, null(), 0);
                }
                track_leave(hwnd);
                0
            }
            WM_MOUSELEAVE => {
                if app.close_hot {
                    app.close_hot = false;
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_LBUTTONUP => {
                if app.in_close(point(lparam)) {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_APP_SETTINGS => {
                if app.panel.is_none() {
                    app.open_panel();
                }
                0
            }
            // Most of the widget is caption (HTCAPTION, for dragging), where DefWindowProc
            // treats right-clicks as system-menu clicks; handle both kinds ourselves.
            WM_NCRBUTTONDOWN | WM_RBUTTONDOWN => 0,
            WM_NCRBUTTONUP | WM_RBUTTONUP => {
                if app.panel.is_some() {
                    app.close_panel();
                } else if GetTickCount().wrapping_sub(app.panel_closed_at) > REOPEN_GUARD_MS {
                    app.open_panel();
                }
                0
            }
            WM_CONTEXTMENU => 0,
            WM_WINDOWPOSCHANGING if app.cfg.layer == Layer::Desktop => {
                // Clicking or activating would raise the widget; pin it to the bottom.
                let pos = &mut *(lparam as *mut WINDOWPOS);
                if pos.flags & SWP_NOZORDER == 0 {
                    pos.hwndInsertAfter = HWND_BOTTOM;
                }
                0
            }
            WM_NCLBUTTONDBLCLK => 0,
            WM_EXITSIZEMOVE => {
                app.save_pos();
                0
            }
            WM_DPICHANGED => {
                let r = &*(lparam as *const RECT);
                app.layout((wparam & 0xFFFF) as u32, Some((r.left, r.top)));
                0
            }
            WM_DESTROY => {
                app.close_panel();
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        // After close_panel the window is still being torn down but no longer has state.
        let Some(app) = app.as_mut().filter(|a| a.panel.as_ref().is_some_and(|p| p.hwnd == hwnd)) else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        // Checked just above; re-borrowed per message so no `&mut Panel` outlives a call on `app`.
        macro_rules! panel {
            () => {
                app.panel.as_mut().unwrap()
            };
        }
        match msg {
            WM_PAINT => {
                let mut ps = zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc: RECT = zeroed();
                GetClientRect(hwnd, &mut rc);
                panel!().paint(hdc, rc.right, rc.bottom);
                EndPaint(hwnd, &ps);
                0
            }
            WM_ERASEBKGND => 1,
            WM_MOUSEMOVE => {
                if panel!().hover(Some(point(lparam)), client_width(hwnd)) {
                    app.redraw_panel();
                }
                track_leave(hwnd);
                0
            }
            WM_MOUSELEAVE => {
                if panel!().hover(None, client_width(hwnd)) {
                    app.redraw_panel();
                }
                0
            }
            WM_LBUTTONUP => {
                match panel!().hit(point(lparam), client_width(hwnd)) {
                    Some(Hit::Close) => app.close_panel(),
                    Some(Hit::Action(action)) => app.apply(action),
                    None => {}
                }
                0
            }
            WM_KEYDOWN if wparam == usize::from(VK_ESCAPE) => {
                app.close_panel();
                0
            }
            // Closes like a flyout when focus moves elsewhere.
            WM_ACTIVATE if wparam & 0xFFFF == WA_INACTIVE as usize => {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                0
            }
            WM_CLOSE => {
                app.close_panel();
                0
            }
            WM_DPICHANGED => {
                let r = &*(lparam as *const RECT);
                let panel = panel!();
                panel.painter.set_font(settings::FONT_PX * (wparam & 0xFFFF) as i32 / 96, settings::COLS);
                let (w, h) = panel.size();
                SetWindowPos(hwnd, null_mut(), r.left, r.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
