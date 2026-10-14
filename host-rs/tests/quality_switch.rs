#![cfg(all(feature = "video", any(target_os = "linux", windows)))]
//! A synthetic phone peer drives the PC-side quality switch over TCP: the
//! capability advertisement, the prepare, the paused boundary, the commit and
//! the applied result. No real phone, no virtual camera.
use mobile_webcam::{
    control::{self, Command, Event},
    settings::Settings,
    wire::{self, Header},
};
use std::{
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command as ProcessCommand, Stdio},
    thread,
    time::{Duration, Instant},
};

fn frame_bytes(kind: u8, payload: &[u8]) -> Vec<u8> {
    let header = Header {
        kind,
        flags: 0,
        sequence: 0,
        payload_len: payload.len() as u32,
        pts_us: 0,
    };
    let mut bytes = header.encode().unwrap().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut header = [0; wire::HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let decoded = Header::decode(&header).unwrap();
    let mut payload = vec![0; decoded.payload_len as usize];
    if !payload.is_empty() {
        stream.read_exact(&mut payload).unwrap();
    }
    (decoded.kind, payload)
}

fn video_config_bytes(width: u16, height: u16, fps: u16) -> Vec<u8> {
    let mut payload = vec![0; 16];
    payload[0..2].copy_from_slice(&width.to_le_bytes());
    payload[2..4].copy_from_slice(&height.to_le_bytes());
    payload[4..6].copy_from_slice(&fps.to_le_bytes());
    payload[12] = 1;
    payload[13] = 1;
    payload
}

/// The synthetic peer: advertise, answer the prepare, pause, receive the
/// commit, then serve the new-epoch config.
fn peer(listener: TcpListener) {
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // The plaintext transport pairs first: answer the HELLO, then the
    // worker opens with a best-effort IDR request before any receive.
    let (kind, _) = read_frame(&mut stream);
    assert_eq!(kind, wire::HELLO);
    let ack = Header {
        kind: wire::HELLO_ACK,
        flags: 0,
        sequence: 1,
        payload_len: 0,
        pts_us: 0,
    };
    stream.write_all(&ack.encode().unwrap()).unwrap();
    let (kind, _) = read_frame(&mut stream);
    assert_eq!(kind, wire::IDR_REQUEST);
    let modes = vec![
        wire::QualityMode {
            width: 1280,
            height: 720,
            target_fps: 30,
            bitrate: 6_000_000,
        },
        wire::QualityMode {
            width: 1920,
            height: 1080,
            target_fps: 30,
            bitrate: 13_500_000,
        },
    ];
    let caps = wire::Capabilities {
        extension_version: 1,
        permission: wire::PERMISSION_READY,
        catalog_revision: 1,
        stream_epoch: 0,
        modes: modes.clone(),
    };
    stream
        .write_all(&frame_bytes(
            wire::QUALITY_CAPABILITIES,
            &caps.encode().unwrap(),
        ))
        .unwrap();

    // The PC requests 1080p.
    let (kind, payload) = read_frame(&mut stream);
    assert_eq!(kind, wire::PREPARE_QUALITY);
    let prepare = wire::PrepareQuality::decode(&payload).unwrap();
    assert_eq!(
        (prepare.width, prepare.height, prepare.target_fps),
        (1920, 1080, 30)
    );

    // The boundary: pause, then receive the commit with the advanced epoch.
    let paused = wire::Epoch {
        request_id: prepare.request_id,
        stream_epoch: 0,
    };
    stream
        .write_all(&frame_bytes(wire::PAUSED, &paused.encode()))
        .unwrap();
    let (kind, payload) = read_frame(&mut stream);
    assert_eq!(kind, wire::COMMIT_QUALITY);
    let commit = wire::Epoch::decode(&payload).unwrap();
    assert_eq!(commit.stream_epoch, 1);

    // The new epoch's config; the applied result must follow.
    stream
        .write_all(&frame_bytes(
            wire::VIDEO_CONFIG,
            &video_config_bytes(1920, 1080, 30),
        ))
        .unwrap();
    let mut byte = [0];
    let _ = stream.read(&mut byte);
}

#[test]
fn pc_worker_switches_mode_through_the_full_boundary() {
    // The worker always dials port 48527 (the Wi-Fi default).
    let listener = TcpListener::bind("127.0.0.1:48527").unwrap();
    let server = thread::spawn(move || peer(listener));

    let executable = std::env::var_os("MW_TEST_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_mobile-webcam").into());
    let mut child = ProcessCommand::new(executable)
        .args(["--worker", &std::process::id().to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let settings = Settings {
        transport: "lan".into(),
        lan_host: "127.0.0.1".into(),
        decode: "off".into(),
        ..Default::default()
    };
    control::write(
        &mut stdin,
        &Command::Start {
            settings,
            device: None,
        },
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_capabilities = false;
    let mut saw_applied = false;
    while Instant::now() < deadline {
        let Some(event) = control::read::<Event>(&mut reader).unwrap() else {
            break;
        };
        match event {
            Event::QualityCapabilities {
                permission, modes, ..
            } => {
                saw_capabilities = true;
                assert_eq!(permission, wire::PERMISSION_READY);
                assert_eq!(modes.len(), 2);
                // Now request the 1080p mode.
                control::write(
                    &mut stdin,
                    &Command::PrepareQuality {
                        width: 1920,
                        height: 1080,
                        fps: 30,
                    },
                )
                .unwrap();
            }
            Event::QualityApplied { width, height, fps } => {
                assert_eq!((width, height, fps), (1920, 1080, 30));
                saw_applied = true;
                break;
            }
            _ => {}
        }
    }
    let _ = control::write(&mut stdin, &Command::Stop {});
    let _ = child.wait();
    server.join().unwrap();
    assert!(
        saw_capabilities,
        "the capability advertisement never arrived"
    );
    assert!(saw_applied, "the applied result never arrived");
}

/// A worker paired with a synthetic peer; the peer thread is joined by the
/// test body. Returns the worker's stdin/stdout handles.
fn spawn_paired_worker() -> (
    std::process::Child,
    std::process::ChildStdin,
    std::process::ChildStdout,
) {
    let executable = std::env::var_os("MW_TEST_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_mobile-webcam").into());
    let mut child = ProcessCommand::new(executable)
        .args(["--worker", &std::process::id().to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let settings = Settings {
        transport: "lan".into(),
        lan_host: "127.0.0.1".into(),
        decode: "off".into(),
        ..Default::default()
    };
    control::write(
        &mut stdin,
        &Command::Start {
            settings,
            device: None,
        },
    )
    .unwrap();
    // The peer side is the listener; the caller drives it.
    (child, stdin, stdout)
}

#[test]
fn rejection_keeps_the_stream_and_only_reports() {
    let listener = TcpListener::bind("127.0.0.1:48527").unwrap();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (kind, _) = read_frame(&mut stream);
        assert_eq!(kind, wire::HELLO);
        stream
            .write_all(&frame_bytes(wire::HELLO_ACK, &[]))
            .unwrap();
        let (kind, _) = read_frame(&mut stream);
        assert_eq!(kind, wire::IDR_REQUEST);
        let caps = wire::Capabilities {
            extension_version: 1,
            permission: wire::PERMISSION_PENDING,
            catalog_revision: 1,
            stream_epoch: 0,
            modes: vec![wire::QualityMode {
                width: 1280,
                height: 720,
                target_fps: 30,
                bitrate: 6_000_000,
            }],
        };
        stream
            .write_all(&frame_bytes(
                wire::QUALITY_CAPABILITIES,
                &caps.encode().unwrap(),
            ))
            .unwrap();
        let (_, payload) = read_frame(&mut stream);
        let prepare = wire::PrepareQuality::decode(&payload).unwrap();
        // The rejection: permission is still pending, so the streaming mode
        // is kept and only an answer goes out.
        let rejected = wire::QualityResult {
            request_id: prepare.request_id,
            status: wire::RESULT_REJECTED,
            reason: wire::REASON_PERMISSION,
            applied_width: 0,
            applied_height: 0,
            applied_fps: 0,
        };
        stream
            .write_all(&frame_bytes(wire::QUALITY_RESULT, &rejected.encode()))
            .unwrap();
        // The connection survives: a fresh advertisement still arrives.
        let mut byte = [0];
        let _ = stream.read(&mut byte);
    });

    let (mut child, mut stdin, stdout) = spawn_paired_worker();
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_failed = false;
    while Instant::now() < deadline {
        let Some(event) = control::read::<Event>(&mut reader).unwrap() else {
            break;
        };
        if let Event::QualityFailed { reason } = event {
            assert!(reason.contains("permission"), "{reason}");
            saw_failed = true;
            break;
        }
        if let Event::QualityCapabilities { .. } = event {
            control::write(
                &mut stdin,
                &Command::PrepareQuality {
                    width: 1920,
                    height: 1080,
                    fps: 30,
                },
            )
            .unwrap();
        }
    }
    let _ = control::write(&mut stdin, &Command::Stop {});
    let _ = child.wait();
    peer.join().unwrap();
    assert!(saw_failed, "the rejection never reported");
}

#[test]
fn no_quality_command_before_the_advertisement() {
    let listener = TcpListener::bind("127.0.0.1:48527").unwrap();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let (kind, _) = read_frame(&mut stream);
        assert_eq!(kind, wire::HELLO);
        stream
            .write_all(&frame_bytes(wire::HELLO_ACK, &[]))
            .unwrap();
        let (kind, _) = read_frame(&mut stream);
        assert_eq!(kind, wire::IDR_REQUEST);
        // The phone never advertises; a prepare must not reach it.
        let mut byte = [0];
        let read = stream.read(&mut byte);
        assert!(
            matches!(read, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut),
            "the worker sent a command before the advertisement: {read:?}"
        );
    });

    let (mut child, mut stdin, stdout) = spawn_paired_worker();
    let mut reader = BufReader::new(stdout);
    control::write(
        &mut stdin,
        &Command::PrepareQuality {
            width: 1920,
            height: 1080,
            fps: 30,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut saw_failed = false;
    while Instant::now() < deadline {
        let Some(event) = control::read::<Event>(&mut reader).unwrap() else {
            break;
        };
        if let Event::QualityFailed { reason } = event {
            assert!(reason.contains("not advertised"), "{reason}");
            saw_failed = true;
            break;
        }
    }
    let _ = control::write(&mut stdin, &Command::Stop {});
    let _ = child.wait();
    peer.join().unwrap();
    assert!(
        saw_failed,
        "the missing advertisement never failed the apply"
    );
}

#[test]
fn the_apply_deadline_fails_when_the_phone_stalls() {
    let listener = TcpListener::bind("127.0.0.1:48527").unwrap();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (kind, _) = read_frame(&mut stream);
        assert_eq!(kind, wire::HELLO);
        stream
            .write_all(&frame_bytes(wire::HELLO_ACK, &[]))
            .unwrap();
        let (kind, _) = read_frame(&mut stream);
        assert_eq!(kind, wire::IDR_REQUEST);
        let caps = wire::Capabilities {
            extension_version: 1,
            permission: wire::PERMISSION_READY,
            catalog_revision: 1,
            stream_epoch: 0,
            modes: vec![wire::QualityMode {
                width: 1280,
                height: 720,
                target_fps: 30,
                bitrate: 6_000_000,
            }],
        };
        stream
            .write_all(&frame_bytes(
                wire::QUALITY_CAPABILITIES,
                &caps.encode().unwrap(),
            ))
            .unwrap();
        let (_, _) = read_frame(&mut stream);
        // The phone stalls: no QualityResult, no PAUSED. The worker's
        // 5-second deadline must fire and close the session.
        let mut byte = [0];
        let _ = stream.read(&mut byte);
    });

    let (mut child, mut stdin, stdout) = spawn_paired_worker();
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_failed = false;
    let mut saw_stopped = false;
    while Instant::now() < deadline {
        let Some(event) = control::read::<Event>(&mut reader).unwrap() else {
            break;
        };
        match event {
            Event::QualityCapabilities { .. } => {
                control::write(
                    &mut stdin,
                    &Command::PrepareQuality {
                        width: 1280,
                        height: 720,
                        fps: 30,
                    },
                )
                .unwrap();
            }
            Event::QualityFailed { reason } => {
                assert!(reason.contains("deadline"), "{reason}");
                saw_failed = true;
            }
            Event::Stopped {} => {
                saw_stopped = true;
                break;
            }
            _ => {}
        }
    }
    let _ = child.wait();
    peer.join().unwrap();
    assert!(saw_failed, "the deadline never failed the apply");
    assert!(saw_stopped, "the worker never stopped after the deadline");
}
