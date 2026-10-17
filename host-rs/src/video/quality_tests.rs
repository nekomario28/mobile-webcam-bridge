//! Established-stream pipeline: real TCP framing, FFmpeg and pixel conversion;
//! only the OS output sink is replaced. This is not virtual-camera acceptance.
//! `applied_switch_reaches_a_real_v4l2_device` opts into the real OS sink through
//! `AMB_V4L2_DEVICE`, so the sink leg is a kernel driver rather than a stand-in;
//! a consuming application is still not attached (issue #8).
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
/// The OS sink when a real V4L2 device is configured, otherwise the test sink.
/// Only the device path reaches a real consumer; every other test is unchanged.
enum AnySink {
    Test(TestSink),
    #[cfg(target_os = "linux")]
    Device {
        camera: crate::output::Camera,
        submitted: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    },
}
impl Drop for AnySink {
    fn drop(&mut self) {
        // The peer asserts the old output was released before Commit; the test
        // sink records that through its own Drop, so the real device does too.
        #[cfg(target_os = "linux")]
        if let AnySink::Device { dropped, .. } = self {
            dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl Sink for AnySink {
    fn layout(&self) -> (usize, usize) {
        match self {
            AnySink::Test(sink) => sink.layout(),
            #[cfg(target_os = "linux")]
            AnySink::Device { camera, .. } => camera.layout(),
        }
    }
    fn ready(&mut self) -> crate::Result<bool> {
        match self {
            AnySink::Test(sink) => sink.ready(),
            #[cfg(target_os = "linux")]
            AnySink::Device { camera, .. } => crate::output::Sink::ready(camera),
        }
    }
    fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        match self {
            AnySink::Test(sink) => sink.submit(bytes, width, height),
            #[cfg(target_os = "linux")]
            AnySink::Device {
                camera, submitted, ..
            } => {
                crate::output::Sink::submit(camera, bytes, width, height)?;
                submitted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    }
}
// The real-sink path is opt-in per test, not per process: a global switch would
// change the outcome of the cases that assert an unavailable or replaced sink.
#[cfg(target_os = "linux")]
thread_local! {
    static V4L2_DEVICE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}
#[cfg(target_os = "linux")]
fn with_v4l2_device<T>(device: Option<String>, run: impl FnOnce() -> T) -> T {
    V4L2_DEVICE.with(|cell| *cell.borrow_mut() = device);
    let result = run();
    V4L2_DEVICE.with(|cell| *cell.borrow_mut() = None);
    result
}
#[cfg(target_os = "linux")]
fn real_v4l2_device() -> Option<String> {
    V4L2_DEVICE.with(|cell| cell.borrow().clone())
}
/// Both device tests use one loopback node and libtest runs tests in parallel:
/// two producers on the same device fail each other, so they take this lock.
#[cfg(target_os = "linux")]
static DEVICE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(target_os = "linux")]
fn device_guard() -> std::sync::MutexGuard<'static, ()> {
    DEVICE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            #[cfg(target_os = "linux")]
            if let Some(device) = real_v4l2_device() {
                return Ok(AnySink::Device {
                    camera: crate::output::Camera::open(
                        &device,
                        mode.width,
                        mode.height,
                        mode.fps,
                    )?,
                    submitted: submitted.clone(),
                    dropped: dropped.clone(),
                });
            }
            Ok(AnySink::Test(TestSink {
                ready: case != Case::OutputUnavailable || opens.len() == 1,
                submitted: submitted.clone(),
                dropped: dropped.clone(),
            }))
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

/// Real V4L2 output: the same applied switch as `Case::Applied`, but the frames
/// go to a real `v4l2loopback` device, so "the sink accepted the frame" is what a
/// kernel driver accepted rather than an in-crate sink. Skipped unless
/// `AMB_V4L2_DEVICE` names an existing device, so CI stays green without one.
///
/// A *consumer* is not yet attached here. This device reports the output side to
/// its first opener, so a reader must open while the writer still holds it, which
/// needs a streaming case that outlives the reader's start; `ffmpeg -f v4l2`
/// reports `Not a video capture device` when it opens outside that window. The
/// concrete follow-up (longer post-apply streaming plus a reader that attaches
/// mid-stream) is tracked in issue #8.
#[test]
#[cfg(target_os = "linux")]
fn applied_switch_reaches_a_real_v4l2_device() {
    let device = std::env::var("AMB_V4L2_DEVICE")
        .ok()
        .filter(|device| std::path::Path::new(device).exists());
    let Some(device) = device else {
        eprintln!("SKIP v4l2 acceptance: set AMB_V4L2_DEVICE to a v4l2loopback device");
        return;
    };
    // The pipeline already asserts Applied, the old-output release before Commit
    // and the submission counts; with the device sink those are device writes.
    let _device_guard = device_guard();
    with_v4l2_device(Some(device.clone()), || pipeline(Case::Applied));
    println!("v4l2 receipt: device={device} real_sink=accepted applied=true");
}

/// A real external consumer must receive frames from the real sink. Skipped unless
/// `AMB_V4L2_DEVICE` names an existing v4l2loopback device, so CI without one
/// stays green; measured on a real device this delivers ~63 of 70 frames.
///
/// The producer must keep streaming while the reader attaches: with only a couple
/// of dozen frames the driver exposes nothing, which is what made three earlier
/// measurements of this look like a producer bug.
#[test]
#[cfg(target_os = "linux")]
fn a_real_consumer_records_frames_from_the_real_sink() {
    let device = std::env::var("AMB_V4L2_DEVICE")
        .ok()
        .filter(|device| std::path::Path::new(device).exists());
    let Some(device) = device else {
        eprintln!("SKIP v4l2 consumer acceptance: set AMB_V4L2_DEVICE to a v4l2loopback device");
        return;
    };
    let _device_guard = device_guard();
    let output = std::env::temp_dir().join(format!("amb-consumer-{}.mkv", std::process::id()));
    let _ = std::fs::remove_file(&output);
    let mut sink = crate::output::Camera::open(&device, 64, 32, 25).expect("the sink must open");
    let mut consumer = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "v4l2", "-i", &device])
        .args(["-t", "4", "-c", "copy"])
        .arg(&output)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("ffmpeg is required for the consumer acceptance");
    thread::sleep(Duration::from_millis(500));
    let mut submitted = 0;
    for step in 0..70u8 {
        sink.submit(&vec![step; sink.size], 64, 32).unwrap();
        submitted += 1;
        thread::sleep(Duration::from_millis(30));
    }
    drop(sink);
    let deadline = Instant::now() + Duration::from_secs(10);
    while consumer.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    if consumer.try_wait().ok().flatten().is_none() {
        let _ = consumer.kill();
    }
    let _ = consumer.wait();
    let mut consumer_log = String::new();
    if let Some(mut stderr) = consumer.stderr.take() {
        let _ = stderr.read_to_string(&mut consumer_log);
    }
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "frame=pts_time,width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .expect("ffprobe is required for the consumer acceptance");
    let text = String::from_utf8_lossy(&probe.stdout);
    let frames: Vec<(f64, u32, u32)> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',');
            Some((
                parts.next()?.trim().parse().ok()?,
                parts.next()?.trim().parse().ok()?,
                parts.next()?.trim().parse().ok()?,
            ))
        })
        .collect();
    let span = frames
        .first()
        .zip(frames.last())
        .map(|(first, last)| last.0 - first.0)
        .unwrap_or(0.0);
    println!(
        "v4l2 consumer receipt: device={device} submitted={submitted} frames={} span={span:.3}s",
        frames.len()
    );
    assert!(
        frames.len() >= 5,
        "the consumer recorded {} of {submitted} submitted frames: probe={} ffmpeg={consumer_log}",
        frames.len(),
        String::from_utf8_lossy(&probe.stderr)
    );
    assert!(
        frames
            .iter()
            .all(|(_, width, height)| (*width, *height) == (64, 32)),
        "the consumer saw unexpected frame sizes: {:?}",
        frames.iter().map(|(_, w, h)| (*w, *h)).collect::<Vec<_>>()
    );
    assert!(
        frames.windows(2).all(|pair| pair[1].0 >= pair[0].0),
        "consumer PTS went backwards: {:?}",
        frames.iter().map(|(pts, _, _)| *pts).collect::<Vec<_>>()
    );
    assert!(span > 0.0, "the consumer saw no measurable time span");
}
