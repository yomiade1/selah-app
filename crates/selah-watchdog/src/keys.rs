//! Is Shift held while Selah starts? Then start in safe mode (SAFE-07).
//!
//! Windows asks the keyboard state. macOS support needs the Quartz event API and comes with
//! the macOS launcher (M6); until then the **Start in safe mode** shortcut flag works there.
//! This module is listed in `tools/unsafe-allowlist.txt` for the Windows call.
#![allow(unsafe_code)]

#[cfg(windows)]
pub fn shift_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    // SAFETY: GetAsyncKeyState takes a virtual-key code and has no pointer arguments.
    let state = unsafe { GetAsyncKeyState(i32::from(VK_SHIFT)) };
    (state as u16) & 0x8000 != 0
}

#[cfg(not(windows))]
pub fn shift_held() -> bool {
    false
}
