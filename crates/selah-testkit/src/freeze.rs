#![allow(unsafe_code)]
//! Freeze and thaw a process for chaos tests (SAFE-03: "with core frozen"). Unix uses
//! `kill -STOP`/`-CONT`. Windows suspends every thread of the process (Toolhelp snapshot,
//! `SuspendThread`/`ResumeThread`), which has no safe API. Test-only.

/// Stop every thread of `pid` until [`thaw`].
pub fn freeze(pid: u32) {
    #[cfg(unix)]
    signal("-STOP", pid);
    #[cfg(windows)]
    windows::each_thread(pid, true);
}

/// Let a frozen process run again.
pub fn thaw(pid: u32) {
    #[cfg(unix)]
    signal("-CONT", pid);
    #[cfg(windows)]
    windows::each_thread(pid, false);
}

#[cfg(unix)]
fn signal(signal: &str, pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status();
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
    };

    pub fn each_thread(pid: u32, suspend: bool) {
        // SAFETY: a snapshot of all threads; the handle is checked and closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return;
        }
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        // SAFETY: `snapshot` is valid and `entry.dwSize` is set as the API requires.
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: opens a thread of the target process with only suspend/resume rights;
                // a null handle (no access, thread gone) is skipped.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // SAFETY: `thread` is a valid handle with THREAD_SUSPEND_RESUME, closed once.
                    unsafe {
                        if suspend {
                            SuspendThread(thread);
                        } else {
                            ResumeThread(thread);
                        }
                        CloseHandle(thread);
                    }
                }
            }
            // SAFETY: as for Thread32First.
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        // SAFETY: closes the snapshot handle opened above, once.
        unsafe { CloseHandle(snapshot) };
    }
}
