#![no_main]
#![forbid(unsafe_code)]
//! The `.selah` bundle parser must never crash, hang, or over-allocate on any input (SEC-03, M2-A11).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = selah_core::library::fuzz_selah_bundle(data);
});
