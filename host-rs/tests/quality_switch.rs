#![cfg(all(feature = "video", any(target_os = "linux", windows)))]
//! Real subprocess + TLS control/framing. Peers do not prove Android capture
//! or native output acceptance; the pipeline's output seam is tested separately.
mod common;
use common::{ACCESS_KEY, PHONE_SHA256, phone_tls};
use mobile_webcam::{
    control::{self, Command, Event},
    settings::Settings,
    wire::{self, Header},
};
use rustls::{ServerConnection, StreamOwned};
use std::{
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, Command as ProcessCommand, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
fn bytes(kind: u8, payload: &[u8], sequence: u32) -> Vec<u8> {
    let header = Header {
        kind,
        flags: 0,
        sequence,
        payload_len: payload.len() as u32,
        pts_us: 0,
    };
    [header.encode().unwrap().as_slice(), payload].concat()
}
fn read(stream: &mut impl Read) -> (u8, Vec<u8>) {
    let mut header = [0; wire::HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let header = Header::decode(&header).unwrap();
    let mut payload = vec![0; header.payload_len as usize];
    stream.read_exact(&mut payload).unwrap();
    (header.kind, payload)
}
fn config(width: u16, height: u16, fps: u16) -> Vec<u8> {
    let mut payload = vec![0; 16];
    payload[..2].copy_from_slice(&width.to_le_bytes());
    payload[2..4].copy_from_slice(&height.to_le_bytes());
    payload[4..6].copy_from_slice(&fps.to_le_bytes());
    payload[12] = 1;
    payload[13] = 1;
    payload
}
#[derive(Clone, Copy)]
enum Case {
    ConfigOnly,
    Stall,
    Fragmented,
    NoAdvertisement,
    PermissionPending,
    UnsupportedMode,
    WrongFps,
    StopPartial,
    TrickledResult,
    TrickledPastBudget,
}
struct Peer(Option<thread::JoinHandle<()>>);
impl Peer {
    fn finish(mut self) {
        self.0.take().unwrap().join().unwrap();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(thread) = self.0.take() {
            let _ = thread.join();
        }
    }
}
/// Assert the worker sent no further command. Any AMB1 byte after a denial or
/// Stop is a failure; EOF, a TLS close_notify absence or an elapsed budget are
/// all acceptable silence.
fn expect_no_more_frames(stream: &mut StreamOwned<ServerConnection, TcpStream>) {
    stream
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    // The worker's own apply deadline is 5s; a staged or output-pending case
    // legitimately keeps the connection open that long before it closes.
    let deadline = Instant::now() + Duration::from_secs(9);
    let mut byte = [0];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return,
            Ok(_) => panic!("unexpected host command after denial/stop"),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                assert!(Instant::now() < deadline, "no close and no further command");
            }
            Err(error) => {
                let text = error.to_string();
                let gone = matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                        | std::io::ErrorKind::ConnectionAborted
                );
                assert!(
                    gone || text.contains("close_notify")
                        || text.contains("UnexpectedEof")
                        || text.contains("peer disconnected"),
                    "unexpected read error: {text}"
                );
                return;
            }
        }
    }
}

