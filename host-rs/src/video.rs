mod decoder;
mod ffmpeg;
mod pixels;
pub mod transform;
use crate::{
    control::{self, Command, Event},
    quality::Action,
    secure::SecureTcp,
    settings::Settings,
    transport::Transport,
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
    let settings = *settings;
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
        std::env::var_os(crate::preview::PATH_ENV).map(Into::into),
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
                // The frame reader is shared with any protocol, so its message
                // says "frame"; a user of this app needs to know it was AMB1.
                message: describe_receive_failure(error.as_ref()),
            },
        )?;
    }
    control::write(&mut stdout, &Event::Stopped {})?;
    result
}

/// Put the protocol back into a transport error the user will read.
fn describe_receive_failure(error: &(dyn std::error::Error + Send + Sync)) -> String {
    let message = error.to_string();
    let named = message.replace(
        "incomplete frame deadline expired",
        "incomplete AMB1 frame deadline expired",
    );
    named.chars().take(2048).collect()
}

fn receive(
    settings: Settings,
    device: Option<DeviceId>,
    stop: &AtomicBool,
    changes: &Mutex<Option<Transform>>,
    quality_request: &Mutex<Option<(u16, u16, u16)>>,
    preview_path: Option<std::path::PathBuf>,
    event: impl FnMut(Event) -> crate::Result<()>,
) -> crate::Result<()> {
    let mut event = event;
    // Fail before opening a transport if its stable identity cannot be loaded.
    let identity = crate::identity::Identity::load_or_create()?;
    let usb_device = if settings.transport == "lan" {
        None
    } else {
        Some(device.ok_or("select a USB device")?)
    };
    let mut transport: Box<dyn Transport> = if settings.transport == "lan" {
        Box::new(SecureTcp::connect(
            &settings.lan_host,
            48527,
            &settings.lan_fingerprint,
            &settings.lan_key,
            Duration::from_secs(5),
        )?)
    } else {
        // `Usb::connect` aligns the stream at a frame boundary; without that, an endpoint
        // still holding a previous connection's frames makes the reader start mid-frame
        // and it can never complete one.
        Box::new(Usb::connect(usb_device.as_ref().expect("checked above"))?)
    };
    let client_id = identity
        .client_id()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let route = if settings.transport == "lan" {
        crate::client_auth::Route::Lan {
            phone_sha256: crate::secure::parse_hex(&settings.lan_fingerprint, 32)?
                .as_slice()
                .try_into()?,
        }
    } else {
        crate::client_auth::Route::Usb
    };
    let mut attempt = |transport: &mut dyn Transport| {
        crate::client_auth::exchange(
            transport,
            &identity,
            route,
            Duration::from_secs(5),
            stop,
            || {
                event(Event::AuthorizationPending {
                    client_id: client_id.clone(),
                })
            },
        )
    };
    let mut outcome = attempt(transport.as_mut());
    if let Err(error) = &outcome
        && usb_device.is_some()
        && crate::usb_probe::a_hello_would_help(&error.to_string())
    {
        // The phone's accessory session is spent (it authorised somebody, and AOA reports no
        // disconnect). Saying hello ends it, so the phone issues a fresh challenge — no app
        // restart, no replug, which is what this used to cost.
        crate::usb_probe::reset_exchange(transport.as_mut(), Duration::from_secs(2))?;
        outcome = attempt(transport.as_mut());
    }
    match outcome {
        Ok(crate::client_auth::Outcome::Authorized { .. }) => {}
        Ok(crate::client_auth::Outcome::Rejected(reason)) => return Err(reason.into()),
        Err(error) => {
            // AOA has no "leave accessory mode" request, so a failed USB attempt leaves the
            // phone rolled into accessory mode with nothing reading it: the app waits for a
            // PC that will never come, and the *next* attempt finds a session that will not
            // start a new exchange. Only a replug (or a USB port reset) clears it, and
            // neither side says so. Measured 2026-10-24; reaching this line with the phone
            // still an accessory is the condition worth naming.
            if let Some(device) = &usb_device
                && crate::usb::switched_identity(device).is_ok()
            {
                return Err(format!(
                    "{error}. The phone is still in USB accessory mode; unplug and replug \
                     the cable before trying again"
                )
                .into());
            }
            return Err(error);
        }
    }
    let output_path = settings.output.trim().to_string();
    let backend =
        crate::output::Backend::parse(&settings.output_kind).ok_or("invalid output backend")?;
    receive_stream(
        settings,
        transport,
        stop,
        Seams {
            changes,
            quality_request,
            preview_path,
        },
        event,
        move |c| crate::output::open(backend, &output_path, c.width, c.height, c.fps),
    )
}
/// The inputs a session reads while it streams, kept together so the seam stays one
/// argument as it grows.
struct Seams<'a> {
    changes: &'a Mutex<Option<Transform>>,
    quality_request: &'a Mutex<Option<(u16, u16, u16)>>,
    preview_path: Option<std::path::PathBuf>,
}

