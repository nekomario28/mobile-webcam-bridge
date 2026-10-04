#![cfg(all(feature = "gui", feature = "video", any(target_os = "linux", windows)))]
use mobile_webcam::{
    session::{Session, State},
    settings::Settings,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn peer(listener: TcpListener) {
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let mut header = [0; 24];
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[5], 1);
    assert_eq!(&header[8..12], &1u32.to_le_bytes());
    header[5] = 2;
    socket.write_all(&header).unwrap();
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[5], 0x12);
    let mut byte = [0];
    assert_eq!(socket.read(&mut byte).unwrap(), 0);
}
#[test]
fn real_worker_disconnect_and_parent_death() {
    let executable = std::env::var_os("MW_TEST_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_mobile-webcam").into());
    let ctx = eframe::egui::Context::default();
    let mut session = Session::new(executable.clone());
    let settings = Settings {
        transport: "lan".into(),
        lan_host: "127.0.0.1".into(),
        decode: "off".into(),
        ..Default::default()
    };
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
    session
        .start(
            Settings {
                transport: "lan".into(),
                lan_host: "127.0.0.1".into(),
                decode: "off".into(),
                ..Default::default()
            },
            None,
            ctx.clone(),
        )
        .unwrap();
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
