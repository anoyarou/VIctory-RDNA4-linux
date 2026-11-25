//! Writes our own bytes over a few instructions of a mapped image, before
//! the title runs (the title's own watchdog, `victory_game::KnownBuild::patches`).

use std::ffi::c_void;

use anyhow::{Result, bail};
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// Writes `bytes` at `base + rva`, inside an image of `size` bytes, and
/// restores the page protection. Returns the bytes that were there, for the
/// log only (they are the title's code: never stored).
pub fn patch_code(base: usize, size: usize, rva: usize, bytes: &[u8]) -> Result<Vec<u8>> {
    if rva.checked_add(bytes.len()).is_none_or(|end| end > size) {
        bail!(
            "code patch at {rva:#x} ({} bytes) is outside the image",
            bytes.len()
        );
    }
    let at = (base + rva) as *mut u8;
    let mut old = 0u32;
    // SAFETY: inside the mapped image, checked above.
    if unsafe {
        VirtualProtect(
            at as *const c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        )
    } == 0
    {
        bail!(
            "VirtualProtect for the code patch at {rva:#x} failed: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: the range is writable now and nothing runs the title yet.
    let before = unsafe {
        let before = std::slice::from_raw_parts(at, bytes.len()).to_vec();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
        VirtualProtect(at as *const c_void, bytes.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at as *const c_void, bytes.len());
        before
    };
    Ok(before)
}
