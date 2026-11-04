//! Terminal-style settings panel: drawn on a character grid, clicked through hit zones
//! recorded while drawing.

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::HDC;

use crate::config::{Config, Layer, ROW_NAMES, SIZES};
use crate::grid::*;

pub const COLS: usize = 26;
pub const ROWS: usize = 16;
/// Font pixel height at 96 DPI; the panel does not follow the widget's text size.
pub const FONT_PX: i32 = 14;
/// Columns of the `‹ value ›` selector.
const PREV: usize = 16;
const VALUE: usize = 18;
const NEXT: usize = COLS - 1;

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Layer(Layer),
    CloseButton,
    Lock,
    Fahrenheit,
    Row(usize),
    Opacity(isize),
    Size(isize),
    Refresh(isize),
    Quit,
}

struct Zone {
    row: usize,
    cols: std::ops::Range<usize>,
    action: Action,
}

pub enum Hit {
    Close,
    Action(Action),
}

pub struct Panel {
    pub hwnd: HWND,
    grid: Grid,
    pub painter: Painter,
    zones: Vec<Zone>,
    hover: Option<Action>,
    close_hot: bool,
}

/// `items[i]` advanced by `step` with wrap-around.
pub fn cycle<T: PartialEq + Copy>(items: &[T], current: T, step: isize) -> T {
    let i = items.iter().position(|&x| x == current).unwrap_or(0) as isize;
    items[(i + step).rem_euclid(items.len() as isize) as usize]
}

impl Panel {
    pub fn new(hwnd: HWND, dpi: u32) -> Self {
        let mut painter = Painter::new();
        painter.close_gutter = true;
        painter.set_font(FONT_PX * dpi as i32 / 96, COLS);
        Self { hwnd, grid: Grid::new(COLS, ROWS), painter, zones: Vec::new(), hover: None, close_hot: false }
    }

    pub fn size(&self) -> (i32, i32) {
        self.painter.window_size(&self.grid)
    }

    pub fn paint(&self, hdc: HDC, width: i32, height: i32) {
        self.painter.paint(hdc, &self.grid, width, height, Some(self.close_hot));
    }

    /// Updates hover state for a client-area point (`None` = cursor left); true if it changed.
    pub fn hover(&mut self, pt: Option<POINT>, width: i32) -> bool {
        let (close_hot, hover) = match pt.map(|p| self.hit(p, width)) {
            Some(Some(Hit::Close)) => (true, None),
            Some(Some(Hit::Action(a))) => (false, Some(a)),
            _ => (false, None),
        };
        let changed = close_hot != self.close_hot || hover != self.hover;
        (self.close_hot, self.hover) = (close_hot, hover);
        changed
    }

    pub fn hit(&self, pt: POINT, width: i32) -> Option<Hit> {
        let r = self.painter.close_rect(width);
        if pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom {
            return Some(Hit::Close);
        }
        let (row, col) = self.painter.cell_at(pt)?;
        self.zones.iter().find(|z| z.row == row && z.cols.contains(&col)).map(|z| Hit::Action(z.action))
    }

    pub fn draw(&mut self, cfg: &Config, has_gpu: bool) {
        let g = &mut self.grid;
        self.zones.clear();
        g.clear();
        g.put(0, 0, "settings", ACCENT);

        g.put(1, 0, "layer", MUTED);
        for (i, (layer, _, label)) in Layer::ALL.iter().enumerate() {
            let row = 2 + i;
            self.zones.push(Zone { row, cols: 1..COLS, action: Action::Layer(*layer) });
            let hot = self.hover == Some(Action::Layer(*layer));
            let on = cfg.layer == *layer;
            g.put(row, 1, if on { "(•)" } else { "( )" }, if on { ACCENT } else { MUTED });
            g.put(row, 5, label, if hot { ACCENT } else { FG });
        }

        g.put(5, 0, "widget", MUTED);
        let checks = [
            (Action::CloseButton, "close button", cfg.show_close),
            (Action::Lock, "lock position", cfg.locked),
            (Action::Fahrenheit, "fahrenheit", cfg.fahrenheit),
        ];
        for (i, (action, label, on)) in checks.into_iter().enumerate() {
            let row = 6 + i;
            self.zones.push(Zone { row, cols: 1..COLS, action });
            check(g, row, 1, label, on, self.hover == Some(action));
        }

        g.put(9, 0, "rows", MUTED);
        let visible = (0..4).filter(|&i| cfg.rows[i] && (i != 1 || has_gpu)).count();
        for (i, name) in ROW_NAMES.iter().enumerate() {
            let (row, col) = (10 + i / 2, 1 + (i % 2) * 12);
            if i == 1 && !has_gpu {
                g.put(row, col, "[-] gpu", DIM);
                continue;
            }
            check(g, row, col, name, cfg.rows[i], self.hover == Some(Action::Row(i)));
            // The last visible row can't be hidden.
            if !(cfg.rows[i] && visible == 1) {
                self.zones.push(Zone { row, cols: col..col + 11, action: Action::Row(i) });
            }
        }

        let size = SIZES.iter().find(|(px, _)| *px == cfg.size).map_or("", |(_, l)| l);
        let opacity = format!("{}%", cfg.opacity);
        let refresh = format!("{}s", cfg.refresh);
        let selectors: [(&str, &str, fn(isize) -> Action); 3] = [
            ("opacity", &opacity, Action::Opacity),
            ("text size", size, Action::Size),
            ("refresh", &refresh, Action::Refresh),
        ];
        for (i, (label, value, action)) in selectors.into_iter().enumerate() {
            let row = 12 + i;
            g.put(row, 1, label, FG);
            self.zones.push(Zone { row, cols: PREV..VALUE, action: action(-1) });
            self.zones.push(Zone { row, cols: VALUE..COLS, action: action(1) });
            g.put(row, PREV, "‹", if self.hover == Some(action(-1)) { ACCENT } else { MUTED });
            g.put(row, VALUE, value, FG);
            g.put(row, NEXT, "›", if self.hover == Some(action(1)) { ACCENT } else { MUTED });
        }

        self.zones.push(Zone { row: 15, cols: 1..6, action: Action::Quit });
        g.put(15, 1, "quit", if self.hover == Some(Action::Quit) { HOT } else { MUTED });
    }
}

fn check(g: &mut Grid, row: usize, col: usize, label: &str, on: bool, hot: bool) {
    g.put(row, col, if on { "[x]" } else { "[ ]" }, if on { ACCENT } else { MUTED });
    g.put(row, col + 4, label, if hot { ACCENT } else { FG });
}
