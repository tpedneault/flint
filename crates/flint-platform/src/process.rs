//! Facts about this process for `flint status`.

/// Resident memory (working set) in bytes, `None` where it can't be read.
/// The budget is 120 MB (DESIGN.md, "Performance budget").
pub fn resident_bytes() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: zeroed counters are a valid out-parameter; the size
        // passed matches the struct.
        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            (GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) != 0)
                .then_some(counters.WorkingSetSize as u64)
        }
    }
    #[cfg(unix)]
    {
        // `/proc/self/statm`: sizes in pages; the second is resident.
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: sysconf has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        u64::try_from(page).ok().map(|page| pages * page)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_process_uses_some_memory() {
        let bytes = super::resident_bytes().expect("readable on Windows and Linux");
        assert!(bytes > 1024 * 1024, "{bytes}");
    }
}
