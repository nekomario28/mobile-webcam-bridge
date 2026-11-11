//! Entry points for the cargo-fuzz targets in `fuzz/`; enabled only by the
//! `fuzzing` feature, which nothing but the fuzz crate turns on. Not an API.
//!
//! Each function takes raw fuzzer bytes and drives one parser that sees input
//! from outside the process: the AMB1 stream reassembly that USB and TCP share,
//! the JSON control lines between the GUI, the worker and the privileged USB
//! helper, and the Linux settings file.
use crate::wire;
use std::io::{BufReader, Cursor};
use std::time::Duration;

/// Feed `data` through the shared AMB1 frame reader in fuzzer-chosen chunk
/// sizes, then decode every completed frame with the decoder its type selects.
/// The first byte picks the chunking; the rest is the byte stream.
pub fn frame_stream(data: &[u8]) {
    let Some((&shape, mut stream)) = data.split_first() else {
        return;
    };
    let mut reader = crate::transport::FrameReader::default();
    let mut step = 0usize;
    for _ in 0..4096 {
        let polled = reader.poll(Duration::from_secs(1), |target, _| {
            if stream.is_empty() {
                return Ok(0);
            }
            // 1..=32 bytes per read, varying, so header and payload boundaries
            // fall at every offset across inputs.
            step = step.wrapping_add(1);
            let chunk = 1 + ((shape as usize).wrapping_mul(step) % 32);
            let count = chunk.min(target.len()).min(stream.len());
            target[..count].copy_from_slice(&stream[..count]);
            stream = &stream[count..];
            Ok(count)
        });
        match polled {
            Ok(Some(frame)) => decode_by_kind(&frame),
            Ok(None) => continue,
            Err(_) => return,
        }
    }
}

fn decode_by_kind(frame: &wire::Frame) {
    let payload = frame.payload.as_slice();
    match frame.header.kind {
        wire::QUALITY_CAPABILITIES => drop(wire::Capabilities::decode(payload)),
        wire::PREPARE_QUALITY => drop(wire::PrepareQuality::decode(payload)),
        wire::QUALITY_RESULT => drop(wire::QualityResult::decode(payload)),
        wire::PAUSED | wire::COMMIT_QUALITY => drop(wire::Epoch::decode(payload)),
        wire::CLIENT_CHALLENGE => drop(wire::ClientChallenge::decode(payload)),
        wire::CLIENT_AUTH => drop(wire::ClientAuth::decode(payload)),
        wire::CLIENT_AUTH_RESULT => drop(wire::ClientAuthResult::decode(payload)),
        wire::PAIRING_INFO => drop(wire::PairingInfo::decode(payload)),
        wire::VIDEO_CONFIG => drop(crate::video_config::Config::parse(payload)),
        _ => {}
    }
    let _ = frame.validate();
}

/// Read JSON control lines the way each receiving process does: the worker's
/// commands, the GUI's events, and the privileged USB helper's device id.
pub fn control_lines(data: &[u8]) {
    let mut commands = BufReader::new(Cursor::new(data));
    while let Ok(Some(_)) = crate::control::read::<crate::control::Command>(&mut commands) {}
    let mut events = BufReader::new(Cursor::new(data));
    while let Ok(Some(_)) = crate::control::read::<crate::control::Event>(&mut events) {}
    // Small buffers split lines across `fill_buf` calls.
    let mut device = BufReader::with_capacity(7, Cursor::new(data));
    let _ = crate::control::read::<crate::usb::DeviceId>(&mut device);
}

/// Parse a Linux settings file (INI with Qt escapes) and validate the result.
pub fn settings_file(data: &[u8]) {
    if let Ok(text) = std::str::from_utf8(data) {
        crate::settings::fuzz_parse(text);
    }
    let _ = crate::headless::parse_quality(&String::from_utf8_lossy(data));
    let _ = crate::secure::parse_hex(&String::from_utf8_lossy(data), 16);
}
