//! The PC's own preview: a small, fixed-rate copy of what the sink is being fed.
//!
//! The GUI runs in its own process and the control channel is a JSON line capped at
//! 16 KiB, so frames cannot ride it. They travel through a small file instead — a
//! seqlock record the worker publishes at a fixed rate and the GUI reads at
//! whatever rate it repaints. That keeps the preview independent of GUI polling,
//! works on every platform (no device-specific read-back), and still shows the
//! picture when no application has attached to the virtual camera yet.
//!
//! The record is written by one process and read by another, so the reader can
//! catch a half-written frame; a mismatched sequence number means "keep the frame
//! you have", never a torn image.

use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

/// Where the worker is told to publish; set by the GUI when it spawns it.
pub const PATH_ENV: &str = "AMB_PREVIEW_PATH";

/// Widest preview kept for the UI; the source is usually 1280 or 1920 wide.
pub const PREVIEW_WIDTH: usize = 320;
/// How often the worker publishes, independent of the frame rate and the GUI.
pub const PREVIEW_HZ: u64 = 10;

const MAGIC: [u8; 4] = *b"MBWP";
const VERSION: u32 = 1;
const HEADER: usize = 32;
/// Room for 320x1024 RGBA — far beyond any preview shape, small on disk.
const CAPACITY: usize = PREVIEW_WIDTH * 1024 * 4;

/// One downscaled frame, RGBA8, tightly packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// Create a fresh private record file and return its path.
///
/// `create_new` refuses any existing path, so a pre-planted symlink or file in the
/// temp directory cannot redirect the preview.
pub fn create_path() -> io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "mobile-webcam-preview-{}-{nanos}.bin",
        std::process::id()
    ));
    let file = private_create(&path)?;
    file.set_len((HEADER + CAPACITY) as u64)?;
    Ok(path)
}

#[cfg(unix)]
fn private_create(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_create(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn write_at(file: &File, offset: u64, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(bytes, offset)
}

#[cfg(unix)]
fn read_at(file: &File, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(bytes, offset)
}

#[cfg(windows)]
fn write_at(file: &File, offset: u64, bytes: &[u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < bytes.len() {
        done += file.seek_write(&bytes[done..], offset + done as u64)?;
    }
    Ok(())
}

#[cfg(windows)]
fn read_at(file: &File, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < bytes.len() {
        let read = file.seek_read(&mut bytes[done..], offset + done as u64)?;
        if read == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        done += read;
    }
    Ok(())
}

/// A sequence number that is odd while a write is in progress.
fn header(seq: u64, width: usize, height: usize, len: usize) -> [u8; HEADER] {
    let mut bytes = [0u8; HEADER];
    bytes[..4].copy_from_slice(&MAGIC);
    bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
    bytes[8..16].copy_from_slice(&seq.to_le_bytes());
    bytes[16..20].copy_from_slice(&(width as u32).to_le_bytes());
    bytes[20..24].copy_from_slice(&(height as u32).to_le_bytes());
    bytes[24..28].copy_from_slice(&(len as u32).to_le_bytes());
    bytes
}

struct Parsed {
    seq: u64,
    width: usize,
    height: usize,
    len: usize,
}

fn parse_header(bytes: &[u8; HEADER]) -> Option<Parsed> {
    if bytes[..4] != MAGIC || u32::from_le_bytes(bytes[4..8].try_into().ok()?) != VERSION {
        return None;
    }
    let seq = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let width = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    let height = u32::from_le_bytes(bytes[20..24].try_into().ok()?) as usize;
    let len = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as usize;
    if width == 0 || height == 0 || len != width * height * 4 || len > CAPACITY {
        return None;
    }
    Some(Parsed {
        seq,
        width,
        height,
        len,
    })
}

/// The worker's end: publish the newest frame for the GUI to read.
pub struct Producer {
    file: File,
    seq: u64,
}

impl Producer {
    /// Open (or create) the record the GUI asked for.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = match private_create(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // The GUI created it first; it is ours by construction.
                OpenOptions::new().read(true).write(true).open(path)?
            }
            Err(error) => return Err(error),
        };
        let body = file.metadata()?.len();
        if body < (HEADER + CAPACITY) as u64 {
            file.set_len((HEADER + CAPACITY) as u64)?;
        }
        Ok(Self { file, seq: 0 })
    }

    /// Write `frame` so that a reader sees either this one or the previous one.
    pub fn publish(&mut self, frame: &Frame) -> io::Result<()> {
        if frame.pixels.len() != frame.width * frame.height * 4 || frame.pixels.len() > CAPACITY {
            return Ok(());
        }
        self.seq = self.seq.wrapping_add(1) | 1;
        write_at(
            &self.file,
            0,
            &header(self.seq, frame.width, frame.height, frame.pixels.len()),
        )?;
        write_at(&self.file, HEADER as u64, &frame.pixels)?;
        self.seq = self.seq.wrapping_add(1) & !1;
        write_at(&self.file, 8, &self.seq.to_le_bytes())
    }
}

/// The GUI's end: the newest complete frame, or `None` while one is being written.
pub struct Consumer {
    file: File,
}