fn receive_stream<S: crate::output::Sink>(
    settings: Settings,
    mut transport: Box<dyn Transport>,
    stop: &AtomicBool,
    seams: Seams,
    mut event: impl FnMut(Event) -> crate::Result<()>,
    mut create_output: impl FnMut(Config) -> crate::Result<S>,
) -> crate::Result<()> {
    let Seams {
        changes,
        quality_request,
        preview_path,
    } = seams;
    // The preview is a property of the stream, not of the sink: it is published at a
    // fixed rate whether or not an application has attached to the virtual camera.
    let mut preview = preview_path.and_then(|path| crate::preview::Producer::open(&path).ok());
    let preview_period = Duration::from_millis(1000 / crate::preview::PREVIEW_HZ.max(1));
    let mut preview_due_at = Instant::now();
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
        if incoming.header.kind == wire::PAIRING_INFO {
            let info = wire::PairingInfo::decode(&incoming.payload)?;
            event(Event::PairingInfo {
                addresses: info
                    .addresses
                    .iter()
                    .map(|address| address.to_string())
                    .collect(),
                fingerprint: info.fingerprint,
                key: info.key,
            })?;
            continue;
        }
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
            let preview_due = preview.is_some() && Instant::now() >= preview_due_at;
            if active || preview_due {
                let (stride, size) = camera.layout();
                let bytes = pixels.convert(&frame, c, stride, size, transform)?;
                if active {
                    camera.submit(bytes, c.width, c.height)?;
                    submitted += 1;
                    report_quality(
                        switch.step(now_ms(), crate::quality::Event::OutputReady),
                        &mut event,
                    )?;
                }
                if preview_due {
                    let shot = crate::preview::from_sink(
                        bytes,
                        c.width as usize,
                        c.height as usize,
                        stride,
                    );
                    if let Some(producer) = &mut preview {
                        // A failed publish must not end a session; the preview is
                        // an affordance, not the product.
                        let _ = producer.publish(&shot);
                    }
                    preview_due_at = Instant::now() + preview_period;
                }
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
    fn the_worker_publishes_a_preview_while_it_streams() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        struct Source {
            frames: std::collections::VecDeque<wire::Frame>,
        }
        impl Transport for Source {
            fn receive(&mut self, timeout: Duration) -> crate::Result<wire::Frame> {
                self.poll_receive(timeout)?.ok_or_else(|| "no frame".into())
            }
            fn send(&mut self, _frame: &wire::Frame, _timeout: Duration) -> crate::Result<()> {
                Ok(())
            }
            fn poll_receive(&mut self, _timeout: Duration) -> crate::Result<Option<wire::Frame>> {
                Ok(self.frames.pop_front())
            }
        }
        struct Sink64 {
            submitted: Arc<Mutex<usize>>,
        }
        impl crate::output::Sink for Sink64 {
            fn layout(&self) -> (usize, usize) {
                let bpp = if cfg!(windows) { 4 } else { 2 };
                (64 * bpp, 64 * 32 * bpp)
            }
            fn ready(&mut self) -> crate::Result<bool> {
                Ok(true)
            }
            fn submit(&mut self, bytes: &[u8], _w: u32, _h: u32) -> crate::Result<()> {
                let bpp = if cfg!(windows) { 4 } else { 2 };
                assert_eq!(bytes.len(), 64 * 32 * bpp);
                *self.submitted.lock().unwrap() += 1;
                Ok(())
            }
        }
        let mut config = [0u8; 16];
        config[..2].copy_from_slice(&64u16.to_le_bytes());
        config[2..4].copy_from_slice(&32u16.to_le_bytes());
        config[4..6].copy_from_slice(&30u16.to_le_bytes());
        config[12] = 1;
        config[13] = 1;
        let mut frames = std::collections::VecDeque::new();
        frames.push_back(wire::Frame::new(wire::VIDEO_CONFIG, 0, config.to_vec()).unwrap());
        let mut access_unit = wire::Frame::new(
            wire::VIDEO_AU,
            0,
            include_bytes!("../tests/fixtures/red-blue.h264").to_vec(),
        )
        .unwrap();
        access_unit.header.flags = wire::KEYFRAME | wire::CONFIG_INCLUDED;
        frames.push_back(access_unit);
        let path = crate::preview::create_path().unwrap();
        let consumer = crate::preview::Consumer::open(&path).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let watcher = stop.clone();
        let watcher_consumer = crate::preview::Consumer::open(&path).unwrap();
        let watchdog = std::thread::spawn(move || {
            for _ in 0..300 {
                if watcher_consumer.latest().is_some() {
                    watcher.store(true, Ordering::Release);
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        });
        let submitted = Arc::new(Mutex::new(0usize));
        let counter = submitted.clone();
        let settings = Settings {
            decode: "off".into(),
            transport: "usb".into(),
            output: "unused".into(),
            ..Settings::default()
        };
        let result = receive_stream(
            settings,
            Box::new(Source { frames }),
            &stop,
            Seams {
                changes: &Mutex::new(None),
                quality_request: &Mutex::new(None),
                preview_path: Some(path.clone()),
            },
            |_| Ok(()),
            move |_| {
                Ok(Sink64 {
                    submitted: counter.clone(),
                })
            },
        );
        let published = watchdog.join().unwrap();
        // Either outcome is reported as the cause, so a failure names itself.
        assert!(
            published,
            "the worker never published a preview: {result:?}"
        );
        assert!(
            *submitted.lock().unwrap() > 0,
            "the sink was never handed a frame"
        );
        let frame = consumer.latest().expect("a preview frame");
        assert_eq!((frame.width, frame.height), (64, 32));
        assert_eq!(frame.pixels.len(), 64 * 32 * 4);
        // The fixture is red over blue. Windows' RGBA conversion presents it the other
        // way up, which the transform test already records, so pick the rows the same
        // way it does: a flat or misread frame still fails on either platform.
        let red_row = if cfg!(windows) { 24 } else { 8 };
        let blue_row = 31 - red_row;
        let red = &frame.pixels[(red_row * 64 + 32) * 4..][..3];
        let blue = &frame.pixels[(blue_row * 64 + 32) * 4..][..3];
        assert!(
            red[0] > 150 && blue[2] > 150,
            "preview pixels are wrong: red row={red:?} blue row={blue:?}"
        );
        std::fs::remove_file(path).ok();
    }

    /// The pipeline's own throughput, on a real 720p H.264 stream.
    ///
    /// Ignored by default because it is a measurement, not an assertion: run
    /// `cargo test --release --features pipewire --lib worker_throughput -- --ignored --nocapture`.
    /// The sink is a counting fake, so this measures decode, swscale and the frame
    /// loop — not the copy into a v4l2 or PipeWire buffer, which needs a device.
    /// Does the worker drift? Streams the same 720p fixture at 30 fps for
    /// `SOAK_SECONDS` (default 120) while the preview record is published, sampling
    /// resident memory and CPU time every 15 s.
    ///
    /// Ignored by default; run
    /// `SOAK_SECONDS=300 cargo test --release --features pipewire --lib worker_soak -- --ignored --nocapture`.
    /// A short soak cannot prove a two-hour claim: it can only show drift per minute,
    /// which is what it prints.
    #[test]
    #[ignore = "measurement: --release --ignored --nocapture"]
    fn worker_soak() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        };
        struct Paced {
            units: Vec<Vec<u8>>,
            deadline: Instant,
            sent: Arc<AtomicU64>,
            stop: Arc<AtomicBool>,
        }
        impl Transport for Paced {
            fn receive(&mut self, timeout: Duration) -> crate::Result<wire::Frame> {
                self.poll_receive(timeout)?.ok_or_else(|| "no frame".into())
            }
            fn send(&mut self, _frame: &wire::Frame, _timeout: Duration) -> crate::Result<()> {
                Ok(())
            }
            fn poll_receive(&mut self, _timeout: Duration) -> crate::Result<Option<wire::Frame>> {
                if Instant::now() >= self.deadline {
                    self.stop.store(true, Ordering::Release);
                    return Ok(None);
                }
                let index = self.sent.load(Ordering::Acquire) as usize;
                let mut au = wire::Frame::new(
                    wire::VIDEO_AU,
                    index as u32,
                    self.units[index % self.units.len()].clone(),
                )?;
                au.header.flags = if index == 0 {
                    wire::KEYFRAME | wire::CONFIG_INCLUDED
                } else if index.is_multiple_of(60) {
                    wire::KEYFRAME
                } else {
                    0
                };
                au.header.pts_us = index as u64 * 33_333;
                self.sent.fetch_add(1, Ordering::AcqRel);
                // Pace at 30 fps: 33 ms per access unit.
                std::thread::sleep(Duration::from_millis(33));
                Ok(Some(au))
            }
        }
        struct NullSink;
        impl crate::output::Sink for NullSink {
            fn layout(&self) -> (usize, usize) {
                let bpp = if cfg!(windows) { 4 } else { 2 };
                (1280 * bpp, 1280 * 720 * bpp)
            }
            fn ready(&mut self) -> crate::Result<bool> {
                Ok(true)
            }
            fn submit(&mut self, _bytes: &[u8], _w: u32, _h: u32) -> crate::Result<()> {
                Ok(())
            }
        }

        let seconds: u64 = std::env::var("SOAK_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        let stream = include_bytes!("../tests/fixtures/testsrc-720p.h264");
        let units: Vec<Vec<u8>> = split_access_units(stream)
            .iter()
            .map(|unit| unit.to_vec())
            .collect();
        assert!(!units.is_empty(), "fixture must hold access units");

        let mut config = [0u8; 16];
        config[..2].copy_from_slice(&1280u16.to_le_bytes());
        config[2..4].copy_from_slice(&720u16.to_le_bytes());
        config[4..6].copy_from_slice(&30u16.to_le_bytes());
        config[12] = 1;
        config[13] = 1;

        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicU64::new(0));
        let preview_path = crate::preview::create_path().ok();
        let transport = Paced {
            units,
            deadline: Instant::now() + Duration::from_secs(seconds),
            sent: sent.clone(),
            stop: stop.clone(),
        };

        // Sampling thread: resident memory and throughput, every 15 s. CPU share is
        // *not* sampled here: the first version read /proc/self/stat and reported 0.0 s
        // for a process that had decoded for minutes, and a broken instrument is worse
        // than none. The per-frame cost comes from `worker_throughput` instead.
        let sampling = {
            let stop = stop.clone();
            let sent = sent.clone();
            let started = Instant::now();
            let mut first: Option<f64> = None;
            std::thread::spawn(move || {
                let rss = || -> f64 {
                    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
                    status
                        .lines()
                        .find(|l| l.starts_with("VmRSS:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                        / 1024.0
                };
                while !stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_secs(15));
                    let rss = rss();
                    let elapsed = started.elapsed().as_secs_f64();
                    let frames = sent.load(Ordering::Acquire);
                    match first {
                        None => {
                            first = Some(rss);
                            println!("t={elapsed:6.1}s frames={frames:6} rss={rss:7.1} MB");
                        }
                        Some(rss0) => println!(
                            "t={elapsed:6.1}s frames={frames:6} rss={rss:7.1} MB ({:+.2} MB/min) {:5.2} fps",
                            (rss - rss0) / (elapsed / 60.0),
                            frames as f64 / elapsed
                        ),
                    }
                }
            })
        };

        let settings = Settings {
            decode: "off".into(),
            transport: "usb".into(),
            output: "unused".into(),
            ..Settings::default()
        };
        let mut frames = std::collections::VecDeque::new();
        frames.push_back(wire::Frame::new(wire::VIDEO_CONFIG, 0, config.to_vec()).unwrap());
        let result = receive_stream(
            settings,
            Box::new(transport),
            &stop,
            Seams {
                changes: &Mutex::new(None),
                quality_request: &Mutex::new(None),
                preview_path: preview_path.clone(),
            },
            |_| Ok(()),
            |_| Ok(NullSink),
        );
        stop.store(true, Ordering::Release);
        let _ = sampling.join();
        if let Some(path) = preview_path {
            std::fs::remove_file(path).ok();
        }
        let served = sent.load(Ordering::Acquire);
        println!("soak done: {seconds} s, {served} access units, result {result:?}");
    }

    /// What the largest accepted `VIDEO_CONFIG` costs the host, measured rather than
    /// reasoned about (Q5 of the 2026-10-23 review: "the host trusts the phone's config
    /// for a large allocation").
    ///
    /// Ignored by default: run
    /// `cargo test --release --lib largest_configuration_footprint -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement: --release --ignored --nocapture"]
    fn largest_configuration_footprint() {
        let rss = || -> f64 {
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            status
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0)
                / 1024.0
        };
        // The largest shape the parser accepts: 8192x4096 = 33,554,432 pixels, its cap.
        let mut bytes = [0u8; 16];
        bytes[..2].copy_from_slice(&8192u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&4096u16.to_le_bytes());
        bytes[4..6].copy_from_slice(&30u16.to_le_bytes());
        bytes[12] = 1;
        bytes[13] = 1;
        let config = Config::parse(&bytes).expect("8192x4096 is the accepted maximum");
        let bpp = if cfg!(windows) { 4 } else { 2 };
        let layout = config.width as usize * bpp * config.height as usize;

        // Touch one byte per page: `vec![0u8; n]` is calloc-backed, so an RSS delta
        // measured without touching reports ~2.6 MiB for a 64 MiB buffer — a lazy
        // allocation that looks like a small one. The real path writes every row, so
        // every page is touched, and this is the number that matches a live stream.
        let before = rss();
        let mut sink = vec![0u8; layout];
        for offset in (0..layout).step_by(4096) {
            sink[offset] = 1;
        }
        let after_sink = rss();
        // The decoder's own frame is I420: 1.5 bytes per pixel, plus swscale output.
        let decoder_estimate = config.width as usize * config.height as usize * 3 / 2;
        println!(
            "config {}x{} ({} px, the accepted maximum)\n  sink buffer {:>6.1} MiB ({} bytes per pixel, every page touched)\n  RSS {:>6.1} -> {:>6.1} MiB (delta {:>5.1})\n  decoder frame, arithmetic not measured: {:.1} MiB",
            config.width,
            config.height,
            config.width as u64 * config.height as u64,
            layout as f64 / (1024.0 * 1024.0),
            bpp,
            before,
            after_sink,
            after_sink - before,
            decoder_estimate as f64 / (1024.0 * 1024.0),
        );
        std::hint::black_box(&sink);
    }

    /// Split an Annex-B stream into access units at its AUD NALs (type 9).
    fn split_access_units(stream: &[u8]) -> Vec<&[u8]> {
        let mut starts = Vec::new();
        let mut index = 0;
        while index + 3 < stream.len() {
            if stream[index] == 0 && stream[index + 1] == 0 && stream[index + 2] == 1 {
                let header = index + 3;
                if header < stream.len() && stream[header] & 0x1f == 9 {
                    starts.push(index);
                }
                index = header;
            } else {
                index += 1;
            }
        }
        starts
            .iter()
            .enumerate()
            .map(|(i, &start)| {
                let end = starts.get(i + 1).copied().unwrap_or(stream.len());
                &stream[start..end]
            })
            .collect()
    }

    fn contains_idr(unit: &[u8]) -> bool {
        let mut index = 0;
        while index + 3 < unit.len() {
            if unit[index] == 0 && unit[index + 1] == 0 && unit[index + 2] == 1 {
                if index + 3 < unit.len() && unit[index + 3] & 0x1f == 5 {
                    return true;
                }
                index += 3;
            } else {
                index += 1;
            }
        }
        false
    }

    #[test]
    #[ignore = "measurement: --release --ignored --nocapture"]
    fn worker_throughput() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        struct Source {
            frames: std::collections::VecDeque<wire::Frame>,
        }
        impl Transport for Source {
            fn receive(&mut self, timeout: Duration) -> crate::Result<wire::Frame> {
                self.poll_receive(timeout)?.ok_or_else(|| "no frame".into())
            }
            fn send(&mut self, _frame: &wire::Frame, _timeout: Duration) -> crate::Result<()> {
                Ok(())
            }
            fn poll_receive(&mut self, _timeout: Duration) -> crate::Result<Option<wire::Frame>> {
                Ok(self.frames.pop_front())
            }
        }
        struct CountingSink {
            submitted: Arc<Mutex<u64>>,
        }
        impl crate::output::Sink for CountingSink {
            fn layout(&self) -> (usize, usize) {
                let bpp = if cfg!(windows) { 4 } else { 2 };
                (1280 * bpp, 1280 * 720 * bpp)
            }
            fn ready(&mut self) -> crate::Result<bool> {
                Ok(true)
            }
            fn submit(&mut self, _bytes: &[u8], _w: u32, _h: u32) -> crate::Result<()> {
                *self.submitted.lock().unwrap() += 1;
                Ok(())
            }
        }

        let stream = include_bytes!("../tests/fixtures/testsrc-720p.h264");
        let mut config = [0u8; 16];
        config[..2].copy_from_slice(&1280u16.to_le_bytes());
        config[2..4].copy_from_slice(&720u16.to_le_bytes());
        config[4..6].copy_from_slice(&30u16.to_le_bytes());
        config[12] = 1;
        config[13] = 1;

        // 1. Decode alone.
        // One access unit per frame: the fixture was encoded with an AUD NAL before
        // each, so the decoder is fed the shape the phone sends instead of one
        // 90-frame packet (which it decodes as a single picture).
        let access_units = split_access_units(stream);
        assert!(
            access_units.len() > 30,
            "fixture should hold the recorded frames"
        );

        // 1. Decode alone.
        let mut decoder = decoder::Decoder::open("off").unwrap();
        let mut frame = ffmpeg::Frame::empty().unwrap();
        let decode_started = Instant::now();
        let mut decoded = 0u64;
        for (index, unit) in access_units.iter().enumerate() {
            decoder
                .send(unit, index as u64 * 33_333, index == 0)
                .unwrap();
            while decoder.receive(&mut frame).unwrap() {
                decoded += 1;
            }
        }
        let decode = decode_started.elapsed();

        // 2. Swscale alone, on a frame of its own: the frame the loop above leaves
        // behind is not guaranteed to still be valid afterwards.
        let c = Config {
            width: 1280,
            height: 720,
            fps: 30,
        };
        let bpp = if cfg!(windows) { 4 } else { 2 };
        let mut decoder = decoder::Decoder::open("off").unwrap();
        let mut frame = ffmpeg::Frame::empty().unwrap();
        decoder.send(access_units[0], 1_000, true).unwrap();
        assert!(
            decoder.receive(&mut frame).unwrap(),
            "the fixture must decode"
        );
        let mut pixels = pixels::Pixels::default();
        let convert_started = Instant::now();
        for _ in 0..decoded.max(1) {
            let bytes = pixels
                .convert(
                    &frame,
                    c,
                    1280 * bpp,
                    1280 * 720 * bpp,
                    Transform {
                        rotation: 0,
                        mirror: false,
                        vertical: false,
                    },
                )
                .unwrap();
            std::hint::black_box(bytes);
        }
        let convert = convert_started.elapsed();

        // 3. The whole loop, ending when the fixture's frames have all been submitted.
        let stop = Arc::new(AtomicBool::new(false));
        let submitted = Arc::new(Mutex::new(0u64));
        let watcher = stop.clone();
        let counter = submitted.clone();
        let watchdog = std::thread::spawn(move || {
            for _ in 0..2000 {
                if *counter.lock().unwrap() >= decoded.max(1) {
                    watcher.store(true, Ordering::Release);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let mut frames = std::collections::VecDeque::new();
        frames.push_back(wire::Frame::new(wire::VIDEO_CONFIG, 0, config.to_vec()).unwrap());
        for (index, unit) in access_units.iter().enumerate() {
            let mut au = wire::Frame::new(wire::VIDEO_AU, index as u32, unit.to_vec()).unwrap();
            au.header.flags = if index == 0 {
                wire::KEYFRAME | wire::CONFIG_INCLUDED
            } else if contains_idr(unit) {
                wire::KEYFRAME
            } else {
                0
            };
            au.header.pts_us = index as u64 * 33_333;
            frames.push_back(au);
        }
        let counting = submitted.clone();
        let settings = Settings {
            decode: "off".into(),
            transport: "usb".into(),
            output: "unused".into(),
            ..Settings::default()
        };
        let pipeline_started = Instant::now();
        let _ = receive_stream(
            settings,
            Box::new(Source { frames }),
            &stop,
            Seams {
                changes: &Mutex::new(None),
                quality_request: &Mutex::new(None),
                preview_path: None,
            },
            |_| Ok(()),
            move |_| {
                Ok(CountingSink {
                    submitted: counting.clone(),
                })
            },
        );
        let pipeline = pipeline_started.elapsed();
        watchdog.join().ok();

        let frames = decoded.max(1) as f64;
        let ms = |d: Duration| d.as_secs_f64() * 1000.0 / frames;
        println!(
            "frames {decoded}  decode {:.2} ms/frame  convert {:.2} ms/frame  pipeline {:.2} ms/frame  ({:.1} fps)",
            ms(decode),
            ms(convert),
            ms(pipeline),
            frames / pipeline.as_secs_f64(),
        );
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
