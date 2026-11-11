//! See `mobile_webcam::fuzzing::settings_file` for what this target drives.
//!
//! `cargo +nightly fuzz run settings_file -- -max_total_time=120`

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    mobile_webcam::fuzzing::settings_file(data);
});
