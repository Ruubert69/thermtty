//! Memory and network throughput via plain Win32 APIs.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::time::Instant;

use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// `(used, total)` physical memory in bytes.
pub fn memory() -> (f64, f64) {
    let mut m: MEMORYSTATUSEX = unsafe { zeroed() };
    m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    unsafe { GlobalMemoryStatusEx(&mut m) };
    ((m.ullTotalPhys - m.ullAvailPhys) as f64, m.ullTotalPhys as f64)
}

/// Total receive/transmit rate over physical, connected adapters.
#[derive(Default)]
pub struct Net {
    last: Option<(u64, u64, Instant)>,
}

impl Net {
    /// `(down, up)` in bytes per second; `None` until two samples exist.
    pub fn sample(&mut self) -> (Option<f32>, Option<f32>) {
        let (mut rx, mut tx) = (0u64, 0u64);
        unsafe {
            let mut table: *mut MIB_IF_TABLE2 = null_mut();
            if GetIfTable2(&mut table) != 0 {
                return (None, None);
            }
            let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            for row in rows {
                // Bit 0 = HardwareInterface, bit 1 = FilterInterface (LWF duplicates of the same NIC).
                let flags = row.InterfaceAndOperStatusFlags._bitfield;
                if flags & 0b01 != 0 && flags & 0b10 == 0 && row.OperStatus == IfOperStatusUp {
                    rx += row.InOctets;
                    tx += row.OutOctets;
                }
            }
            FreeMibTable(table.cast());
        }
        let now = Instant::now();
        let Some((prx, ptx, at)) = self.last.replace((rx, tx, now)) else {
            return (None, None);
        };
        let secs = (now - at).as_secs_f32();
        if secs <= 0.0 || rx < prx || tx < ptx {
            return (None, None); // adapter set changed; skip one sample
        }
        (Some((rx - prx) as f32 / secs), Some((tx - ptx) as f32 / secs))
    }
}
