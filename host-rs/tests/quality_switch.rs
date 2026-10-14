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
    io::{BufRead, BufReader, Read, Write},
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
    // The worker opens with a best-effort IDR request before any receive.
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
        .write_all(&frame_bytes(wire::PAUSED, &paused.encode().unwrap()))
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
        lan_fingerprint: String::new(),
        lan_key: String::new(),
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
