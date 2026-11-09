#![no_main]
#![forbid(unsafe_code)]
//! The ChordPro parser must never crash, hang or over-allocate on any input (SEC-03).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = selah_core::songs::fuzz_chordpro(data);
});