fn peer(case: Case) -> (String, Peer) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    listener.set_nonblocking(true).unwrap();
    let task = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "worker never connected");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        // Accepted sockets inherit the listener's nonblocking mode on Windows.
        stream.set_nonblocking(false).unwrap();
        let mut stream = phone_tls(stream);
        assert_eq!(read(&mut stream).0, wire::HELLO);
        stream.write_all(&bytes(wire::HELLO_ACK, &[], 1)).unwrap();
        common::authorize_tls(&mut stream, true);
        assert_eq!(read(&mut stream).0, wire::IDR_REQUEST);
        // The stream is configured before advertising: Apply cannot hide
        // behind the previous 120-second established-stream receive.
        stream
            .write_all(&bytes(wire::VIDEO_CONFIG, &config(64, 32, 15), 0))
            .unwrap();
        if !matches!(case, Case::NoAdvertisement) {
            let caps = wire::Capabilities {
                extension_version: 1,
                permission: if matches!(case, Case::PermissionPending) {
                    wire::PERMISSION_PENDING
                } else {
                    wire::PERMISSION_READY
                },
                catalog_revision: 7,
                stream_epoch: 0,
                modes: vec![wire::QualityMode {
                    width: 1920,
                    height: 1080,
                    target_fps: 30,
                    bitrate: 13_500_000,
                }],
            };
            let advertisement = bytes(wire::QUALITY_CAPABILITIES, &caps.encode().unwrap(), 0);
            if matches!(case, Case::Fragmented) {
                for chunk in advertisement.chunks(8) {
                    stream.write_all(chunk).unwrap();
                    thread::sleep(Duration::from_millis(150));
                }
            } else {
                stream.write_all(&advertisement).unwrap();
            }
        }
        if matches!(case, Case::StopPartial) {
            stream
                .write_all(&bytes(wire::VIDEO_AU, &[0; 128], 0)[..11])
                .unwrap();
        }
        if matches!(case, Case::TrickledPastBudget) {
            // A frame that starts before any apply exists, so only the partial
            // frame's own budget can expire - not the apply deadline.
            // 64 payload bytes, one byte every 400ms: the frame cannot finish
            // before its own 5s budget expires.
            // A peer-side timeout firing inside rustls' complete_io aborts a
            // record mid-write, which the worker reports as a decryption
            // failure instead of the frame deadline under test. This peer is
            // bounded by its own loop, not by socket timeouts.
            let socket = stream.get_mut();
            socket
                .set_read_timeout(Some(Duration::from_secs(30)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(30)))
                .unwrap();
            let partial = bytes(wire::VIDEO_AU, &[7; 64], 0);
            stream.write_all(&partial[..wire::HEADER_SIZE + 1]).unwrap();
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(9) {
                thread::sleep(Duration::from_millis(400));
                let sent = 1 + started.elapsed().as_millis() as usize / 400;
                // The worker closes once the frame deadline expires; writing
                // into that closed socket is the expected end of this peer.
                if stream
                    .write_all(&partial[wire::HEADER_SIZE + sent..][..1])
                    .is_err()
                {
                    break;
                }
            }
        }
        if matches!(case, Case::TrickledResult) {
            let (kind, payload) = read(&mut stream);
            assert_eq!(kind, wire::PREPARE_QUALITY);
            let prepare = wire::PrepareQuality::decode(&payload).unwrap();
            let result = wire::QualityResult {
                request_id: prepare.request_id,
                status: wire::RESULT_REJECTED,
                reason: wire::REASON_BUSY,
                applied_width: 0,
                applied_height: 0,
                applied_fps: 0,
            };
            let reply = bytes(wire::QUALITY_RESULT, &result.encode(), 0);
            if matches!(case, Case::TrickledResult) {
                // Every chunk lands inside its own read timeout, so only the
                // retained partial frame can keep the AMB1 boundary. rustls
                // buffers the incomplete record internally; the PC must not.
                for chunk in reply.chunks(3) {
                    stream.write_all(chunk).unwrap();
                    thread::sleep(Duration::from_millis(180));
                }
            }
        }
        if matches!(case, Case::ConfigOnly | Case::Stall | Case::WrongFps) {
            let (kind, payload) = read(&mut stream);
            assert_eq!(kind, wire::PREPARE_QUALITY);
            let prepare = wire::PrepareQuality::decode(&payload).unwrap();
            assert_eq!(
                (
                    prepare.catalog_revision,
                    prepare.width,
                    prepare.height,
                    prepare.target_fps
                ),
                (7, 1920, 1080, 30)
            );
            if !matches!(case, Case::Stall) {
                let epoch = wire::Epoch {
                    request_id: prepare.request_id,
                    stream_epoch: 0,
                };
                stream
                    .write_all(&bytes(wire::PAUSED, &epoch.encode(), 0))
                    .unwrap();
                let (kind, payload) = read(&mut stream);
                assert_eq!(kind, wire::COMMIT_QUALITY);
                assert_eq!(wire::Epoch::decode(&payload).unwrap().stream_epoch, 1);
                stream
                    .write_all(&bytes(
                        wire::VIDEO_CONFIG,
                        &config(
                            1920,
                            1080,
                            if matches!(case, Case::WrongFps) {
                                25
                            } else {
                                30
                            },
                        ),
                        0,
                    ))
                    .unwrap();
            }
        }
        // Every case must close; denied requests must not leak even one byte.
        // A closed TLS peer surfaces as EOF or a missing close_notify, never as
        // a clean zero-length read, so both are accepted as "nothing more".
        expect_no_more_frames(&mut stream);
    });
    (address, Peer(Some(task)))
}
struct Worker {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<Event>,
}
impl Worker {
    fn new(address: String) -> Self {
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
        let (sender, events) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(event) = control::read::<Event>(&mut reader).unwrap() {
                if sender.send(event).is_err() {
                    break;
                }
            }
        });
        control::write(
            &mut stdin,
            &Command::Start {
                settings: Box::new(Settings {
                    transport: "lan".into(),
                    lan_host: address,
                    lan_fingerprint: PHONE_SHA256.into(),
                    lan_key: ACCESS_KEY.into(),
                    decode: "off".into(),
                    ..Default::default()
                }),
                device: None,
            },
        )
        .unwrap();
        Self {
            child,
            stdin,
            events,
        }
    }
    fn command(&mut self, command: Command) {
        control::write(&mut self.stdin, &command).unwrap();
    }
    fn until(&self, predicate: impl Fn(&Event) -> bool) -> Vec<Event> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut observed = Vec::new();
        loop {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| panic!("worker event deadline ({error}): {observed:?}"));
            let done = predicate(&event);
            observed.push(event);
            if done {
                return observed;
            }
        }
    }
    fn advertised(&self) {
        self.until(|event| matches!(event, Event::QualityCapabilities { .. }));
    }
    fn apply(&mut self, width: u16) {
        self.command(Command::PrepareQuality {
            width,
            height: if width == 1920 { 1080 } else { 720 },
            fps: 30,
        });
    }
    fn stop(&mut self) {
        self.command(Command::Stop {});
        self.until(|event| matches!(event, Event::Stopped {}));
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn fails_without_applied(case: Case, expected_reason: &str) {
    let (address, peer) = peer(case);
    let mut worker = Worker::new(address);
    worker.advertised();
    worker.apply(1920);
    let events = worker.until(|event| matches!(event, Event::Stopped {}));
    assert!(events.iter().any(|event| matches!(event, Event::QualityFailed { reason, .. } if reason.contains(expected_reason))), "{events:?}");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::QualityApplied { .. })),
        "metadata claimed Applied: {events:?}"
    );
    drop(worker);
    peer.finish();
}
#[test]
fn pause_commit_and_config_without_output_hit_deadline_not_applied() {
    fails_without_applied(Case::ConfigOnly, "deadline");
}
#[test]
fn established_stream_stall_does_not_hide_apply_or_its_deadline() {
    fails_without_applied(Case::Stall, "deadline");
}
#[test]
fn matching_dimensions_but_wrong_fps_fail_before_applied() {
    fails_without_applied(Case::WrongFps, "did not match");
}
#[test]
fn fragmented_advertisement_survives_multiple_receive_polls() {
    let (address, peer) = peer(Case::Fragmented);
    let mut worker = Worker::new(address);
    worker.advertised();
    worker.stop();
    drop(worker);
    peer.finish();
}
#[test]
fn stop_remains_responsive_during_partial_frame() {
    let (address, peer) = peer(Case::StopPartial);
    let mut worker = Worker::new(address);
    worker.advertised();
    thread::sleep(Duration::from_millis(250));
    let start = Instant::now();
    worker.stop();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "partial receive hid Stop"
    );
    drop(worker);
    peer.finish();
}
#[test]
fn trickled_control_frame_over_tls_keeps_the_amb1_boundary() {
    let (address, peer) = peer(Case::TrickledResult);
    let mut worker = Worker::new(address);
    worker.advertised();
    worker.apply(1920);
    // A frame split across many records and polls must arrive whole: the
    // rejection reason proves the payload was decoded, not discarded.
    worker.until(
        |event| matches!(event, Event::QualityFailed { reason, .. } if reason.contains("in flight")),
    );
    worker.stop();
    drop(worker);
    peer.finish();
}

