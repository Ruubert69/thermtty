//! Fixed character grid (the "terminal") and its GDI painter.

use std::fmt::{self, Write as _};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;

use crate::wide;

pub type Color = u8;
pub const FG: Color = 0;
pub const MUTED: Color = 1;
pub const DIM: Color = 2;
pub const ACCENT: Color = 3;
pub const WARN: Color = 4;
pub const HOT: Color = 5;

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
pub const BACKGROUND: u32 = rgb(0x0e, 0x12, 0x15);
pub const BORDER: u32 = rgb(0x2a, 0x31, 0x36);
const PALETTE: [u32; 6] = [
    rgb(0xc9, 0xd1, 0xd9), // FG
    rgb(0x68, 0x72, 0x7c), // MUTED
    rgb(0x27, 0x2e, 0x33), // DIM
    rgb(0x3f, 0xc8, 0x5c), // ACCENT
    rgb(0xe3, 0xb3, 0x41), // WARN
    rgb(0xf8, 0x51, 0x49), // HOT
];

pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    chars: Vec<u16>,
    colors: Vec<Color>,
    scratch: String,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let n = cols * rows;
        Self { cols, rows, chars: vec![b' '.into(); n], colors: vec![FG; n], scratch: String::new() }
    }

    pub fn clear(&mut self) {
        self.chars.fill(b' '.into());
        self.colors.fill(FG);
    }

    /// Writes `s` at `(row, col)`, clipped to the row; returns the column after the text.
    pub fn put(&mut self, row: usize, col: usize, s: &str, color: Color) -> usize {
        let mut c = col;
        if row >= self.rows {
            return c;
        }
        for ch in s.chars() {
            if c >= self.cols {
                break;
            }
            let i = row * self.cols + c;
            self.chars[i] = u16::try_from(u32::from(ch)).unwrap_or(b'?'.into());
            self.colors[i] = color;
            c += 1;
        }
        c
    }

    /// Writes `s` so it ends just before column `end`; returns its start column.
    pub fn put_r(&mut self, row: usize, end: usize, s: &str, color: Color) -> usize {
        let start = end.saturating_sub(s.chars().count());
        self.put(row, start, s, color);
        start
    }

    pub fn putf_r(&mut self, row: usize, end: usize, color: Color, args: fmt::Arguments) -> usize {
        let s = self.format(args);
        let start = self.put_r(row, end, &s, color);
        self.scratch = s;
        start
    }

    fn format(&mut self, args: fmt::Arguments) -> String {
        let mut s = std::mem::take(&mut self.scratch);
        s.clear();
        let _ = s.write_fmt(args);
        s
    }

    pub fn text(&self) -> String {
        self.chars.chunks(self.cols).map(|row| String::from_utf16_lossy(row).trim_end().to_owned() + "\n").collect()
    }
}

pub struct Painter {
    font: HFONT,
    cell_w: i32,
    line_h: i32,
    text_dy: i32,
    pad: i32,
    dx: Vec<i32>,
    background: HBRUSH,
    /// Reserve a two-cell gutter right of the grid for the close button.
    pub close_gutter: bool,
}

impl Painter {
    pub fn new() -> Self {
        Self {
            font: null_mut(),
            cell_w: 0,
            line_h: 0,
            text_dy: 0,
            pad: 0,
            dx: Vec::new(),
            background: unsafe { CreateSolidBrush(BACKGROUND) },
            close_gutter: false,
        }
    }

