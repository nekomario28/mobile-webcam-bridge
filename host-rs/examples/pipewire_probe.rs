//! Prove the PipeWire backend end to end without a phone.
//!
//! Opens the node the way the worker does, then pushes colour bars at 30 fps for a
//! few seconds so a real consumer can be pointed at it:
//!
//! ```text
//! cargo run --features pipewire --example pipewire_probe -- 640 480 8
//! gst-launch-1.0 pipewiresrc target-object=mobile-webcam num-buffers=1 \
//!   ! videoconvert ! pngenc ! filesink location=/tmp/pipewire-camera.png
//! ```

#[cfg(not(feature = "pipewire"))]
fn main() {
    eprintln!("build with --features pipewire");
}

#[cfg(feature = "pipewire")]
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use mobile_webcam::output::{Backend, open};

    let mut args = std::env::args().skip(1);
    let width: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(640);
    let height: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(480);
    let seconds: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(8);

    let mut sink = open(Backend::Pipewire, "Mobile Webcam", width, height, 30)?;
    let (stride, size) = sink.frame_layout();
    let mut frame = vec![0u8; size];
    // Five vertical bars in BT.709 limited range, so a consumer can tell "the right
    // picture arrived" from "some picture arrived".
    let bars = [
        (81u8, 90u8, 240u8),
        (145, 54, 34),
        (41, 240, 110),
        (200, 140, 150),
        (126, 128, 128),
    ];
    for row in 0..height as usize {
        for x in 0..width as usize {
            let (y, u, v) = bars[(x * bars.len() / width as usize).min(bars.len() - 1)];
            let at = row * stride + x / 2 * 4;
            frame[at] = y;
            frame[at + 1] = u;
            frame[at + 2] = y;
            frame[at + 3] = v;
        }
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    let mut submitted = 0u64;
    let mut watching = 0u64;
    while std::time::Instant::now() < deadline {
        let ready = sink.consuming()?;
        if ready {
            watching += 1;
        }
        sink.push(&frame, width, height)?;
        submitted += 1;
        if submitted.is_multiple_of(30) {
            eprintln!("submitted {submitted} frames, consumer attached for {watching} ticks");
        }
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
    println!("node \"mobile-webcam\" served {submitted} frames ({width}x{height})");
    Ok(())
}
