use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::pawnio::PawnIo;

/// THM_TCON_CUR_TMP: Tctl, bits 31:21 in 0.125 °C steps, bit 19 = range select (-49 °C).
const SMN_TCTL: u32 = 0x0005_9800;
const MAX_CCDS: u32 = 8;

#[derive(Clone, Copy, Default)]
pub struct CpuSample {
    pub load: Option<f32>,
    pub temp: Option<f32>,
}

pub struct Cpu {
    pawn: Option<PawnIo>,
    /// SMN address of CCD0's temperature register; CCDn is at `+ 4 * n`.
    ccd_base: u32,
    ccd_mask: u32,
    last_times: (u64, u64),
}

impl Cpu {
    pub fn new() -> Self {
        let pawn = PawnIo::open_amd();
        let ccd_base = ccd_base();
        let ccd_mask = pawn.as_ref().map_or(0, |p| {
            (0..MAX_CCDS)
                .filter(|&i| p.read_smn(ccd_base + 4 * i).and_then(ccd_temp).is_some())
                .fold(0, |m, i| m | 1 << i)
        });
        Self { pawn, ccd_base, ccd_mask, last_times: system_times() }
    }

    pub fn sample(&mut self) -> CpuSample {
        let (idle, total) = system_times();
        let (prev_idle, prev_total) = std::mem::replace(&mut self.last_times, (idle, total));
        let dt = total.wrapping_sub(prev_total);
        let load = (dt > 0).then(|| (1.0 - idle.wrapping_sub(prev_idle) as f32 / dt as f32).clamp(0.0, 1.0) * 100.0);

        let Some(p) = &self.pawn else {
            return CpuSample { load, temp: None };
        };
        // Report the hottest of Tctl and the CCD dies, like most monitoring tools' "CPU" reading.
        let tctl = p.read_smn(SMN_TCTL).map(|raw| {
            let t = ((raw >> 21) & 0x7FF) as f32 * 0.125;
            if raw & (1 << 19) != 0 { t - 49.0 } else { t }
        });
        let ccd = (0..MAX_CCDS)
            .filter(|i| self.ccd_mask & (1 << i) != 0)
            .filter_map(|i| p.read_smn(self.ccd_base + 4 * i).and_then(ccd_temp))
            .reduce(f32::max);
        let temp = match (tctl, ccd) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        CpuSample { load, temp }
    }
}

fn ccd_temp(raw: u32) -> Option<f32> {
    let raw = raw & 0xFFF;
    let t = raw as f32 * 0.125 - 305.0;
    (raw > 0 && t > 0.0 && t < 125.0).then_some(t)
}

fn ccd_base() -> u32 {
    let eax = std::arch::x86_64::__cpuid(1).eax;
    let base_family = (eax >> 8) & 0xF;
    let family = base_family + if base_family == 0xF { (eax >> 20) & 0xFF } else { 0 };
    let model = ((eax >> 4) & 0xF) | (((eax >> 16) & 0xF) << 4);
    match (family, model) {
        // Zen 4 / Zen 5 moved the CCD temperature block.
        (0x19, 0x10..=0x1F | 0x60..=0x7F | 0xA0..=0xAF) | (0x1A, _) => 0x0005_9B08,
        _ => 0x0005_9954,
    }
}

/// `(idle, kernel + user)` in 100 ns units; kernel time includes idle time.
fn system_times() -> (u64, u64) {
    let mut t = [FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }; 3];
    let [idle, kernel, user] = &mut t;
    unsafe { GetSystemTimes(idle, kernel, user) };
    let v = |f: &FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    (v(&t[0]), v(&t[1]) + v(&t[2]))
}
