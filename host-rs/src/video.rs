mod decoder;
mod ffmpeg;
mod pixels;
pub mod transform;
use crate::{
    control::{self, Command, Event},
    quality::Action,
    settings::Settings,
    transport::{Tcp, Transport},
    usb::{DeviceId, Usb},
    wire,
};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl Config {
    pub fn parse(bytes: &[u8]) -> crate::Result<Self> {
        if bytes.len() != 16 || bytes[12] != 1 || bytes[13] != 1 {
            return Err("invalid VIDEO_CONFIG".into());
        }
        let c = Self {
            width: u16::from_le_bytes(bytes[0..2].try_into()?) as u32,
            height: u16::from_le_bytes(bytes[2..4].try_into()?) as u32,
            fps: u16::from_le_bytes(bytes[4..6].try_into()?) as u32,
        };
        if c.width == 0
            || c.height == 0
            || c.fps == 0
            || c.width > 8192
            || c.height > 8192
            || c.width as u64 * c.height as u64 > 33_554_432
        {
            return Err("invalid VIDEO_CONFIG dimensions or rate".into());
        }
        Ok(c)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub rotation: u16,
    pub mirror: bool,
    pub vertical: bool,
}
impl From<&Settings> for Transform {
    fn from(s: &Settings) -> Self {
        Self {
            rotation: s.rotation,
            mirror: s.mirror,
            vertical: s.vertical_flip,
        }
    }
}

pub fn worker(parent: u32) -> crate::Result<()> {
    crate::platform::bind_parent(parent)?;
    let first =
        control::read::<Command>(&mut io::stdin().lock())?.ok_or("missing start command")?;
    let Command::Start { settings, device } = first else {
        return Err("first command must start a session".into());
    };
    settings.validate()?;
    let stop = Arc::new(AtomicBool::new(false));
    let changes = Arc::new(Mutex::new(None));
    let quality_request = Arc::new(Mutex::new(None));
    let thread_stop = stop.clone();
    let thread_changes = changes.clone();
    let thread_quality = quality_request.clone();
    std::thread::spawn(move || {
        let mut reader = io::stdin().lock();
        loop {
            match control::read::<Command>(&mut reader) {
                Ok(Some(Command::Transform {
                    rotation,
                    mirror,
                    vertical_flip,
                })) if [0, 90, 180, 270].contains(&rotation) => {
                    *thread_changes.lock().unwrap() = Some(Transform {
                        rotation,
                        mirror,
                        vertical: vertical_flip,
                    });
                }
                Ok(Some(Command::PrepareQuality { width, height, fps })) => {
                    *thread_quality.lock().unwrap() = Some((width, height, fps));
                }
                _ => {
                    thread_stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
    });
    let mut stdout = io::stdout().lock();
    control::write(&mut stdout, &Event::Connecting {})?;
    let result = receive(
        settings,
        device,
        &stop,
        &changes,
        &quality_request,
        |event| control::write(&mut stdout, &event).map_err(Into::into),
    );
    if let Err(error) = &result {
        #[cfg(target_os = "linux")]
        if let Some(denied) = error.downcast_ref::<crate::usb::PermissionDenied>() {
            control::write(
                &mut stdout,
                &Event::UsbPermission {
                    device: denied.0.clone(),
                },
            )?;
        }
        control::write(
            &mut stdout,
            &Event::Error {
                message: error.to_string().chars().take(2048).collect(),
            },
        )?;
    }
    control::write(&mut stdout, &Event::Stopped {})?;
    result
}

fn receive(
    settings: Settings,
    device: Option<DeviceId>,
    stop: &AtomicBool,
    changes: &Mutex<Option<Transform>>,
    quality_request: &Mutex<Option<(u16, u16, u16)>>,
    event: impl FnMut(Event) -> crate::Result<()>,
) -> crate::Result<()> {
    let transport: Box<dyn Transport> = if settings.transport == "lan" {
        Box::new(Tcp::connect(
            &settings.lan_host,
            48527,
            Duration::from_secs(5),
        )?)
    } else {
        Box::new(Usb::connect(&device.ok_or("select a USB device")?)?)
    };
    let output_path = settings.output.trim().to_string();
    receive_stream(
        settings,
        transport,
        stop,
        changes,
        quality_request,
        event,
        |c| crate::output::Camera::open(&output_path, c.width, c.height, c.fps),
    )
}
fn receive_stream<S: crate::output::Sink>(
    settings: Settings,
    mut transport: Box<dyn Transport>,
    stop: &AtomicBool,
    changes: &Mutex<Option<Transform>>,
    quality_request: &Mutex<Option<(u16, u16, u16)>>,
    mut event: impl FnMut(Event) -> crate::Result<()>,
    mut create_output: impl FnMut(Config) -> crate::Result<S>,
) -> crate::Result<()> {
    let mut decoder = Some(decoder::Decoder::open(&settings.decode)?);
    let idr = transport.send(
        &wire::Frame::new(wire::IDR_REQUEST, 2, Vec::new())?,
        Duration::from_secs(2),
    );
    if let Err(error) = idr {
        // A phone awaiting accessory permission may not read the OUT endpoint
        // yet. This best-effort request never blocks incoming video acceptance.
        if settings.transport != "usb"
            || error.downcast_ref::<rusb::Error>() != Some(&rusb::Error::Timeout)
        {
            return Err(error);
        }
    }
    event(Event::Waiting {})?;
    let mut switch = crate::quality::Switch::default();
    let clock = Instant::now();
    let now_ms = || clock.elapsed().as_millis() as u64;
    let mut modes: Vec<crate::quality::Mode> = Vec::new();
    let mut catalog_revision = 0u16;
    // A new PC sends no quality command before the advertisement; frames
    // between PAUSED and the new config are stale by contract.
    let mut advertised = false;
    let mut permission = wire::PERMISSION_UNAVAILABLE;
    let mut config = None;
    let mut output = None;
    let mut pixels = pixels::Pixels::default();
    let mut transform = Transform::from(&settings);
    let mut frame = ffmpeg::Frame::empty()?;
    let mut last_pts = 0;
    let mut started = false;
    let mut decoded = 0;
    let mut submitted = 0;
    let mut report = Instant::now() - Duration::from_secs(1);
    while !stop.load(Ordering::Acquire) {
        // The GUI's apply request; the machine owns one-at-a-time, and the
        // advertisement gates it (spec slice 02).
        if let Some((width, height, fps)) = quality_request.lock().unwrap().take() {
            let rejection = if !advertised {
                Some("the phone has not advertised its capabilities yet")
            } else if permission != wire::PERMISSION_READY {
                Some("camera permission or camera availability is not ready")
            } else if !modes
                .iter()
                .any(|mode| mode.tuple() == (width, height, fps))
            {
                Some("the requested quality is not in the advertised camera modes")
            } else {
                None
            };
            if let Some(reason) = rejection {
                event(Event::QualityFailed {
                    reason: reason.into(),
                    terminal: false,
                })?;
                continue;
            }
            for action in switch.step(
                now_ms(),
                crate::quality::Event::Prepare {
                    revision: catalog_revision,
                    width,
                    height,
                    fps,
                },
            ) {
                if let Action::SendPrepareQuality {
                    request_id,
                    revision,
                    width,
                    height,
                    fps,
                } = action
                {
                    let payload = wire::PrepareQuality {
                        request_id,
                        catalog_revision: revision,
                        width,
                        height,
                        target_fps: fps,
                    }
                    .encode();
                    transport.send(
                        &wire::Frame::new(wire::PREPARE_QUALITY, 0, payload)?,
                        Duration::from_secs(2),
                    )?;
                }
            }
        }
        // The apply deadline fires on the poll/frame cadence even when the
        // phone stalls (spec slice 02).
        if switch.busy() {
            for action in switch.step(now_ms(), crate::quality::Event::DeadlineExceeded) {
                match action {
                    Action::ReportFailed { reason } => {
                        event(Event::QualityFailed {
                            reason: reason.to_string(),
                            terminal: true,
                        })?;
                    }
                    Action::StopCapture | Action::CloseConnection => {
                        return Err(reason_of_expiry());
                    }
                    _ => {}
                }
            }
        }
        // Poll every stream state, retaining partial frames in the transport.
        // No long receive can hide Apply, Stop or the monotonic apply deadline.
        let Some(incoming) = transport.poll_receive(Duration::from_millis(100))? else {
            continue;
        };
        // The quality control plane (slice 02); unknown types keep being
        // ignored by the VIDEO_AU guard below.
        let quality_event = match incoming.header.kind {
            wire::QUALITY_CAPABILITIES => {
                if incoming.payload.first() != Some(&1) {
                    continue;
                }
                let caps = wire::Capabilities::decode(&incoming.payload)?;
                switch.step(
                    now_ms(),
                    crate::quality::Event::EpochObserved(caps.stream_epoch),
                );
                catalog_revision = caps.catalog_revision;
                advertised = true;
                permission = caps.permission;
                modes = caps
                    .modes
                    .into_iter()
                    .map(crate::quality::Mode::from)
                    .filter(|mode| mode.usable())
                    .collect();
                event(Event::QualityCapabilities {
                    permission: caps.permission,
                    revision: caps.catalog_revision,
                    epoch: caps.stream_epoch,
                    modes: modes.clone(),
                })?;
                continue;
            }
            wire::QUALITY_RESULT => {
                let result = wire::QualityResult::decode(&incoming.payload)?;
                Some(match result.status {
                    wire::RESULT_SWITCHING | wire::RESULT_STAGED => {
                        crate::quality::Event::PrepareAccepted {
                            request_id: result.request_id,
                            // Only the phone knows whether it is waiting for its
                            // user. A retained VIDEO_CONFIG cannot answer this.
                            streaming: result.status == wire::RESULT_SWITCHING,
                        }
                    }
                    wire::RESULT_REJECTED => crate::quality::Event::Rejected {
                        request_id: result.request_id,
                        reason: result.reason,
                    },
                    wire::RESULT_APPLIED => crate::quality::Event::CaptureStarted {
                        request_id: result.request_id,
                        width: result.applied_width,
                        height: result.applied_height,
                        fps: result.applied_fps,
                    },
                    _ => continue,
                })
            }
            wire::PAUSED => {
                let paused = wire::Epoch::decode(&incoming.payload)?;
                Some(crate::quality::Event::Paused {
                    request_id: paused.request_id,
                    epoch: paused.stream_epoch,
                })
            }
            _ => None,
        };
        if let Some(quality_event) = quality_event {
            let was_staged = switch.staged();
            let actions = switch.step(now_ms(), quality_event);
            report_quality(actions.clone(), &mut event)?;
            let boundary = actions.contains(&Action::ReleaseOutputs);
            for action in actions {
                match action {
                    Action::ReleaseOutputs => {
                        // The phone writer fenced old media before its reply.
                        // Drop all old resources before Commit or staged wait.
                        output = None;
                        event(Event::Waiting {})?;
                        decoder = None;
                        pixels = pixels::Pixels::default();
                        frame = ffmpeg::Frame::empty()?;
                        config = None;
                        started = false;
                        decoded = 0;
                        submitted = 0;
                        last_pts = 0;
                    }
                    Action::SendCommitQuality { request_id, epoch } => {
                        let payload = wire::Epoch {
                            request_id,
                            stream_epoch: epoch,
                        }
                        .encode();
                        transport.send(
                            &wire::Frame::new(wire::COMMIT_QUALITY, 0, payload)?,
                            Duration::from_secs(2),
                        )?;
                    }
                    _ => {}
                }
            }
            if boundary || switch.staged() != was_staged {
                event(Event::QualityProgress {
                    staged: switch.staged(),
                })?;
            }
            continue;
        }
        if incoming.header.kind == wire::VIDEO_CONFIG {
            let next = Config::parse(&incoming.payload)?;
            if switch.awaiting_video() {
                let was_staged = switch.staged();
                report_quality(
                    switch.step(
                        now_ms(),
                        crate::quality::Event::VideoConfig {
                            width: next.width as u16,
                            height: next.height as u16,
                            fps: next.fps as u16,
                        },
                    ),
                    &mut event,
                )?;
                if was_staged {
                    event(Event::QualityProgress { staged: false })?;
                }
            } else if config.is_some_and(|c| c != next) {
                return Err("VIDEO_CONFIG changed during active output".into());
            }
            config = Some(next);
            continue;
        }
        if incoming.header.kind != wire::VIDEO_AU || incoming.payload.is_empty() {
            continue;
        }
        let Some(c) = config else { continue };
        let keyframe = incoming.header.flags & wire::KEYFRAME != 0;
        if !started && (!keyframe || incoming.header.flags & wire::CONFIG_INCLUDED == 0) {
            continue;
        }
        if started && incoming.header.pts_us <= last_pts {
            return Err("non-monotonic VIDEO_AU PTS".into());
        }
        if decoder.is_none() {
            decoder = Some(decoder::Decoder::open(&settings.decode)?);
        }
        let decoder = decoder.as_mut().unwrap();
        if incoming.header.flags & wire::DISCONTINUITY != 0 {
            decoder.flush();
        }
        decoder.send(&incoming.payload, incoming.header.pts_us, keyframe)?;
        last_pts = incoming.header.pts_us;
        while decoder.receive(&mut frame)? {
            if stop.load(Ordering::Acquire) {
                break;
            }
            if frame.width() != c.width || frame.height() != c.height {
                return Err("decoded dimensions do not match VIDEO_CONFIG".into());
            }
            if c.width % 2 != 0 {
                return Err("YUYV output width must be even".into());
            }
            if let Some(next) = changes.lock().unwrap().take() {
                transform = next;
            }
            decoded += 1;
            started = true;
            if output.is_none() {
                output = Some(create_output(c)?);
            }
            let camera = output.as_mut().unwrap();
            let active = camera.ready()?;
            if active {
                let (stride, size) = camera.layout();
                let bytes = pixels.convert(&frame, c, stride, size, transform)?;
                camera.submit(bytes, c.width, c.height)?;
                submitted += 1;
                report_quality(
                    switch.step(now_ms(), crate::quality::Event::OutputReady),
                    &mut event,
                )?;
            }
            if report.elapsed() >= Duration::from_secs(1) {
                event(Event::Stats {
                    decoded,
                    submitted,
                    active,
                    width: c.width,
                    height: c.height,
                    target_fps: c.fps,
                    decoder: decoder.active().into(),
                })?;
                report = Instant::now();
            }
        }
    }
    for action in switch.step(now_ms(), crate::quality::Event::UserStop) {
        if let Action::ReportFailed { reason } = action {
            event(Event::QualityFailed {
                reason: reason.into(),
                terminal: false,
            })?;
        }
    }
    Ok(())
}
fn report_quality(
    actions: Vec<Action>,
    event: &mut impl FnMut(Event) -> crate::Result<()>,
) -> crate::Result<()> {
    let terminal = actions.contains(&Action::CloseConnection);
    for action in actions {
        match action {
            Action::ReportApplied { width, height, fps } => {
                event(Event::QualityApplied { width, height, fps })?
            }
            Action::ReportFailed { reason } => event(Event::QualityFailed {
                reason: reason.into(),
                terminal,
            })?,
            Action::StopCapture | Action::CloseConnection => {
                return Err("quality apply failed".into());
            }
            _ => {}
        }
    }
    Ok(())
}
fn reason_of_expiry() -> Box<dyn std::error::Error + Send + Sync> {
    "the apply deadline expired".into()
}

#[cfg(test)]
mod quality_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_config_limits() {
        let mut data = [0; 16];
        data[..2].copy_from_slice(&1280u16.to_le_bytes());
        data[2..4].copy_from_slice(&720u16.to_le_bytes());
        data[4..6].copy_from_slice(&30u16.to_le_bytes());
        data[12] = 1;
        data[13] = 1;
        assert_eq!(
            Config::parse(&data).unwrap(),
            Config {
                width: 1280,
                height: 720,
                fps: 30
            }
        );
        data[..2].copy_from_slice(&8193u16.to_le_bytes());
        assert!(Config::parse(&data).is_err());
        data[..2].copy_from_slice(&8192u16.to_le_bytes());
        data[2..4].copy_from_slice(&8192u16.to_le_bytes());
        assert!(Config::parse(&data).is_err());
    }
    #[test]
    fn real_h264_decode_and_all_transforms() {
        let mut decoder = decoder::Decoder::open("off").unwrap();
        decoder
            .send(include_bytes!("../tests/fixtures/red-blue.h264"), 1, true)
            .unwrap();
        let mut frame = ffmpeg::Frame::empty().unwrap();
        assert!(decoder.receive(&mut frame).unwrap());
        assert_eq!((frame.width(), frame.height()), (64, 32));
        assert_eq!(decoder.active(), "software");
        let mut pixels = pixels::Pixels::default();
        let c = Config {
            width: 64,
            height: 32,
            fps: 30,
        };
        let bpp = if cfg!(windows) { 4 } else { 2 };
        for rotation in [0, 90, 180, 270] {
            for mirror in [false, true] {
                for vertical in [false, true] {
                    let output = pixels
                        .convert(
                            &frame,
                            c,
                            64 * bpp,
                            64 * 32 * bpp,
                            Transform {
                                rotation,
                                mirror,
                                vertical,
                            },
                        )
                        .unwrap();
                    assert_eq!(output.len(), 64 * 32 * bpp);
                    if rotation == 0 {
                        let red_row = if vertical ^ cfg!(windows) { 24 } else { 8 };
                        let blue_row = 31 - red_row;
                        if cfg!(windows) {
                            assert!(output[red_row * 256 + 32 * 4] > 180);
                            assert!(output[blue_row * 256 + 32 * 4 + 2] > 180);
                        } else {
                            assert!(
                                output[red_row * 128 + 32 * 2] > output[blue_row * 128 + 32 * 2]
                            );
                        }
                    }
                }
            }
        }
        assert!(!decoder.receive(&mut frame).unwrap());
    }
    #[test]
    fn minimal_runtime_closure_when_requested() {
        if std::env::var("MOBILE_WEBCAM_REQUIRE_MINIMAL").as_deref() != Ok("1") {
            return;
        }
        let mut state = std::ptr::null_mut();
        let mut names = Vec::new();
        unsafe {
            loop {
                let codec = ffmpeg::abi::av_codec_iterate(&mut state);
                if codec.is_null() {
                    break;
                }
                names.push(
                    std::ffi::CStr::from_ptr((*codec).name)
                        .to_str()
                        .unwrap()
                        .to_owned(),
                );
                assert_eq!(ffmpeg::abi::av_codec_is_decoder(codec), 1);
                assert_eq!(ffmpeg::abi::av_codec_is_encoder(codec), 0);
            }
        }
        assert_eq!(names, ["h264"]);
    }
}
