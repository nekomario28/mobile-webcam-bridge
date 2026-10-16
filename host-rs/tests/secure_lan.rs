//! Negative-path TLS tests: mismatched fingerprints and unauthorized access
//! must never complete AMB1 pairing or silently downgrade to plaintext.
mod common;

use common::{ACCESS_KEY, PHONE_SHA256, phone_tls};
use mobile_webcam::{
    secure::SecureTcp,
    transport::Transport,
    wire::{self, Frame},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

fn server(listener: TcpListener, allow: bool) {
    let mut tls = phone_tls(listener.accept().expect("accept").0);
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
        // The real phone closes the connection on a rejected credential;
        // an unauthorized client receives no HELLO_ACK or VIDEO_AU.
    }
}

fn attempt(fingerprint: &str, access_key: &str, allow: bool) -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let thread = thread::spawn(move || server(listener, allow));
    let outcome = SecureTcp::connect(
        "127.0.0.1",
        port,
        fingerprint,
        access_key,
        Duration::from_secs(4),
    )
    .map(|_| ())
    .map_err(|error| error.to_string());
    thread.join().expect("test TLS server thread");
    outcome
}

#[test]
fn trickled_tls_handshake_cannot_extend_connect_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        // An incomplete handshake record never reaches certificate verification.
        // Each fragment arrives before the original per-read timeout, but the
        // whole record remains incomplete well past the connect budget.
        socket.write_all(&[22, 3, 3, 0, 16]).unwrap();
        for _ in 0..12 {
            thread::sleep(Duration::from_millis(60));
            if socket.write_all(&[0]).is_err() {
                break;
            }
        }
    });
    let started = Instant::now();
    let outcome = SecureTcp::connect(
        "127.0.0.1",
        port,
        PHONE_SHA256,
        ACCESS_KEY,
        Duration::from_millis(200),
    );
    let elapsed = started.elapsed();
    peer.join().unwrap();
    let error = match outcome {
        Ok(_) => panic!("an incomplete TLS handshake cannot pair"),
        Err(error) => error,
    };
    assert!(
        error.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )
        }),
        "expected a deadline error, got {error}"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "TLS trickling extended the 200ms connect budget to {elapsed:?}"
    );
}

#[test]
fn trickled_tls_record_cannot_extend_receive_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = thread::spawn(move || {
        let mut tls = phone_tls(listener.accept().expect("accept").0);
        let mut hello = [0; 40];
        tls.read_exact(&mut hello).unwrap();
        assert_eq!(
            &hello[24..],
            mobile_webcam::secure::parse_hex(ACCESS_KEY, 16).unwrap()
        );
        let ack = Frame::new(wire::HELLO_ACK, 1, Vec::new()).unwrap();
        tls.write_all(&ack.header.encode().unwrap()).unwrap();
        tls.flush().unwrap();
        // Generate a real encrypted record, but withhold its tail so no
        // plaintext becomes available. Individual socket reads still progress.
        let video = Frame::new(wire::VIDEO_AU, 2, vec![0; 32]).unwrap();
        tls.conn
            .writer()
            .write_all(&video.header.encode().unwrap())
            .unwrap();
        tls.conn.writer().write_all(&video.payload).unwrap();
        let mut record = Vec::new();
        while tls.conn.wants_write() {
            tls.conn.write_tls(&mut record).unwrap();
        }
        assert!(record.len() > 17);
        tls.sock.write_all(&record[..5]).unwrap();
        for byte in &record[5..17] {
            thread::sleep(Duration::from_millis(60));
            if tls.sock.write_all(&[*byte]).is_err() {
                break;
            }
        }
    });
    let mut client = SecureTcp::connect(
        "127.0.0.1",
        port,
        PHONE_SHA256,
        ACCESS_KEY,
        Duration::from_secs(4),
    )
    .unwrap();
    let started = Instant::now();
    let outcome = client.receive(Duration::from_millis(200));
    let elapsed = started.elapsed();
    // Close before joining so a timed-out peer cannot keep this test alive.
    drop(client);
    peer.join().unwrap();
    let error = outcome.expect_err("an incomplete TLS record cannot yield a frame");
    assert!(
        error.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )
        }),
        "expected a deadline error, got {error}"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "TLS record trickling extended the receive budget to {elapsed:?}"
    );
}

#[test]
fn pinned_and_authorized_client_completes_pairing() {
    attempt(PHONE_SHA256, ACCESS_KEY, true).expect("authorized client must pair");
}

#[test]
fn wrong_fingerprint_is_rejected_during_tls_handshake() {
    let error =
        attempt(&"0".repeat(64), ACCESS_KEY, false).expect_err("wrong fingerprint must not pair");
    assert!(error.contains("fingerprint mismatch"), "{error}");
}

#[test]
fn wrong_access_key_is_rejected_before_any_video() {
    let error = attempt(PHONE_SHA256, "ffeeddccbbaa99887766554433221100", false)
        .expect_err("wrong access key must not pair");
    assert!(error.contains("not authorized"), "{error}");
}