impl Consumer {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: OpenOptions::new().read(true).open(path)?,
        })
    }

    pub fn latest(&self) -> Option<Frame> {
        let mut raw = [0u8; HEADER];
        read_at(&self.file, 0, &mut raw).ok()?;
        let parsed = parse_header(&raw)?;
        if parsed.seq % 2 == 1 {
            return None;
        }
        let mut pixels = vec![0u8; parsed.len];
        read_at(&self.file, HEADER as u64, &mut pixels).ok()?;
        // The writer may have started again while this frame was being copied;
        // a changed sequence number means the copy may be torn.
        let mut check = [0u8; HEADER];
        read_at(&self.file, 0, &mut check).ok()?;
        if parse_header(&check)?.seq != parsed.seq {
            return None;
        }
        Some(Frame {
            width: parsed.width,
            height: parsed.height,
            pixels,
        })
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

/// RGBA (the Windows sink's layout) to RGBA, sampled at `step` columns and rows.
pub fn rgba_to_rgba(
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
        let y = oy * step + step / 2;
        for ox in 0..out_width {
            let x = ox * step + step / 2;
            let index = y.saturating_mul(stride) + x * 4;
            let out_index = (oy * out_width + ox) * 4;
            match src.get(index..index + 4) {
                Some(pixel) => out[out_index..out_index + 4].copy_from_slice(pixel),
                None => out[out_index..out_index + 4].copy_from_slice(&[0, 0, 0, 255]),
            }
        }
    }
    (out_width, out_height, out)
}

/// Downscale whatever the sink was just handed, in the sink's own layout.
pub fn from_sink(bytes: &[u8], width: usize, height: usize, stride: usize) -> Frame {
    let step = step_for(width);
    let (width, height, pixels) = if cfg!(windows) {
        rgba_to_rgba(bytes, width, height, stride, step)
    } else {
        yuyv_to_rgba(bytes, width, height, stride, step)
    };
    Frame {
        width,
        height,
        pixels,
    }
}

/// The sampling step that keeps a frame at or under [`PREVIEW_WIDTH`] columns.
pub fn step_for(width: usize) -> usize {
    if width <= PREVIEW_WIDTH {
        1
    } else {
        width.div_ceil(PREVIEW_WIDTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize, value: u8) -> Frame {
        Frame {
            width,
            height,
            pixels: vec![value; width * height * 4],
        }
    }

    fn record() -> (PathBuf, Producer, Consumer) {
        let path = create_path().unwrap();
        let producer = Producer::open(&path).unwrap();
        let consumer = Consumer::open(&path).unwrap();
        (path, producer, consumer)
    }

    #[test]
    fn a_published_frame_is_read_back_whole() {
        let (path, mut producer, consumer) = record();
        producer.publish(&frame(4, 2, 7)).unwrap();
        let read = consumer.latest().expect("a complete frame");
        assert_eq!((read.width, read.height), (4, 2));
        assert!(read.pixels.iter().all(|value| *value == 7));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_write_in_progress_is_never_read_as_a_frame() {
        let (path, _producer, consumer) = record();
        // Exactly what a crashed writer leaves behind: an odd sequence number.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        write_at(&file, 0, &header(7, 4, 2, 32)).unwrap();
        assert!(consumer.latest().is_none());
        // And an even one is readable again.
        write_at(&file, 0, &header(8, 4, 2, 32)).unwrap();
        assert!(consumer.latest().is_some());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_record_that_disagrees_with_itself_is_rejected() {
        let (path, _producer, consumer) = record();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        // Sequence says even, payload length says a frame that cannot exist.
        write_at(&file, 0, &header(2, 4, 2, 999)).unwrap();
        assert!(consumer.latest().is_none());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn the_latest_frame_wins() {
        let (path, mut producer, consumer) = record();
        for value in 1..=3u8 {
            producer.publish(&frame(4, 2, value)).unwrap();
        }
        assert!(consumer.latest().unwrap().pixels.iter().all(|v| *v == 3));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn grey_stays_grey_through_the_bt709_matrix() {
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
        let (r, g, b) = yuv_to_rgb(63, 102, 240);
        assert!(r > 200, "r={r}");
        assert!(g < 40, "g={g}");
        assert!(b < 40, "b={b}");
    }

    #[test]
    fn yuyv_pairs_share_chroma_and_keep_their_own_luma() {
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
        assert_eq!(
            yuyv_to_rgba(&packed, 2, 1, 4, 1).2,
            yuyv_to_rgba(&padded, 2, 1, 8, 1).2
        );
    }

    #[test]
    fn a_short_frame_reads_as_black_instead_of_panicking() {
        let (width, height, pixels) = yuyv_to_rgba(&[], 2, 1, 4, 1);
        assert_eq!((width, height), (2, 1));
        assert_eq!(&pixels[..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn rgba_samples_keep_their_colour_and_alpha() {
        let src = [10u8, 20, 30, 255, 40, 50, 60, 255];
        let (width, height, pixels) = rgba_to_rgba(&src, 2, 1, 8, 1);
        assert_eq!((width, height), (2, 1));
        assert_eq!(&pixels[..4], &[10, 20, 30, 255]);
        assert_eq!(&pixels[4..], &[40, 50, 60, 255]);
    }

    #[test]
    fn the_step_keeps_the_preview_at_or_below_the_target_width() {
        assert_eq!(step_for(320), 1);
        assert_eq!(step_for(640), 2);
        assert_eq!(step_for(1280), 4);
        assert_eq!(step_for(1920), 6);
        assert!(1920 / step_for(1920) <= PREVIEW_WIDTH);
    }

    #[test]
    fn a_1280_wide_frame_comes_back_320_wide() {
        let src = vec![128u8; 1280 * 720 * 2];
        let (width, height, pixels) = yuyv_to_rgba(&src, 1280, 720, 2560, step_for(1280));
        assert_eq!((width, height), (320, 180));
        assert_eq!(pixels.len(), 320 * 180 * 4);
    }

    #[test]
    fn the_record_refuses_a_frame_larger_than_it_can_hold() {
        let (path, mut producer, consumer) = record();
        let too_wide = Frame {
            width: 4096,
            height: 1024,
            pixels: vec![0; 4096 * 1024 * 4],
        };
        producer.publish(&too_wide).unwrap();
        assert!(consumer.latest().is_none(), "an oversized frame is dropped");
        std::fs::remove_file(path).ok();
    }
}
