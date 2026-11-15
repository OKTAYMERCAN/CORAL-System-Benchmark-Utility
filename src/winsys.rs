//! Windows platform layer: thin wrappers over the few Win32 calls SysCheck needs
//! (console colors, CPU affinity, memory, disk space, CPU topology).
//! Only compiled on Windows. Every function fails soft: `None` / empty / `false` on error.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW};
use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode, SetConsoleOutputCP, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformation, GlobalMemoryStatusEx, CacheData, CacheInstruction, CacheUnified, RelationCache, RelationProcessorCore, RelationProcessorPackage, MEMORYSTATUSEX,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, GetProcessAffinityMask, IsProcessorFeaturePresent, SetThreadAffinityMask, PF_VIRT_FIRMWARE_ENABLED};

fn wide(p: &Path) -> Vec<u16> {
    // relative paths are fine for these calls, but make them absolute so the volume lookup is exact
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf()) };
    abs.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// Switch the console to UTF-8 and enable ANSI escape sequences. Returns false when the
/// escape-sequence mode could not be turned on (very old Windows): then colors must stay off.
pub fn enable_ansi() -> bool {
    let mut ok = true;
    // SAFETY: plain console API calls with handles obtained from GetStdHandle.
    unsafe {
        SetConsoleOutputCP(65001);
        for id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let h = GetStdHandle(id);
            if h.is_null() || h as isize == -1 {
                continue;
            }
            let mut mode = 0u32;
            if GetConsoleMode(h, &mut mode) == 0 {
                continue; // not a console (redirected): nothing to enable
            }
            if SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0 {
                ok = false;
            }
        }
    }
    ok
}

/// CPUs of the current processor group this process may use (a mask covers at most 64 CPUs).
pub fn allowed_cpus() -> Vec<u32> {
    let (mut p, mut s) = (0usize, 0usize);
    // SAFETY: valid pointers to two usize out-values.
    let ok = unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut p, &mut s) } != 0;
    if !ok || p == 0 {
        return Vec::new();
    }
    (0..usize::BITS).filter(|i| ((p >> i) & 1) == 1).collect()
}

/// Pin the calling thread to one CPU of the current processor group.
pub fn pin_to_cpu(cpu: u32) -> bool {
    if cpu >= usize::BITS {
        return false;
    }
    // SAFETY: pseudo handle of the current thread; the mask has a single bit.
    unsafe { SetThreadAffinityMask(GetCurrentThread(), 1usize << cpu) != 0 }
}

/// (total physical bytes, available physical bytes, page-file bytes)
pub fn memory() -> Option<(u64, u64, u64)> {
    // SAFETY: zeroed struct with dwLength set as the API requires.
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) == 0 {
            return None;
        }
        Some((m.ullTotalPhys, m.ullAvailPhys, m.ullTotalPageFile.saturating_sub(m.ullTotalPhys)))
    }
}

/// (total bytes, bytes available to this user) of the volume holding `path`.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    let w = wide(path);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: NUL-terminated wide path and three valid out-pointers.
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } != 0;
    ok.then_some((total, avail))
}

/// File system name of the volume holding `path`, lower case ("ntfs", "refs", "exfat", "fat32").
pub fn fs_name(path: &Path) -> Option<String> {
    let w = wide(path);
    let mut root = [0u16; 261];
    let mut name = [0u16; 64];
    // SAFETY: buffers are large enough for the sizes passed; unused out-parameters are null.
    unsafe {
        if GetVolumePathNameW(w.as_ptr(), root.as_mut_ptr(), root.len() as u32) == 0 {
            return None;
        }
        if GetVolumeInformationW(root.as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), name.as_mut_ptr(), name.len() as u32) == 0 {
            return None;
        }
    }
    let n = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..n]).to_lowercase())
}

/// Whether virtualization is enabled in the firmware (reported even when Hyper-V hides VT-x).
pub fn virt_firmware_enabled() -> bool {
    // SAFETY: simple query.
    unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) != 0 }
}

pub struct Topology {
    pub physical: u32,
    pub packages: u32,
    /// (level, kind, size in bytes), one entry per distinct (level, kind)
    pub caches: Vec<(u8, &'static str, u64)>,
}

pub fn topology() -> Option<Topology> {
    let item = std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
    let mut len = 0u32;
    // SAFETY: first call only asks for the required buffer size (fails with INSUFFICIENT_BUFFER).
    unsafe { GetLogicalProcessorInformation(std::ptr::null_mut(), &mut len) };
    if (len as usize) < item {
        return None;
    }
    // SAFETY: all-zero is a valid value for this plain C struct/union.
    let mut v: Vec<SYSTEM_LOGICAL_PROCESSOR_INFORMATION> = vec![unsafe { std::mem::zeroed() }; len as usize / item];
    let mut len2 = (v.len() * item) as u32;
    // SAFETY: buffer holds `len2` bytes.
    if unsafe { GetLogicalProcessorInformation(v.as_mut_ptr(), &mut len2) } == 0 {
        return None;
    }
    let n = (len2 as usize / item).min(v.len());
    let (mut physical, mut packages) = (0u32, 0u32);
    let mut caches: Vec<(u8, &'static str, u64)> = Vec::new();
    for e in &v[..n] {
        match e.Relationship {
            RelationProcessorCore => physical += 1,
            RelationProcessorPackage => packages += 1,
            RelationCache => {
                // SAFETY: the Cache member is the valid one for RelationCache entries.
                let c = unsafe { e.Anonymous.Cache };
                let kind = match c.Type {
                    CacheInstruction => "Instruction",
                    CacheData => "Data",
                    CacheUnified => "Unified",
                    _ => continue,
                };
                if !caches.iter().any(|(l, k, _)| *l == c.Level && *k == kind) {
                    caches.push((c.Level, kind, c.Size as u64));
                }
            }
            _ => {}
        }
    }
    Some(Topology { physical, packages, caches })
}
