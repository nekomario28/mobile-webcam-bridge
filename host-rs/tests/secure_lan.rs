//! Negative-path TLS tests: mismatched fingerprints and unauthorized access
//! must never complete AMB1 pairing or silently downgrade to plaintext.
use mobile_webcam::secure::SecureTcp;
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::Duration,
};

const PHONE_SHA256: &str = "5331e886b4b4a360fb7ae16a0afc15791c595354a3c9bf329bf8b4a3eef25a25";
const ACCESS_KEY: &str = "00112233445566778899aabbccddeeff";

fn server(listener: TcpListener, allow: bool) {
    let (socket, _) = listener.accept().expect("accept");
    socket.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    socket.set_write_timeout(Some(Duration::from_secs(4))).unwrap();
    let cert = CertificateDer::from(include_bytes!("fixtures/test-phone-cert.der").to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        include_bytes!("fixtures/test-phone-key.der").to_vec(),
    ));
    let config = ServerConfig::builder_with_provider(
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    let mut tls = StreamOwned::new(
        ServerConnection::new(Arc::new(config)).unwrap(),
        socket,
    );
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
