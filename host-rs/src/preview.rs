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
    path::Path,
};

mod posio;

pub use posio::create_path;
use posio::{read_at, write_at};

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
        let file = match posio::private_create(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // The GUI created it first. "By construction" is not a check: this is the
                // path that normally runs, so it refuses anything that is not our own
                // regular file — no symlink, no other owner, no group/other bits.
                posio::private_reopen(path)?
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

    fn record() -> (std::path::PathBuf, Producer, Consumer) {
        let path = create_path().unwrap();
        let producer = Producer::open(&path).unwrap();
        let consumer = Consumer::open(&path).unwrap();
        (path, producer, consumer)
    }

    /// The path that normally runs is GUI-creates-then-worker-opens, and the first
    /// version of `Producer::open` re-opened that file with a plain `open`, which follows
    /// a symlink. The 2026-10-23 review called this out as a question (Q1) because the
    /// tests only ever exercised that fallback without asserting anything about it.
    #[cfg(unix)]
    #[test]
    fn a_record_replaced_by_a_symlink_is_refused() {
        let dir = std::env::temp_dir().join(format!("mwb-symlink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let record = dir.join("record.bin");
        let elsewhere = dir.join("elsewhere.bin");
        std::fs::write(&elsewhere, b"not a preview record").unwrap();
        // The GUI creates the record; the substitution happens before the worker opens.
        drop(create_path_in(&record).unwrap());
        std::fs::remove_file(&record).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &record).unwrap();

        let refused = Producer::open(&record);
        assert!(refused.is_err(), "a symlinked record must not be opened");
        // The target was not written through either.
        let untouched = std::fs::read(&elsewhere).unwrap();
        assert_eq!(&untouched, b"not a preview record");

        // And a record whose permissions widened is refused as well.
        std::fs::remove_file(&record).unwrap();
        drop(create_path_in(&record).unwrap());
        let mut permissions = std::fs::metadata(&record).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o644);
        std::fs::set_permissions(&record, permissions).unwrap();
        assert!(
            Producer::open(&record).is_err(),
            "a world-readable record must be refused"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `create_path` with a caller-chosen name, so a test can substitute the file.
    #[cfg(unix)]
    fn create_path_in(path: &Path) -> io::Result<File> {
        let file = posio::private_create(path)?;
        file.set_len((HEADER + CAPACITY) as u64)?;
        Ok(file)
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
    fn a_header_that_lies_about_its_shape_is_rejected() {
        let (path, _producer, consumer) = record();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        // Right version, wrong magic: only the magic check is exercised here.
        let mut wrong_magic = header(2, 4, 2, 32);
        wrong_magic[0] = b'X';
        write_at(&file, 0, &wrong_magic).unwrap();
        assert!(consumer.latest().is_none(), "wrong magic must be refused");
        // Zero width and zero height are not shapes, even with a matching length.
        write_at(&file, 0, &header(2, 0, 2, 0)).unwrap();
        assert!(consumer.latest().is_none(), "zero width must be refused");
        write_at(&file, 0, &header(2, 4, 0, 0)).unwrap();
        assert!(consumer.latest().is_none(), "zero height must be refused");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn the_record_accepts_a_frame_at_exactly_its_capacity() {
        // 640x512 is the largest shape CAPACITY holds; the boundary must be
        // inclusive or a legal frame would be dropped.
        let (path, mut producer, consumer) = record();
        let largest = Frame {
            width: 640,
            height: 512,
            pixels: vec![9; 640 * 512 * 4],
        };
        assert_eq!(largest.pixels.len(), super::CAPACITY);
        producer.publish(&largest).unwrap();
        let read = consumer.latest().expect("a frame at capacity is legal");
        assert_eq!((read.width, read.height), (640, 512));
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
    fn the_conversion_matches_the_published_coefficients() {
        // Exact values for BT.709 limited-range, so a flipped sign, a changed
        // constant or a swapped term in any of the three lines moves a channel.
        // Measured: loose bounds (g < 40) let several such mutants survive.
        assert_eq!(yuv_to_rgb(81, 90, 240), (255, 24, 0));
        assert_eq!(yuv_to_rgb(145, 54, 34), (0, 216, 0));
        assert_eq!(yuv_to_rgb(41, 240, 110), (0, 15, 255));
        assert_eq!(yuv_to_rgb(100, 138, 120), (83, 100, 119));
        assert_eq!(yuv_to_rgb(200, 140, 150), (254, 200, 240));
        // Same luma, opposite chroma: the two must never coincide.
        assert_ne!(yuv_to_rgb(126, 160, 128), yuv_to_rgb(126, 96, 128));
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
    fn sampling_reads_the_pixel_it_says_it_reads() {
        // A uniform image hides index arithmetic: every address looks the same. Each
        // pixel here carries its own column as luma, so a wrong row, a wrong macro-pixel
        // or an off-by-one lands on a different value. Measured: 30 of 185 mutants
        // survived on the uniform fixtures, nearly all of them in these indices.
        let mut yuyv = Vec::new();
        for y in 0..2u8 {
            for x in 0..4u8 {
                yuyv.push(16 + x * 10 + y); // luma of this pixel
                if x % 2 == 1 {
                    yuyv.push(128); // neutral U
                    yuyv.push(128); // neutral V
                }
            }
        }
        // Wait: YUYV is Y0 U Y1 V, so build it in that order instead.
        let mut packed = Vec::new();
        for y in 0..2u8 {
            for x in (0..4u8).step_by(2) {
                packed.push(16 + x * 10 + y);
                packed.push(128);
                packed.push(16 + (x + 1) * 10 + y);
                packed.push(128);
            }
        }
        let grey = |luma: u8| (luma as f32 - 16.0).mul_add(1.164_383, 0.0).round() as u8;

        let (width, height, pixels) = yuyv_to_rgba(&packed, 4, 2, 8, 1);
        assert_eq!((width, height), (4, 2));
        for y in 0..2usize {
            for x in 0..4usize {
                let expected = grey(16 + x as u8 * 10 + y as u8);
                assert_eq!(
                    pixels[(y * 4 + x) * 4],
                    expected,
                    "step 1: pixel ({x},{y}) is not its own column"
                );
            }
        }

        // step 2 samples the middle column of each block: x=1 and x=3.
        let (width, height, pixels) = yuyv_to_rgba(&packed, 4, 2, 8, 2);
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels[0], grey(16 + 10 + 1), "step 2 samples x=1");
        assert_eq!(pixels[4], grey(16 + 30 + 1), "step 2 samples x=3");
        let _ = yuyv;
    }

    #[test]
    fn rgba_sampling_reads_the_pixel_it_says_it_reads() {
        // Same idea for the Windows layout: every pixel carries its own coordinates.
        let mut rgba = Vec::new();
        for y in 0..2u8 {
            for x in 0..4u8 {
                rgba.extend_from_slice(&[x * 40, y * 40, 7, 255]);
            }
        }
        let (width, height, pixels) = rgba_to_rgba(&rgba, 4, 2, 16, 1);
        assert_eq!((width, height), (4, 2));
        for y in 0..2usize {
            for x in 0..4usize {
                let at = (y * 4 + x) * 4;
                assert_eq!(pixels[at], (x as u8) * 40, "red is the column");
                assert_eq!(pixels[at + 1], (y as u8) * 40, "green is the row");
                assert_eq!(pixels[at + 3], 255);
            }
        }
        let (_, _, pixels) = rgba_to_rgba(&rgba, 4, 2, 16, 2);
        assert_eq!(pixels[0], 40, "step 2 samples x=1");
        assert_eq!(pixels[4], 120, "step 2 samples x=3");
        // The block's middle row is y = 0*2 + 1 = 1, not its first row.
        assert_eq!(pixels[1], 40, "step 2 samples the middle row");
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
