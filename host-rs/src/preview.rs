//! The PC's own preview: read back what this app is writing to the virtual camera.
//!
//! The GUI runs in its own process, and the control channel is JSON lines capped at
//! 16 KiB, so frames cannot come from the worker without a second protocol. Reading
//! the output device instead needs no protocol at all: v4l2loopback serves consumers
//! what the producer wrote, and this app is both. The read is deliberately
//! format-passive — it never calls `S_FMT`, because a consumer that renegotiates the
//! loopback's format changes what every other app sees.
//!
//! The preview is for framing ("is the phone pointing at me?"), so frames are
//! downscaled to at most [`PREVIEW_WIDTH`] columns with nearest-neighbour sampling.

/// Widest preview kept for the UI; the source is usually 1280 or 1920 wide.
pub const PREVIEW_WIDTH: usize = 320;

/// One downscaled frame, RGBA8, tightly packed.
#[derive(Debug)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

// The reader is V4L2, which only exists with the `video` feature: a GUI-only build
// has no bindings to include and no device to read, so it gets the stub.
#[cfg(all(target_os = "linux", feature = "video"))]
pub use linux::Reader;
#[cfg(not(all(target_os = "linux", feature = "video")))]
pub use stub::Reader;

#[cfg(not(all(target_os = "linux", feature = "video")))]
mod stub {
    /// Windows reads its virtual camera through DirectShow and has no compat path
    /// for this app's own output, so the preview is unavailable rather than absent.
    pub struct Reader;
    impl Reader {
        pub fn open(_path: &str) -> std::io::Result<Self> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "preview reads the V4L2 loopback device only",
            ))
        }
        pub fn read(&mut self) -> Option<super::Frame> {
            None
        }
    }
}

/// Limited-range BT.709 luma/chroma to RGB, the range the sink advertises.
fn yuv_to_rgb(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let (y, u, v) = (
        (y as f32 - 16.0) * 1.164_383,
        u as f32 - 128.0,
        v as f32 - 128.0,
    );
    let clamp = |value: f32| value.round().clamp(0.0, 255.0) as u8;
    (
        clamp(y + 1.792_741 * v),
        clamp(y - 0.213_249 * u - 0.532_909 * v),
        clamp(y + 2.112_402 * u),
    )
}

/// YUYV (one luma pair per two pixels) to RGBA, sampled at `step` columns and rows.
///
/// `stride` is the driver's `bytesperline`, which may exceed `width * 2`; padding is
/// skipped rather than sheared into the picture.
pub fn yuyv_to_rgba(
    src: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    step: usize,
) -> (usize, usize, Vec<u8>) {
    let step = step.max(1);
    let (out_width, out_height) = (width / step, height / step);
    let mut out = vec![0u8; out_width * out_height * 4];
    for oy in 0..out_height {
        // Sample the middle row and column of each block. YUYV packs two pixels per
        // four bytes sharing one chroma pair, so the luma of an odd column lives at
        // byte 2 of the same group — reading byte 0 would give it its neighbour's.
        let y = oy * step + step / 2;
        for ox in 0..out_width {
            let x = ox * step + step / 2;
            let index = y.saturating_mul(stride) + x / 2 * 4;
            let (luma, u, v) = match src.get(index..index + 4) {
                Some(pixel) => (
                    pixel[if x.is_multiple_of(2) { 0 } else { 2 }],
                    pixel[1],
                    pixel[3],
                ),
                None => (16, 128, 128),
            };
            let (r, g, b) = yuv_to_rgb(luma, u, v);
            let out_index = (oy * out_width + ox) * 4;
            out[out_index] = r;
            out[out_index + 1] = g;
            out[out_index + 2] = b;
            out[out_index + 3] = 255;
        }
    }
    (out_width, out_height, out)
}

/// The sampling step that keeps a frame at or under [`PREVIEW_WIDTH`] columns.
pub fn step_for(width: usize) -> usize {
    if width <= PREVIEW_WIDTH {
        1
    } else {
        width.div_ceil(PREVIEW_WIDTH)
    }
}

#[cfg(all(target_os = "linux", feature = "video"))]
mod linux {
    use std::{fs::File, io::Read, os::fd::AsRawFd, os::unix::fs::OpenOptionsExt};

