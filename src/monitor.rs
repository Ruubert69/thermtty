//! Sampling, temperature history and the four-row layout.

use std::fmt;

use crate::cpu::{Cpu, CpuSample};
use crate::gpu::{Gpu, GpuSample};
use crate::grid::*;
use crate::sys::{self, Net};

pub const COLS: usize = 29;
/// Right edges of the two value columns.
const A_END: usize = 9;
const B_END: usize = 15;
const GRAPH: usize = 17;
const GRAPH_W: usize = COLS - GRAPH;
/// Samples (seconds) folded into one sparkline column; the graph spans `GRAPH_W * BUCKET` s.
const BUCKET: usize = 5;
const RING: usize = (GRAPH_W + 1) * BUCKET;
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// One-sample-per-tick history for a sparkline.
struct History {
    buf: [f32; RING],
    count: usize,
}

impl History {
    fn new() -> Self {
        Self { buf: [f32::NAN; RING], count: 0 }
    }

    fn push(&mut self, v: Option<f32>) {
        self.buf[self.count % RING] = v.unwrap_or(f32::NAN);
        self.count += 1;
    }

    /// Max of the samples in sparkline column `col` (0 = oldest). Columns are anchored to
    /// absolute sample numbers so the graph shifts once per bucket instead of every tick.
    fn column(&self, col: usize) -> Option<f32> {
        if self.count == 0 {
            return None;
        }
        let last_bucket = (self.count - 1) / BUCKET;
        let bucket = (last_bucket + col + 1).checked_sub(GRAPH_W)?;
        let start = bucket * BUCKET;
        (start..(start + BUCKET).min(self.count))
            .filter(|&i| i + RING >= self.count)
            .map(|i| self.buf[i % RING])
            .filter(|v| !v.is_nan())
            .reduce(f32::max)
    }

    fn window_max(&self) -> Option<f32> {
        (0..GRAPH_W).filter_map(|c| self.column(c)).reduce(f32::max)
    }
}

pub struct Monitor {
    cpu: Cpu,
    gpu: Option<Gpu>,
    net: Net,
    cpu_now: CpuSample,
    gpu_now: GpuSample,
    mem: (f64, f64),
    rx: Option<f32>,
    tx: Option<f32>,
    cpu_temp: History,
    gpu_temp: History,
    traffic: History,
}

impl Monitor {
    pub fn new() -> Self {
        let mut cpu = Cpu::new();
        cpu.sample(); // establish the load baseline
        let mut net = Net::default();
        net.sample();
        Self {
            cpu,
            gpu: Gpu::open(),
            net,
            cpu_now: CpuSample::default(),
            gpu_now: GpuSample::default(),
            mem: sys::memory(),
            rx: None,
            tx: None,
            cpu_temp: History::new(),
            gpu_temp: History::new(),
            traffic: History::new(),
        }
    }

    pub fn rows(&self) -> usize {
        3 + usize::from(self.gpu.is_some())
    }

    /// Call once per second.
    pub fn tick(&mut self) {
        self.cpu_now = self.cpu.sample();
        self.cpu_temp.push(self.cpu_now.temp);
        if let Some(gpu) = &self.gpu {
            self.gpu_now = gpu.sample();
            self.gpu_temp.push(self.gpu_now.temp);
        }
        self.mem = sys::memory();
        (self.rx, self.tx) = self.net.sample();
        self.traffic.push(self.rx.zip(self.tx).map(|(r, t)| r + t));
    }

    pub fn draw(&self, g: &mut Grid) {
        g.clear();
        let mut row = 0;
        let cpu_heat = |t: f32| heat(t, 70.0, 85.0);
        g.put(row, 0, "cpu", MUTED);
        temp_row(g, row, self.cpu_now.temp, self.cpu_now.load, &self.cpu_temp, cpu_heat);

        if self.gpu.is_some() {
            row += 1;
            g.put(row, 0, "gpu", MUTED);
            temp_row(g, row, self.gpu_now.temp, self.gpu_now.load, &self.gpu_temp, |t| heat(t, 70.0, 83.0));
        }

        row += 1;
        let (used, total) = self.mem;
        let pct = (used / total * 100.0) as f32;
        g.put(row, 0, "ram", MUTED);
        g.putf_r(row, A_END, FG, format_args!("{:.1}G", used / GIB));
        g.putf_r(row, B_END, load_color(pct), format_args!("{pct:.0}%"));
        let filled = (pct / 100.0 * GRAPH_W as f32).round() as usize;
        for col in 0..GRAPH_W {
            let (ch, c) = if col < filled { ("━", load_color(pct)) } else { ("─", DIM) };
            g.put(row, GRAPH + col, ch, c);
        }

        row += 1;
        g.put(row, 0, "net", MUTED);
        if let (Some(rx), Some(tx)) = (self.rx, self.tx) {
            g.putf_r(row, A_END, FG, format_args!("↓{}", Rate(rx)));
            g.putf_r(row, B_END, FG, format_args!("↑{}", Rate(tx)));
        }
        let hi = self.traffic.window_max().unwrap_or(0.0).max(64.0 * 1024.0);
        sparkline(g, row, &self.traffic, 0.0, hi, |_| ACCENT);
    }
}

/// Temperature, load, and the temperature sparkline.
fn temp_row(g: &mut Grid, row: usize, temp: Option<f32>, load: Option<f32>, hist: &History, heat: impl Fn(f32) -> Color) {
    match temp {
        Some(t) => g.putf_r(row, A_END, heat(t), format_args!("{t:.0}°")),
        None => g.put_r(row, A_END, "--", MUTED),
    };
    match load {
        Some(l) => g.putf_r(row, B_END, load_color(l), format_args!("{l:.0}%")),
        None => g.put_r(row, B_END, "--", MUTED),
    };
    sparkline(g, row, hist, 30.0, 95.0, heat);
}

fn sparkline(g: &mut Grid, row: usize, h: &History, lo: f32, hi: f32, color: impl Fn(f32) -> Color) {
    let mut buf = [0u8; 4];
    for col in 0..GRAPH_W {
        let (ch, c) = match h.column(col) {
            Some(v) => {
                let level = ((v - lo) / (hi - lo) * SPARK.len() as f32).clamp(0.0, SPARK.len() as f32 - 1.0);
                (SPARK[level as usize], color(v))
            }
            None => (SPARK[0], DIM),
        };
        g.put(row, GRAPH + col, ch.encode_utf8(&mut buf), c);
    }
}

fn heat(v: f32, warn: f32, hot: f32) -> Color {
    if v >= hot {
        HOT
    } else if v >= warn {
        WARN
    } else {
        ACCENT
    }
}

fn load_color(v: f32) -> Color {
    heat(v, 60.0, 85.0)
}

/// Compact byte rate: `512B`, `48K`, `1.2M`, `34M`, `1.1G`.
struct Rate(f32);

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut v = self.0;
        for unit in ["B", "K", "M", "G"] {
            if v < 1000.0 || unit == "G" {
                return if v < 10.0 && unit != "B" { write!(f, "{v:.1}{unit}") } else { write!(f, "{v:.0}{unit}") };
            }
            v /= 1024.0;
        }
        unreachable!()
    }
}
