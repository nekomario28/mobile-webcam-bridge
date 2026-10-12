//! Negative-path TLS tests: mismatched fingerprints and unauthorized access
//! must never complete AMB1 pairing or silently downgrade to plaintext.
mod common;

use common::{ACCESS_KEY, PHONE_SHA256, accept_phone};
use mobile_webcam::secure::SecureTcp;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

fn server(listener: TcpListener, allow: bool) {
    let mut tls = accept_phone(listener);
    let mut header = [0u8; 24];
    // With an incorrect server pin the client aborts during the TLS handshake.
    if tls.read_exact(&mut header).is_err() {
        return;
    }
    assert_eq!(&header[..4], b"AMB1");
    assert_eq!(header[5], 1);
    assert_eq!(&header[12..16], &16u32.to_le_bytes());
    let mut received = [0u8; 16];
    tls.read_exact(&mut received).unwrap();
    let expected = mobile_webcam::secure::parse_hex(ACCESS_KEY, 16).unwrap();
    if allow {
        assert_eq!(received.as_slice(), expected.as_slice());
        header[5] = 2;
        header[12..16].copy_from_slice(&0u32.to_le_bytes());
        tls.write_all(&header).unwrap();
        tls.flush().unwrap();
    } else {
        assert_ne!(received.as_slice(), expected.as_slice());
        // An unauthorized client receives no HELLO_ACK or VIDEO_AU.
    }
}

fn attempt(fingerprint: &str, access_key: &str, allow: bool) -> bool {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let thread = thread::spawn(move || server(listener, allow));
    let connected = SecureTcp::connect(
        "127.0.0.1",
        port,
        fingerprint,
        access_key,
        Duration::from_secs(4),
    )
    .is_ok();
    thread.join().expect("test TLS server thread");
    connected
}

#[test]
fn pinned_and_authorized_client_completes_pairing() {
    assert!(attempt(PHONE_SHA256, ACCESS_KEY, true));
}

#[test]
fn wrong_fingerprint_is_rejected_during_tls_handshake() {
    assert!(!attempt(&"0".repeat(64), ACCESS_KEY, false));
}

#[test]
fn wrong_access_key_is_rejected_before_any_video() {
    assert!(!attempt(
        PHONE_SHA256,
        "ffeeddccbbaa99887766554433221100",
        false
    ));
}
