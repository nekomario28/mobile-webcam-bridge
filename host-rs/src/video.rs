mod decoder;
mod ffmpeg;
mod pixels;
pub mod transform;
use crate::{
    control::{self, Command, Event},
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
    let thread_stop = stop.clone();
    let thread_changes = changes.clone();
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
                _ => {
                    thread_stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
    });
    let mut stdout = io::stdout().lock();
    control::write(&mut stdout, &Event::Connecting {})?;
    let result = receive(settings, device, &stop, &changes, |event| {
        control::write(&mut stdout, &event).map_err(Into::into)
    });
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
    mut event: impl FnMut(Event) -> crate::Result<()>,
) -> crate::Result<()> {
    let mut decoder = decoder::Decoder::open(&settings.decode)?;
    let mut transport: Box<dyn Transport> = if settings.transport == "lan" {
        Box::new(Tcp::connect(
            &settings.lan_host,
            48527,
            Duration::from_secs(5),
        )?)
    } else {
        Box::new(Usb::connect(&device.ok_or("select a USB device")?)?)
    };
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
        let incoming = transport.receive(Duration::from_secs(120))?;
        if let Some(next) = changes.lock().unwrap().take() {
            transform = next;
        }
        if incoming.header.kind == wire::VIDEO_CONFIG {
            let next = Config::parse(&incoming.payload)?;
            if config.is_some_and(|c| c != next) {
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
                output = Some(crate::output::Camera::open(
                    settings.output.trim(),
                    c.width,
                    c.height,
                    c.fps,
                )?);
            }
            let camera = output.as_mut().unwrap();
            let active = camera.ready()?;
            if active {
                let bytes = pixels.convert(&frame, c, camera.stride, camera.size, transform)?;
                camera.submit(bytes, c.width, c.height)?;
                submitted += 1;
            }
            if report.elapsed() >= Duration::from_secs(1) {
                event(Event::Stats {
                    decoded,
                    submitted,
                    active,
                    width: c.width,
                    height: c.height,
                    decoder: decoder.active().into(),
                })?;
                report = Instant::now();
            }
        }
    }
    Ok(())
}

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
