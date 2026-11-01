use std::path::PathBuf;

pub const OPACITIES: [u8; 5] = [100, 90, 80, 70, 60];
/// Font pixel heights at 96 DPI with their labels.
pub const SIZES: [(i32, &str); 3] = [(12, "small"), (14, "medium"), (16, "large")];
/// Sampling interval in seconds.
pub const REFRESH: [u32; 3] = [1, 2, 5];
pub const ROW_NAMES: [&str; 4] = ["cpu", "gpu", "ram", "net"];

#[derive(Clone, Copy, PartialEq)]
pub enum Layer {
    /// Above every window.
    Top,
    /// Ordinary z-order.
    Normal,
    /// Below every window, like a desktop gadget.
    Desktop,
}

impl Layer {
    pub const ALL: [(Layer, &str, &str); 3] = [
        (Layer::Top, "top", "always on top"),
        (Layer::Normal, "normal", "normal window"),
        (Layer::Desktop, "desktop", "keep on desktop"),
    ];
}

pub struct Config {
    pub pos: Option<(i32, i32)>,
    pub layer: Layer,
    pub opacity: u8,
    pub size: i32,
    pub show_close: bool,
    pub locked: bool,
    pub fahrenheit: bool,
    pub refresh: u32,
    /// Visibility of the cpu, gpu, ram and net rows.
    pub rows: [bool; 4],
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("thermtty").join("config.ini"))
    }

    pub fn load() -> Self {
        let mut c = Self {
            pos: None,
            layer: Layer::Top,
            opacity: 100,
            size: 14,
            show_close: true,
            locked: false,
            fahrenheit: false,
            refresh: 1,
            rows: [true; 4],
        };
        let text = Self::path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let (mut x, mut y) = (None, None);
        for (key, val) in text.lines().filter_map(|l| l.split_once('=')) {
            let val = val.trim();
            match key.trim() {
                "x" => x = val.parse().ok(),
                "y" => y = val.parse().ok(),
                "layer" => {
                    if let Some(&(l, _, _)) = Layer::ALL.iter().find(|(_, key, _)| *key == val) {
                        c.layer = l;
                    }
                }
                "opacity" => c.opacity = val.parse().ok().filter(|o| OPACITIES.contains(o)).unwrap_or(c.opacity),
                "size" => c.size = val.parse().ok().filter(|s| SIZES.iter().any(|(px, _)| px == s)).unwrap_or(c.size),
                "refresh" => c.refresh = val.parse().ok().filter(|r| REFRESH.contains(r)).unwrap_or(c.refresh),
                "close_button" => c.show_close = val != "0",
                "locked" => c.locked = val == "1",
                "fahrenheit" => c.fahrenheit = val == "1",
                "rows" => {
                    let shown: Vec<&str> = val.split(',').map(str::trim).collect();
                    c.rows = ROW_NAMES.map(|name| shown.contains(&name));
                }
                _ => {}
            }
        }
        c.pos = x.zip(y);
        c
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        let layer = Layer::ALL.iter().find(|(l, _, _)| *l == self.layer).map_or("top", |(_, key, _)| key);
        let rows: Vec<&str> = ROW_NAMES.iter().zip(self.rows).filter(|(_, on)| *on).map(|(n, _)| *n).collect();
        let mut text = format!(
            "layer={layer}\nopacity={}\nsize={}\nrefresh={}\nclose_button={}\nlocked={}\nfahrenheit={}\nrows={}\n",
            self.opacity,
            self.size,
            self.refresh,
            self.show_close as u8,
            self.locked as u8,
            self.fahrenheit as u8,
            rows.join(","),
        );
        if let Some((x, y)) = self.pos {
            text += &format!("x={x}\ny={y}\n");
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}
