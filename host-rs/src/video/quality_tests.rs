//! Established-stream pipeline: real TCP framing, FFmpeg and pixel conversion;
//! only the OS output sink is replaced. This is not virtual-camera acceptance.
use super::*;
// The same phone-side TLS fixture the integration tests use, so this pipeline
// also runs over the authenticated transport rather than a plaintext socket.
#[path = "../../tests/common/mod.rs"]
mod common;
use crate::output::Sink;
use common::{ACCESS_KEY, PHONE_SHA256, phone_tls};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::AtomicUsize,
    thread,
};
const H264: &[u8] = include_bytes!("../../tests/fixtures/red-blue.h264");
fn send(stream: &mut impl Write, kind: u8, payload: Vec<u8>, flags: u16, pts: u64) {
    let mut frame = wire::Frame::new(kind, 0, payload).unwrap();
    frame.header.flags = flags;
    frame.header.pts_us = pts;
    stream.write_all(&frame.header.encode().unwrap()).unwrap();
    stream.write_all(&frame.payload).unwrap();
}
fn read(stream: &mut impl Read) -> wire::Frame {
    let mut bytes = [0; wire::HEADER_SIZE];
    stream.read_exact(&mut bytes).unwrap();
    let header = wire::Header::decode(&bytes).unwrap();
    let mut payload = vec![0; header.payload_len as usize];
    stream.read_exact(&mut payload).unwrap();
    wire::Frame { header, payload }
}
fn config(fps: u16) -> Vec<u8> {
    let mut payload = vec![0; 16];
    payload[..2].copy_from_slice(&64u16.to_le_bytes());
    payload[2..4].copy_from_slice(&32u16.to_le_bytes());
    payload[4..6].copy_from_slice(&fps.to_le_bytes());
    payload[12] = 1;
    payload[13] = 1;
    payload
}
struct TestSink {
    ready: bool,
    submitted: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}
impl Drop for TestSink {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl Sink for TestSink {
    fn layout(&self) -> (usize, usize) {
        let bpp = if cfg!(windows) { 4 } else { 2 };
        (64 * bpp, 64 * 32 * bpp)
    }
    fn ready(&mut self) -> crate::Result<bool> {
        Ok(self.ready)
    }
    fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        assert_eq!((width, height), (64, 32));
        assert_eq!(bytes.len(), self.layout().1);
        assert!(bytes.iter().any(|byte| *byte != 0));
        self.submitted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Applied,
    IdleStage,
    WrongFps,
    OutputUnavailable,
}
/// A closed TLS peer surfaces as EOF or a missing close_notify, never as a
/// clean zero-length read; both mean the worker sent nothing further.
fn expect_closed(stream: &mut impl Read) {
    // The worker's apply deadline is 5s, and a case that waits for output keeps
    // the connection open that long; closing early would cut the deadline short.
    let deadline = Instant::now() + Duration::from_secs(9);
    let mut byte = [0];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return,
            Ok(_) => panic!("unexpected phone frame after the pipeline finished"),
            Err(error) => {
                let gone = matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                        | std::io::ErrorKind::ConnectionAborted
                );
                assert!(
                    gone || error.to_string().contains("close_notify"),
                    "unexpected read error: {error}"
                );
                if !gone || Instant::now() >= deadline {
                    return;
                }
            }
        }
    }
}

