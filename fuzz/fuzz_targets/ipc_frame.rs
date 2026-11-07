#![no_main]
#![forbid(unsafe_code)]
//! The IPC frame decoder must never crash, hang or over-allocate on any input (SEC-02, SEC-03).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = selah_ipc::frame::fuzz_frames(data);
});
