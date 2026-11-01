#![windows_subsystem = "windows"]

mod config;
mod cpu;
mod gpu;
mod grid;
mod monitor;
mod pawnio;
mod settings;
mod sys;
mod window;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        // Writes one rendered frame as text, for checking sensors without the window.
        [flag, path] if flag == "--dump" => {
            let mut mon = monitor::Monitor::new();
            std::thread::sleep(std::time::Duration::from_secs(1));
            mon.tick();
            let mut grid = grid::Grid::new(monitor::COLS, mon.rows());
            mon.draw(&mut grid);
            if let Err(e) = std::fs::write(path, grid.text()) {
                std::process::exit(e.raw_os_error().unwrap_or(1));
            }
        }
        _ => window::run(),
    }
}
