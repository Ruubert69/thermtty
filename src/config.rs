use std::path::PathBuf;

pub const OPACITIES: [u8; 5] = [100, 90, 80, 70, 60];
/// Font pixel heights at 96 DPI with their menu labels.
pub const SIZES: [(i32, &str); 3] = [(12, "Small"), (14, "Medium"), (16, "Large")];

pub struct Config {
    pub pos: Option<(i32, i32)>,
    pub topmost: bool,
    pub opacity: u8,
    pub size: i32,
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("thermtty").join("config.ini"))
    }

    pub fn load() -> Self {
        let mut c = Self { pos: None, topmost: true, opacity: 100, size: 14 };
        let text = Self::path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let (mut x, mut y) = (None, None);
        for (key, val) in text.lines().filter_map(|l| l.split_once('=')) {
            let val = val.trim();
            match key.trim() {
                "x" => x = val.parse().ok(),
                "y" => y = val.parse().ok(),
                "topmost" => c.topmost = val != "0",
                "opacity" => c.opacity = val.parse().ok().filter(|o| OPACITIES.contains(o)).unwrap_or(100),
                "size" => c.size = val.parse().ok().filter(|s| SIZES.iter().any(|(px, _)| px == s)).unwrap_or(14),
                _ => {}
            }
        }
        c.pos = x.zip(y);
        c
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        let mut text = format!("topmost={}\nopacity={}\nsize={}\n", self.topmost as u8, self.opacity, self.size);
        if let Some((x, y)) = self.pos {
            text += &format!("x={x}\ny={y}\n");
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}
