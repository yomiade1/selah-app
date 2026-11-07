#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! `selah`: the launcher the Start menu shortcut runs. It starts the watchdog (which starts
//! everything else) and exits. Holding Shift starts Selah in safe mode (SAFE-07).
//! If Selah is already running, it does nothing (M4 brings the existing window to the front).
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // The installer (Velopack) runs the main program with `--veloapp-install`, `-updated`,
    // `-obsolete` or `-uninstall` and waits for it. Selah has nothing to do then, and must not
    // start: an install or uninstall never puts anything on the screens.
    if selah_watchdog::is_installer_hook(&args) {
        return;
    }
    // --wait (development and the installer test): stay attached and pass the exit code on.
    let wait = args.iter().any(|arg| arg == "--wait");
    args.retain(|arg| arg != "--wait");
    if selah_watchdog::keys::shift_held() && !args.iter().any(|arg| arg == "--safe-mode") {
        args.push("--safe-mode".into());
    }
    let watchdog = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!("selah-watchdog{}", std::env::consts::EXE_SUFFIX));
    let mut command = Command::new(watchdog);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    if !wait {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    if wait {
        let status = command.status();
        std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(1));
    }
    if command.spawn().is_err() {
        std::process::exit(1);
    }
}