    /// (Re)creates fonts for a pixel height already scaled to the window's DPI.
    pub fn set_font(&mut self, px: i32, cols: usize) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            self.font = create_font(px);
            let dc = GetDC(null_mut());
            let old = SelectObject(dc, self.font);
            let mut tm: TEXTMETRICW = std::mem::zeroed();
            GetTextMetricsW(dc, &mut tm);
            let mut size = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, wide("M").as_ptr(), 1, &mut size);
            SelectObject(dc, old);
            ReleaseDC(null_mut(), dc);
            let gap = (px / 4).max(2);
            self.cell_w = size.cx;
            self.line_h = tm.tmHeight + gap;
            self.text_dy = gap / 2;
            self.pad = px;
        }
        self.dx = vec![self.cell_w; cols];
    }

    pub fn window_size(&self, grid: &Grid) -> (i32, i32) {
        let gutter = if self.close_gutter { 2 * self.cell_w } else { 0 };
        let height = grid.rows as i32 * self.line_h + 2 * self.pad;
        (grid.cols as i32 * self.cell_w + gutter + 2 * self.pad, height)
    }

    /// Close button hit area: the gutter beside the first text row, extended to the
    /// top-right corner so it is easy to hit.
    pub fn close_rect(&self, width: i32) -> RECT {
        RECT { left: width - self.pad - self.cell_w * 3 / 2, top: 0, right: width, bottom: self.pad + self.line_h }
    }

    /// Grid cell under a client-area point.
    pub fn cell_at(&self, pt: windows_sys::Win32::Foundation::POINT) -> Option<(usize, usize)> {
        let (x, y) = (pt.x - self.pad, pt.y - self.pad);
        (x >= 0 && y >= 0 && self.cell_w > 0).then(|| ((y / self.line_h) as usize, (x / self.cell_w) as usize))
    }

    /// `close`: `None` hides the close button, `Some(hovered)` draws it.
    pub fn paint(&self, hdc: HDC, grid: &Grid, width: i32, height: i32, close: Option<bool>) {
        unsafe {
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, width, height);
            let old_bmp = SelectObject(mem, bmp);
            FillRect(mem, &RECT { left: 0, top: 0, right: width, bottom: height }, self.background);
            SetBkMode(mem, TRANSPARENT as i32);
            let old_font = SelectObject(mem, self.font);
            let mut y = self.pad + self.text_dy;
            for row in 0..grid.rows {
                let base = row * grid.cols;
                let (chars, colors) = (&grid.chars[base..base + grid.cols], &grid.colors[base..base + grid.cols]);
                let mut start = 0;
                while start < grid.cols {
                    let color = colors[start];
                    let mut end = start + 1;
                    while end < grid.cols && colors[end] == color {
                        end += 1;
                    }
                    if chars[start..end].iter().any(|&c| c != b' '.into()) {
                        SetTextColor(mem, PALETTE[color as usize]);
                        let x = self.pad + start as i32 * self.cell_w;
                        ExtTextOutW(mem, x, y, 0, null(), chars[start..].as_ptr(), (end - start) as u32, self.dx.as_ptr());
                    }
                    start = end;
                }
                y += self.line_h;
            }
            if let Some(hot) = close {
                self.paint_close(mem, width, hot);
            }
            BitBlt(hdc, 0, 0, width, height, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_font);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp);
            DeleteDC(mem);
        }
    }

    /// A thin `×` in the top padding above the gutter, clear of the border; muted until hovered.
    unsafe fn paint_close(&self, dc: HDC, width: i32, hot: bool) {
        let (cx, cy) = (width - self.pad - self.cell_w / 2, self.pad * 3 / 5);
        let h = (self.pad / 4).max(2);
        let color = PALETTE[usize::from(if hot { HOT } else { MUTED })];
        unsafe {
            let pen = CreatePen(PS_SOLID, (self.pad / 12).max(1), color);
            let old = SelectObject(dc, pen);
            // LineTo excludes the end point, hence the +1.
            MoveToEx(dc, cx - h, cy - h, null_mut());
            LineTo(dc, cx + h + 1, cy + h + 1);
            MoveToEx(dc, cx + h, cy - h, null_mut());
            LineTo(dc, cx - h - 1, cy + h + 1);
            SelectObject(dc, old);
            DeleteObject(pen);
        }
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            DeleteObject(self.background);
        }
    }
}

fn create_font(px: i32) -> HFONT {
    unsafe {
        let make = |face: &str| {
            CreateFontW(
                -px,
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_TT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                (FIXED_PITCH | FF_MODERN) as u32,
                wide(face).as_ptr(),
            )
        };
        let font = make("Cascadia Mono");
        // GDI silently substitutes missing faces; fall back to Consolas explicitly.
        let dc = GetDC(null_mut());
        let old = SelectObject(dc, font);
        let mut face = [0u16; 32];
        let len = GetTextFaceW(dc, face.len() as i32, face.as_mut_ptr());
        SelectObject(dc, old);
        ReleaseDC(null_mut(), dc);
        if String::from_utf16_lossy(&face[..(len.max(1) - 1) as usize]) == "Cascadia Mono" {
            font
        } else {
            DeleteObject(font);
            make("Consolas")
        }
    }
}
