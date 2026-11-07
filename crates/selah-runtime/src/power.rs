#![allow(unsafe_code)]
//! Keep the computer awake while something is on the screens (SAFE-06).
//!
//! - Windows: a dedicated thread holds `SetThreadExecutionState(ES_CONTINUOUS |
//!   ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)` while live. The state belongs to the thread that
//!   set it, so one thread owns it for the life of the process. There's no safe API for it.
//! - macOS: `/usr/bin/caffeinate -d -i -w <pid>` runs while live and is stopped when nothing is.
//!   `-w` ends it if Selah exits.
//! - Linux (development only): nothing.

/// Something that can keep the computer and its displays awake.
pub trait KeepAwake: Send {
    fn set(&mut self, awake: bool);
}

/// Does nothing (tests, Linux).
#[derive(Default)]
pub struct NoKeepAwake;

impl KeepAwake for NoKeepAwake {
    fn set(&mut self, _awake: bool) {}
}

/// The platform's way of staying awake.
pub fn platform() -> Box<dyn KeepAwake> {
    #[cfg(windows)]
    return Box::new(windows::ExecutionState::start());
    #[cfg(target_os = "macos")]
    return Box::new(macos::Caffeinate::default());
    #[cfg(not(any(windows, target_os = "macos")))]
    Box::new(NoKeepAwake)
}

#[cfg(windows)]
mod windows {
    use std::sync::mpsc;

    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };

    pub struct ExecutionState {
        sender: mpsc::Sender<bool>,
        last: Option<bool>,
    }

    impl ExecutionState {
        pub fn start() -> Self {
            let (sender, receiver) = mpsc::channel::<bool>();
            let spawned = std::thread::Builder::new()
                .name("selah-keep-awake".into())
                .spawn(move || {
                    while let Ok(awake) = receiver.recv() {
                        let flags = if awake {
                            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
                        } else {
                            ES_CONTINUOUS
                        };
                        // SAFETY: SetThreadExecutionState only changes this thread's power
                        // request; any combination of these documented flags is valid.
                        let previous = unsafe { SetThreadExecutionState(flags) };
                        if previous == 0 {
                            tracing::warn!(awake, "couldn't change the sleep setting");
                        } else {
                            tracing::info!(awake, "keep-awake changed (SAFE-06)");
                        }
                    }
                });
            if let Err(error) = spawned {
                tracing::warn!(%error, "couldn't start the keep-awake thread");
            }
            Self { sender, last: None }
        }
    }

    impl super::KeepAwake for ExecutionState {
        fn set(&mut self, awake: bool) {
            if self.last != Some(awake) && self.sender.send(awake).is_ok() {
                self.last = Some(awake);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    #[derive(Default)]
    pub struct Caffeinate {
        child: Option<std::process::Child>,
    }

    impl super::KeepAwake for Caffeinate {
        fn set(&mut self, awake: bool) {
            if awake && self.child.is_none() {
                match std::process::Command::new("/usr/bin/caffeinate")
                    .args(["-d", "-i", "-w", &std::process::id().to_string()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(child) => {
                        tracing::info!("keep-awake on (SAFE-06)");
                        self.child = Some(child);
                    }
                    Err(error) => tracing::warn!(%error, "couldn't start caffeinate"),
                }
            } else if !awake && let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
                tracing::info!("keep-awake off");
            }
        }
    }

    impl Drop for Caffeinate {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_platform_keep_awake_turns_on_and_off() {
        // safe: SAFE-06. Real call on Windows and macOS; nothing on Linux.
        let mut keep = platform();
        keep.set(true);
        keep.set(true);
        keep.set(false);
    }
}
