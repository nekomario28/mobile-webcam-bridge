//! A PipeWire camera, for machines that should not need a kernel module.
//!
//! `v4l2loopback` is the portable answer but it is a module to install and usually a
//! `pkexec modprobe` to run; a PipeWire `Video/Source` node needs neither. The trade
//! is reach: only PipeWire-aware consumers see it today (OBS, and browsers behind
//! their PipeWire-camera flags), so this backend is offered next to the loopback
//! rather than instead of it.
//!
//! The crate this is built on drives its own timer and *pulls* a frame at the
//! negotiated rate, while the worker *pushes* frames as they decode. The bridge is one
//! slot: the worker stores the newest converted frame, the timer copies it out.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use pipewire_vircam::{Camera as Node, Config, Format, Mode, State};

/// The newest frame the worker produced, in the layout the node advertises.
struct Latest {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

pub struct Camera {
    latest: Arc<Mutex<Latest>>,
    /// Whether a consumer has the node in `streaming`: the worker only counts a frame
    /// as submitted when someone is actually watching, as with the loopback.
    streaming: Arc<AtomicBool>,
    stride: usize,
    size: usize,
    /// Detached on purpose: the node's main loop is not `Send`, and its lifetime is
    /// the worker process, which exits when the session does. A session restart is a
    /// new process, so no stale node is left behind.
    #[allow(dead_code)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Camera {
    pub fn open(name: &str, width: u32, height: u32, fps: u32) -> crate::Result<Self> {
        if !width.is_multiple_of(2) {
            return Err("a PipeWire YUY2 camera needs an even width".into());
        }
        let stride = width as usize * 2;
        let size = stride * height as usize;
        let latest = Arc::new(Mutex::new(Latest {
            bytes: vec![16; size],
            width,
            height,
        }));
        let streaming = Arc::new(AtomicBool::new(false));

        let config = Config {
            name: "mobile-webcam".into(),
            media_name: name.to_string(),
            modes: vec![Mode {
                width,
                height,
                fps: vec![fps.max(1)],
                formats: vec![Format::Yuy2],
            }],
            max_buffers: 4,
        };
        let fill_latest = latest.clone();
        let fill_streaming = streaming.clone();
        let state_streaming = streaming.clone();
        // The node is created on the thread that will run it, because the PipeWire
        // handles are not `Send`; the parent waits for the verdict instead of
        // pretending the camera exists.
        let (verdict, outcome) = std::sync::mpsc::channel::<Result<(), String>>();
        // The `pipewire` crate asserts that its main loop is created on a thread named
        // "main" — an unnamed or differently named thread panics inside the crate
        // (measured), so the name is part of the contract, not decoration.
        let thread = std::thread::Builder::new()
            .name("main".into())
            .spawn(move || {
                let node = match Node::new(config) {
                    Ok(node) => node,
                    Err(error) => {
                        let _ = verdict.send(Err(error.to_string()));
                        return;
                    }
                };
                let _ = verdict.send(Ok(()));
                let result = node
                    .on_state(move |state: State| {
                        state_streaming
                            .store(matches!(state, State::Streaming { .. }), Ordering::Release);
                    })
                    .run(move |frame, _negotiated| {
                        if !fill_streaming.load(Ordering::Acquire) {
                            return;
                        }
                        let source = fill_latest.lock().expect("preview slot poisoned");
                        let (width, height) = (source.width, source.height);
                        let stride = width as usize * 2;
                        for plane in &mut frame.planes {
                            copy_rows(
                                &source.bytes,
                                stride,
                                plane.data,
                                plane.stride as usize,
                                height as usize,
                            );
                        }
                    });
                if let Err(error) = result {
                    eprintln!("mobile-webcam: PipeWire camera stopped: {error}");
                }
            })?;
        match outcome.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error.into());
            }
            Err(_) => {
                let _ = thread.join();
                return Err("the PipeWire camera node did not start".into());
            }
        }
        Ok(Self {
            latest,
            streaming,
            stride,
            size,
            thread: Some(thread),
        })
    }

    pub fn ready(&mut self) -> crate::Result<bool> {
        Ok(self.streaming.load(Ordering::Acquire))
    }

    pub fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        if bytes.len() < width as usize * height as usize * 2 {
            return Err("frame is shorter than the advertised YUY2 size".into());
        }
        let mut latest = self.latest.lock().map_err(|_| "preview slot poisoned")?;
        if latest.bytes.len() == bytes.len() {
            latest.bytes.copy_from_slice(bytes);
        } else {
            latest.bytes.clear();
            latest.bytes.extend_from_slice(bytes);
        }
        latest.width = width;
        latest.height = height;
        Ok(())
    }

    pub(crate) fn layout(&self) -> (usize, usize) {
        (self.stride, self.size)
    }
}

/// Copy `height` rows from `source` (pitch `source_stride`) into `target` (pitch
/// `target_stride`).
///
/// Kept separate and pure because the two strides disagree in practice: the node
/// reports the stride it negotiated, and it is not always `width * 2`.
fn copy_rows(
    source: &[u8],
    source_stride: usize,
    target: &mut [u8],
    target_stride: usize,
    height: usize,
) {
    for row in 0..height {
        let from = row * source_stride;
        let to = row * target_stride;
        let count = source_stride.min(target_stride);
        if from + count > source.len() || to + count > target.len() {
            return;
        }
        target[to..to + count].copy_from_slice(&source[from..from + count]);
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        // Nothing to stop from here (see `thread`): the process owns the node.
        self.streaming.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::copy_rows;

    #[test]
    fn rows_are_copied_at_their_own_pitches() {
        // Three source rows of four bytes, written into a wider target.
        let source = [1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        let mut target = [0u8; 18];
        copy_rows(&source, 4, &mut target, 6, 3);
        assert_eq!(&target[0..4], &[1, 1, 1, 1]);
        assert_eq!(&target[6..10], &[2, 2, 2, 2]);
        assert_eq!(&target[12..16], &[3, 3, 3, 3]);
        // The padding the target added stays untouched.
        assert_eq!(target[4], 0);
        assert_eq!(target[5], 0);
    }

    #[test]
    fn a_short_target_is_left_alone_instead_of_panicking() {
        let source = [7u8; 16];
        let mut target = [0u8; 3];
        copy_rows(&source, 4, &mut target, 4, 4);
        assert_eq!(target, [0, 0, 0]);
    }

    #[test]
    fn a_narrower_target_clips_instead_of_overflowing() {
        let source = [9u8; 8];
        let mut target = [0u8; 4];
        copy_rows(&source, 4, &mut target, 2, 2);
        assert_eq!(target, [9, 9, 9, 9]);
    }
}
