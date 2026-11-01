//! NVIDIA GPU temperature and load through NVAPI (nvapi64.dll, ships with the driver).
//!
//! NVAPI rather than NVML: initializing NVML costs ~20 MiB of private memory that
//! unloading does not return; NVAPI costs ~3 MiB. Function IDs and struct layouts are the
//! ones LibreHardwareMonitor uses.

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::wide;

type Handle = usize;

const ID_INITIALIZE: u32 = 0x0150_E828;
const ID_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
const ID_GET_THERMAL_SETTINGS: u32 = 0xE364_0A56;
const ID_GET_DYNAMIC_PSTATES_INFO_EX: u32 = 0x60DE_D2ED;
const THERMAL_TARGET_ALL: u32 = 15;

/// Struct sizes in u32 words with their NVAPI version numbers.
const THERMAL: (usize, u32) = (17, 2);
const PSTATES: (usize, u32) = (18, 1);

#[derive(Clone, Copy, Default)]
pub struct GpuSample {
    pub temp: Option<f32>,
    pub load: Option<f32>,
}

pub struct Gpu {
    gpu: Handle,
    thermal: unsafe extern "C" fn(Handle, u32, *mut u32) -> i32,
    pstates: unsafe extern "C" fn(Handle, *mut u32) -> i32,
}

/// Fills `buf` with zeroes and stamps the NVAPI struct version.
fn prepare<const N: usize>(buf: &mut [u32; N], version: u32) -> *mut u32 {
    *buf = [0; N];
    buf[0] = (N * 4) as u32 | version << 16;
    buf.as_mut_ptr()
}

impl Gpu {
    pub fn open() -> Option<Self> {
        unsafe {
            let lib = LoadLibraryW(wide("nvapi64.dll").as_ptr());
            if lib.is_null() {
                return None;
            }
            let query: unsafe extern "C" fn(u32) -> usize =
                std::mem::transmute(GetProcAddress(lib, c"nvapi_QueryInterface".as_ptr().cast())?);
            macro_rules! func {
                ($id:expr) => {{
                    let f = query($id);
                    if f == 0 {
                        return None;
                    }
                    std::mem::transmute::<usize, _>(f)
                }};
            }
            let init: unsafe extern "C" fn() -> i32 = func!(ID_INITIALIZE);
            let enumerate: unsafe extern "C" fn(*mut [Handle; 64], *mut u32) -> i32 = func!(ID_ENUM_PHYSICAL_GPUS);
            let (mut gpus, mut count) = ([0; 64], 0);
            if init() != 0 || enumerate(&mut gpus, &mut count) != 0 || count == 0 {
                return None;
            }
            Some(Self {
                gpu: gpus[0],
                thermal: func!(ID_GET_THERMAL_SETTINGS),
                pstates: func!(ID_GET_DYNAMIC_PSTATES_INFO_EX),
            })
        }
    }

    pub fn sample(&self) -> GpuSample {
        let mut s = GpuSample::default();
        unsafe {
            // NV_GPU_THERMAL_SETTINGS v2: version, count, sensor[3] {controller, min, max, current, target}
            let mut th = [0u32; THERMAL.0];
            if (self.thermal)(self.gpu, THERMAL_TARGET_ALL, prepare(&mut th, THERMAL.1)) == 0 && th[1] > 0 {
                s.temp = Some(th[5] as i32 as f32);
            }
            // NV_GPU_DYNAMIC_PSTATES_INFO_EX: version, flags, utilization[8] {present, percent}; [0] = graphics
            let mut ps = [0u32; PSTATES.0];
            if (self.pstates)(self.gpu, prepare(&mut ps, PSTATES.1)) == 0 && ps[2] & 1 != 0 {
                s.load = Some(ps[3] as f32);
            }
        }
        s
    }
}
