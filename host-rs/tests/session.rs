#![cfg(all(feature = "gui", feature = "video", any(target_os = "linux", windows)))]
use mobile_webcam::{
    session::{Session, State},
    settings::Settings,
};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use std::sync::Arc;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PHONE_SHA256: &str = "5331e886b4b4a360fb7ae16a0afc15791c595354a3c9bf329bf8b4a3eef25a25";
const PAIRING_KEY: &str = "00112233445566778899aabbccddeeff";

fn paired_settings() -> Settings {
    Settings {
        transport: "lan".into(),
        lan_host: "127.0.0.1".into(),
        lan_fingerprint: PHONE_SHA256.into(),
        lan_key: PAIRING_KEY.into(),
        decode: "off".into(),
        ..Default::default()
    }
}

fn peer(listener: TcpListener) {
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let cert = CertificateDer::from(include_bytes!("fixtures/test-phone-cert.der").to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        include_bytes!("fixtures/test-phone-key.der").to_vec(),
    ));
    // Test-only fixture identity. Never ship private fixtures as real device identities.
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
    let connection = ServerConnection::new(Arc::new(config)).unwrap();
    let mut socket = StreamOwned::new(connection, socket);
    let mut header = [0; 24];
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[5], 1);
    assert_eq!(&header[8..12], &1u32.to_le_bytes());
    assert_eq!(&header[12..16], &16u32.to_le_bytes());
    let mut token = [0u8; 16];
    socket.read_exact(&mut token).unwrap();
    assert_eq!(
        token.as_slice(),
        &mobile_webcam::secure::parse_hex(PAIRING_KEY, 16).unwrap()
    );
    header[5] = 2;
    header[12..16].copy_from_slice(&0u32.to_le_bytes());
    socket.write_all(&header).unwrap();
    socket.flush().unwrap();
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[5], 0x12);
    let mut byte = [0];
    // Forced process termination can close TCP abortively on Windows.
    // Both terminal outcomes prove the peer closed; data/timeouts still fail.
    match socket.read(&mut byte) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionAborted
            ) => {}
        other => panic!("worker connection remained open or sent unexpected data: {other:?}"),
    }
}
#[test]
fn real_worker_disconnect_and_parent_death() {
    let executable = std::env::var_os("MW_TEST_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_mobile-webcam").into());
    let ctx = eframe::egui::Context::default();
    let mut session = Session::new(executable.clone());
    let settings = paired_settings();
    for _ in 0..2 {
        let listener = TcpListener::bind("127.0.0.1:48527").unwrap();
        let server = thread::spawn(move || peer(listener));
        session.start(settings.clone(), None, ctx.clone()).unwrap();
        let started = Instant::now();
        while !matches!(session.state, State::Waiting) {
            session.poll(&ctx);
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                session.state
            );
            thread::sleep(Duration::from_millis(10));
        }
        let stopped = Instant::now();
        session.stop(&ctx);
        while session.running() {
            session.poll(&ctx);
            assert!(stopped.elapsed() < Duration::from_millis(1500));
            thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(session.state, State::Idle));
        server.join().unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:48527").unwrap();
    let server = thread::spawn(move || peer(listener));
    let mut parent = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "abrupt_parent_fixture", "--nocapture"])
        .env("MW_TEST_PARENT_EXECUTABLE", &executable)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(parent.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("PARENT_READY") {
        line.clear();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
    }
    assert!(parent.wait().unwrap().success());
    server.join().unwrap();
}

#[test]
fn abrupt_parent_fixture() {
    let Some(executable) = std::env::var_os("MW_TEST_PARENT_EXECUTABLE") else {
        return;
    };
    let ctx = eframe::egui::Context::default();
    let mut session = Session::new(executable.into());
    session.start(paired_settings(), None, ctx.clone()).unwrap();
    let started = Instant::now();
    while !matches!(session.state, State::Waiting) {
        session.poll(&ctx);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            session.state
        );
        thread::sleep(Duration::from_millis(10));
    }
    println!("PARENT_READY");
    std::io::stdout().flush().unwrap();
    // Exit without running Session::drop: OS parent/job ownership must stop it.
    std::process::exit(0);
}