    #[allow(
        non_upper_case_globals,
        non_camel_case_types,
        non_snake_case,
        dead_code
    )]
    mod abi {
        include!(concat!(env!("OUT_DIR"), "/v4l2.rs"));
    }
    use abi::*;

    /// A capture-side view of the loopback device.
    pub struct Reader {
        file: File,
        width: usize,
        height: usize,
        stride: usize,
        buffer: Vec<u8>,
        /// The capture fixture replays a synthetic frame instead of a device.
        #[cfg(feature = "capture")]
        replay: Option<Vec<u8>>,
    }

    fn ioctl<T>(file: &File, request: u64, value: &mut T) -> std::io::Result<i32> {
        loop {
            // The generated UAPI type and request code bind this pointer to the
            // kernel ABI, exactly as the output path does.
            let result =
                unsafe { libc::ioctl(file.as_raw_fd(), request as libc::c_ulong, &mut *value) };
            if result >= 0 {
                return Ok(result);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    impl Reader {
        /// Open the output device for reading without changing its format.
        ///
        /// With `exclusive_caps=1` the device only advertises capture while a
        /// producer is streaming, so callers are expected to retry.
        pub fn open(path: &str) -> std::io::Result<Self> {
            #[cfg(feature = "capture")]
            if let Some(replay) = capture_replay() {
                return Ok(replay);
            }
            // Read-only and non-blocking: no frame yet must never stall the GUI, and
            // the descriptor must not be able to renegotiate the format.
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)?;
            let mut capability = v4l2_capability::default();
            ioctl(&file, AMB_QUERYCAP, &mut capability)?;
            let caps = if capability.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
                capability.device_caps
            } else {
                capability.capabilities
            };
            if caps & V4L2_CAP_VIDEO_CAPTURE == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "device does not expose capture",
                ));
            }
            let mut format = v4l2_format {
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                ..Default::default()
            };
            ioctl(&file, AMB_G_FMT, &mut format)?;
            let pix = unsafe { format.fmt.pix };
            if pix.pixelformat != AMB_YUYV || pix.width == 0 || pix.height == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "preview expects the YUYV format this app writes",
                ));
            }
            let width = pix.width as usize;
            let height = pix.height as usize;
            let stride = (pix.bytesperline as usize).max(width * 2);
            let size = (pix.sizeimage as usize).max(stride * height);
            Ok(Self {
                file,
                width,
                height,
                stride,
                buffer: vec![0u8; size],
                #[cfg(feature = "capture")]
                replay: None,
            })
        }

        /// The next frame, or `None` while the producer has not written one yet.
        pub fn read(&mut self) -> Option<super::Frame> {
            #[cfg(feature = "capture")]
            if self.replay.is_some() {
                // The fixture frame is served fresh each poll so the UI keeps its
                // texture without needing a device.
                return self.decode();
            }
            match self.file.read(&mut self.buffer) {
                Ok(0) | Err(_) => None,
                Ok(_) => self.decode(),
            }
        }

        fn decode(&mut self) -> Option<super::Frame> {
            let (width, height, pixels) = super::yuyv_to_rgba(
                &self.buffer,
                self.width,
                self.height,
                self.stride,
                super::step_for(self.width),
            );
            if pixels.is_empty() {
                return None;
            }
            Some(super::Frame {
                width,
                height,
                pixels,
            })
        }
    }

    #[cfg(feature = "capture")]
    fn capture_replay() -> Option<Reader> {
        use std::io::Read as _;
        let path = std::env::var("MW_PREVIEW_YUYV").ok()?;
        let size = std::env::var("MW_PREVIEW_SIZE").ok()?;
        let (width, height) = size.split_once('x')?;
        let (width, height): (usize, usize) = (width.parse().ok()?, height.parse().ok()?);
        let mut raw = Vec::new();
        std::fs::File::open(path).ok()?.read_to_end(&mut raw).ok()?;
        if raw.len() < width * height * 2 {
            return None;
        }
        Some(Reader {
            file: std::fs::File::open("/dev/null").ok()?,
            width,
            height,
            stride: width * 2,
            buffer: raw,
            replay: Some(Vec::new()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grey_stays_grey_through_the_bt709_matrix() {
        // Limited-range mid grey: Y=126, neutral chroma.
        let (r, g, b) = yuv_to_rgb(126, 128, 128);
        assert_eq!((r, g, b), (128, 128, 128));
    }

    #[test]
    fn black_and_white_sit_at_the_ends_of_the_range() {
        assert_eq!(yuv_to_rgb(16, 128, 128), (0, 0, 0));
        assert_eq!(yuv_to_rgb(235, 128, 128), (255, 255, 255));
    }

    #[test]
    fn a_red_frame_decodes_to_red() {
        // BT.709 limited red: Y=63, U=102, V=240.
        let (r, g, b) = yuv_to_rgb(63, 102, 240);
        assert!(r > 200, "r={r}");
        assert!(g < 40, "g={g}");
        assert!(b < 40, "b={b}");
    }

    #[test]
    fn yuyv_pairs_share_chroma_and_keep_their_own_luma() {
        // Two pixels: dark then bright, one shared chroma pair (neutral).
        let src = [32u8, 128, 224, 128];
        let (width, height, pixels) = yuyv_to_rgba(&src, 2, 1, 4, 1);
        assert_eq!((width, height), (2, 1));
        assert!(pixels[0] < 40, "first pixel should be dark: {pixels:?}");
        assert!(pixels[4] > 200, "second pixel should be bright: {pixels:?}");
        assert_eq!(pixels[3], 255, "alpha is opaque");
    }

    #[test]
    fn padding_in_the_stride_is_skipped_not_sheared() {
        let packed = [32u8, 128, 224, 128];
        let mut padded = vec![0u8; 8];
        padded[..4].copy_from_slice(&packed);
        let a = yuyv_to_rgba(&packed, 2, 1, 4, 1);
        let b = yuyv_to_rgba(&padded, 2, 1, 8, 1);
        assert_eq!(a.2, b.2);
    }

    #[test]
    fn a_short_frame_reads_as_black_instead_of_panicking() {
        let (width, height, pixels) = yuyv_to_rgba(&[], 2, 1, 4, 1);
        assert_eq!((width, height), (2, 1));
        assert_eq!(&pixels[..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn the_step_keeps_the_preview_at_or_below_the_target_width() {
        assert_eq!(step_for(320), 1);
        assert_eq!(step_for(640), 2);
        assert_eq!(step_for(1280), 4);
        assert_eq!(step_for(1920), 6);
        assert!(1280 / step_for(1280) <= PREVIEW_WIDTH);
        assert!(1920 / step_for(1920) <= PREVIEW_WIDTH);
    }

    /// End-to-end against a live loopback, when one is available.
    ///
    /// Run with `AMB_PREVIEW_DEVICE=/dev/video10` while something feeds that device
    /// (`ffmpeg -f lavfi -i testsrc ... -f v4l2 /dev/video10`). Without the variable
    /// it skips, because a CI runner has no v4l2loopback.
    #[test]
    fn live_preview_reads_the_loopback() {
        let Ok(device) = std::env::var("AMB_PREVIEW_DEVICE") else {
            return;
        };
        let mut reader = Reader::open(&device).expect("open the loopback for capture");
        let mut frame = None;
        for _ in 0..100 {
            if let Some(next) = reader.read() {
                frame = Some(next);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let frame = frame.expect("a live producer should deliver a frame within 5s");
        assert!(frame.pixels.len() == frame.width * frame.height * 4);
        assert!(frame.width <= PREVIEW_WIDTH);
        // A flat synthetic frame would still pass the size checks; require some
        // variation so a blank read is not mistaken for a working preview.
        let first = frame.pixels.chunks(4).next().unwrap().to_vec();
        let varies = frame
            .pixels
            .chunks(4)
            .any(|pixel| pixel != first.as_slice());
        assert!(varies, "preview frame is a single flat colour");
    }

    #[test]
    fn a_1280_wide_frame_comes_back_320_wide() {
        let src = vec![128u8; 1280 * 720 * 2];
        let (width, height, pixels) = yuyv_to_rgba(&src, 1280, 720, 2560, step_for(1280));
        assert_eq!((width, height), (320, 180));
        assert_eq!(pixels.len(), 320 * 180 * 4);
    }
}