#[test]
fn trickled_frame_cannot_extend_the_partial_frame_budget_over_tls() {
    let (address, peer) = peer(Case::TrickledPastBudget);
    let worker = Worker::new(address);
    worker.advertised();
    let events = worker.until(|event| matches!(event, Event::Stopped {}));
    assert!(
        events.iter().any(|event| matches!(event, Event::Error { message } if message.contains("incomplete AMB1 frame"))),
        "{events:?}"
    );
    drop(worker);
    peer.finish();
}

#[test]
fn no_command_before_advertisement() {
    let (address, peer) = peer(Case::NoAdvertisement);
    let mut worker = Worker::new(address);
    worker.until(|event| matches!(event, Event::Waiting {}));
    worker.apply(1920);
    worker.until(|event| matches!(event, Event::QualityFailed { reason, .. } if reason.contains("not advertised")));
    worker.stop();
    drop(worker);
    peer.finish();
}
#[test]
fn pending_permission_never_sends_prepare() {
    let (address, peer) = peer(Case::PermissionPending);
    let mut worker = Worker::new(address);
    worker.advertised();
    worker.apply(1920);
    worker.until(
        |event| matches!(event, Event::QualityFailed { reason, .. } if reason.contains("permission")),
    );
    worker.stop();
    drop(worker);
    peer.finish();
}
#[test]
fn unsupported_explicit_mode_is_not_silently_replaced() {
    let (address, peer) = peer(Case::UnsupportedMode);
    let mut worker = Worker::new(address);
    worker.advertised();
    worker.apply(1280);
    worker.until(|event| matches!(event, Event::QualityFailed { reason, .. } if reason.contains("not in the advertised")));
    worker.stop();
    drop(worker);
    peer.finish();
}
