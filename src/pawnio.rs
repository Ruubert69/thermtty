//! Minimal client for the PawnIO driver (https://pawnio.eu), used for AMD SMN/MSR reads.

use std::ffi::CStr;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::wide;

type OpenFn = unsafe extern "system" fn(*mut HANDLE) -> i32;
type LoadFn = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
type ExecFn = unsafe extern "system" fn(HANDLE, *const u8, *const u64, usize, *mut u64, usize, *mut usize) -> i32;

/// `AMDFamily17` module (families 17h-1Ah) from namazso/PawnIO.Modules 0.2.11, LGPL-2.1.
static AMD_FAMILY_17: &[u8] = include_bytes!("../assets/AMDFamily17.bin");

pub struct PawnIo {
    handle: HANDLE,
    execute: ExecFn,
    /// System-wide PCI config lock shared with other monitoring tools (HWiNFO, LHM, ...).
    pci_lock: HANDLE,
}

impl PawnIo {
    /// Opens the driver and loads the AMD module. `None` when PawnIO is missing, the
    /// process is not elevated, or the CPU is not AMD family 17h-1Ah.
    pub fn open_amd() -> Option<Self> {
        let dir = std::env::var("ProgramFiles").ok()?;
        unsafe {
            let lib = LoadLibraryW(wide(&format!("{dir}\\PawnIO\\PawnIOLib.dll")).as_ptr());
            if lib.is_null() {
                return None;
            }
            let open: OpenFn = std::mem::transmute(GetProcAddress(lib, c"pawnio_open".as_ptr().cast())?);
            let load: LoadFn = std::mem::transmute(GetProcAddress(lib, c"pawnio_load".as_ptr().cast())?);
            let execute: ExecFn = std::mem::transmute(GetProcAddress(lib, c"pawnio_execute".as_ptr().cast())?);
            let mut handle = null_mut();
            if open(&mut handle) < 0 || load(handle, AMD_FAMILY_17.as_ptr(), AMD_FAMILY_17.len()) < 0 {
                return None;
            }
            let pci_lock = CreateMutexW(null(), 0, wide("Global\\Access_PCI").as_ptr());
            Some(Self { handle, execute, pci_lock })
        }
    }

    fn call(&self, name: &CStr, arg: u64) -> Option<u64> {
        let (mut out, mut written) = (0u64, 0usize);
        let hr = unsafe { (self.execute)(self.handle, name.as_ptr().cast(), &arg, 1, &mut out, 1, &mut written) };
        (hr >= 0 && written == 1).then_some(out)
    }

    pub fn read_smn(&self, addr: u32) -> Option<u32> {
        if !self.pci_lock.is_null() {
            let wait = unsafe { WaitForSingleObject(self.pci_lock, 100) };
            if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                return None;
            }
        }
        let value = self.call(c"ioctl_read_smn", addr.into());
        if !self.pci_lock.is_null() {
            unsafe { ReleaseMutex(self.pci_lock) };
        }
        value.map(|v| v as u32)
    }
}
