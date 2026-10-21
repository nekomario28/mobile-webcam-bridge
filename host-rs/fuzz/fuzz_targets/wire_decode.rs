//! Fuzz every decoder reachable from the network.
//!
//! Built for depth rather than for CI: `cargo +nightly fuzz run wire_decode -- -max_total_time=120`.
//! `tests/parsers.rs` runs the same shapes on stable in CI, so this is the deeper net,
//! not the only one.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let bytes = data;
    let _ = mobile_webcam::wire::Capabilities::decode(bytes);
    let _ = mobile_webcam::wire::PrepareQuality::decode(bytes);
    let _ = mobile_webcam::wire::QualityResult::decode(bytes);
    let _ = mobile_webcam::wire::ClientChallenge::decode(bytes);
    let _ = mobile_webcam::wire::ClientAuth::decode(bytes);
    let _ = mobile_webcam::wire::ClientAuthResult::decode(bytes);
    let _ = mobile_webcam::wire::Epoch::decode(bytes);
    let _ = mobile_webcam::wire::Header::decode(bytes);
    let _ = mobile_webcam::wire::PairingInfo::decode(bytes);
    let _ = mobile_webcam::video::Config::parse(bytes);
});