fn pipeline(case: Case) {
    let submitted = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let peer_dropped = dropped.clone();
    let (marker_sent, marker_received) = std::sync::mpsc::channel();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The peer must be accepting before the client starts its TLS handshake:
    // the kernel completes the TCP connect from its backlog, but the handshake
    // needs the phone side to answer.
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut stream = phone_tls(stream);
        // Pairing first: the credential must match before any AMB1 frame.
        let hello = read(&mut stream);
        assert_eq!(hello.header.kind, wire::HELLO);
        assert_eq!(
            hello.payload,
            crate::secure::parse_hex(ACCESS_KEY, 16).unwrap()
        );
        // Pairing completes only with an empty HELLO_ACK carrying sequence 1.
        let ack = wire::Frame::new(wire::HELLO_ACK, hello.header.sequence, Vec::new()).unwrap();
        stream.write_all(&ack.header.encode().unwrap()).unwrap();
        assert_eq!(read(&mut stream).header.kind, wire::IDR_REQUEST);
        let caps = wire::Capabilities {
            extension_version: 1,
            permission: wire::PERMISSION_READY,
            catalog_revision: 7,
            stream_epoch: 0,
            modes: vec![wire::QualityMode {
                width: 64,
                height: 32,
                target_fps: 30,
                bitrate: 100_000,
            }],
        };
        send(
            &mut stream,
            wire::QUALITY_CAPABILITIES,
            caps.encode().unwrap(),
            0,
            0,
        );
        send(&mut stream, wire::VIDEO_CONFIG, config(15), 0, 0);
        send(
            &mut stream,
            wire::VIDEO_AU,
            H264.to_vec(),
            wire::KEYFRAME | wire::CONFIG_INCLUDED,
            1_000_000,
        );
        let prepare = read(&mut stream);
        assert_eq!(prepare.header.kind, wire::PREPARE_QUALITY);
        let request = wire::PrepareQuality::decode(&prepare.payload).unwrap();
        if case == Case::IdleStage {
            let reply = |status| {
                wire::QualityResult {
                    request_id: request.request_id,
                    status,
                    reason: 0,
                    applied_width: 0,
                    applied_height: 0,
                    applied_fps: 0,
                }
                .encode()
            };
            // A stopped phone stages even though the PC retained its old config.
            send(
                &mut stream,
                wire::QUALITY_RESULT,
                reply(wire::RESULT_STAGED),
                0,
                0,
            );
            marker_received
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert_eq!(peer_dropped.load(Ordering::SeqCst), 1);
            send(
                &mut stream,
                wire::QUALITY_RESULT,
                reply(wire::RESULT_SWITCHING),
                0,
                0,
            );
        } else {
            let paused = wire::Epoch {
                request_id: request.request_id,
                stream_epoch: 0,
            };
            send(&mut stream, wire::PAUSED, paused.encode(), 0, 0);
            let commit = read(&mut stream);
            assert_eq!(commit.header.kind, wire::COMMIT_QUALITY);
            assert_eq!(
                wire::Epoch::decode(&commit.payload).unwrap().stream_epoch,
                1
            );
            assert_eq!(
                peer_dropped.load(Ordering::SeqCst),
                1,
                "old output was not released before commit"
            );
        }
        // Old video after PAUSED is ignored until the new config arrives.
        send(
            &mut stream,
            wire::VIDEO_AU,
            H264.to_vec(),
            wire::KEYFRAME | wire::CONFIG_INCLUDED,
            2_000_000,
        );
        send(
            &mut stream,
            wire::VIDEO_CONFIG,
            config(if case == Case::WrongFps { 25 } else { 30 }),
            0,
            0,
        );
        if case != Case::WrongFps {
            send(&mut stream, wire::VIDEO_AU, H264.to_vec(), 0, 1);
            send(
                &mut stream,
                wire::VIDEO_AU,
                H264.to_vec(),
                wire::KEYFRAME,
                1,
            );
            // FIFO marker proves the invalid AUs were consumed before asserting
            // they produced no output; no timing-based sleep or lucky sample.
            let mut marker = caps;
            marker.stream_epoch = 1;
            send(
                &mut stream,
                wire::QUALITY_CAPABILITIES,
                marker.encode().unwrap(),
                0,
                0,
            );
            marker_received
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            send(
                &mut stream,
                wire::VIDEO_AU,
                H264.to_vec(),
                wire::KEYFRAME | wire::CONFIG_INCLUDED,
                1,
            );
        }
        expect_closed(&mut stream);
    });
    let client = crate::secure::SecureTcp::connect(
        "127.0.0.1",
        port,
        PHONE_SHA256,
        ACCESS_KEY,
        Duration::from_secs(5),
    )
    .unwrap();
    let stop = AtomicBool::new(false);
    let request = Mutex::new(None);
    let changes = Mutex::new(None);
    let mut opens = Vec::new();
    let mut applied = false;
    let mut marker_observed = false;
    let mut failure = None;
    let mut requested = false;
    let result = receive_stream(
        Settings {
            transport: "lan".into(),
            decode: "off".into(),
            ..Default::default()
        },
        Box::new(client),
        &stop,
        &changes,
        &request,
        |event| {
            match event {
                Event::QualityProgress { staged: true } if case == Case::IdleStage => {
                    assert_eq!(
                        dropped.load(Ordering::SeqCst),
                        1,
                        "staged wait hid a still-live old output"
                    );
                    marker_sent.send(()).unwrap();
                }
                Event::QualityCapabilities { .. } if requested => {
                    assert!(!applied, "metadata alone reported Applied");
                    assert_eq!(
                        submitted.load(Ordering::SeqCst),
                        1,
                        "non-config/non-key video reached the new output"
                    );
                    marker_observed = true;
                    marker_sent.send(()).unwrap();
                }
                Event::Stats {
                    submitted: count, ..
                } if !requested && count == 1 => {
                    requested = true;
                    *request.lock().unwrap() = Some((64, 32, 30));
                }
                Event::QualityApplied { width, height, fps } => {
                    assert!(
                        marker_observed,
                        "unqualified AU or metadata completed the apply before the FIFO marker"
                    );
                    assert_eq!((width, height, fps), (64, 32, 30));
                    assert_eq!(
                        submitted.load(Ordering::SeqCst),
                        2,
                        "Applied preceded output acceptance or admitted stale/non-config video"
                    );
                    applied = true;
                    stop.store(true, Ordering::Release);
                }
                Event::QualityFailed { reason, .. } => failure = Some(reason),
                _ => {}
            }
            Ok(())
        },
        |mode| {
            opens.push(mode);
            Ok(TestSink {
                ready: case != Case::OutputUnavailable || opens.len() == 1,
                submitted: submitted.clone(),
                dropped: dropped.clone(),
            })
        },
    );
    server.join().unwrap();
    if matches!(case, Case::Applied | Case::IdleStage) {
        assert!(result.is_ok());
        assert!(applied);
        assert!(failure.is_none());
        assert_eq!(
            opens.iter().map(|mode| mode.fps).collect::<Vec<_>>(),
            vec![15, 30]
        );
    } else {
        assert!(result.is_err());
        assert!(!applied);
        let reason = failure.unwrap();
        assert!(
            reason.contains(if case == Case::WrongFps {
                "did not match"
            } else {
                "deadline"
            }),
            "{reason}"
        );
        assert_eq!(submitted.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn applied_requires_new_config_keyframe_and_output_acceptance_with_pts_reset() {
    pipeline(Case::Applied);
}
#[test]
fn stopped_phone_staging_releases_old_config_and_accepts_reset_pts() {
    pipeline(Case::IdleStage);
}
#[test]
fn advertised_fps_mismatch_is_not_applied() {
    pipeline(Case::WrongFps);
}
#[test]
fn unavailable_output_does_not_claim_applied_and_hits_deadline() {
    pipeline(Case::OutputUnavailable);
}
