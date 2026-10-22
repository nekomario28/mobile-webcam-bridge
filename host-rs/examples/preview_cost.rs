//! What the preview costs, measured rather than assumed.
//!
//! The worker converts a frame for the preview at 10 Hz whether or not anyone is
//! watching, so the conversion and the publish are on the streaming path. This prints
//! their per-frame cost for the modes the phone actually offers.

use std::hint::black_box;
use std::time::Instant;

use mobile_webcam::preview;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("mode          convert      publish      per second at 10 Hz");
    for (width, height) in [(320u32, 240u32), (640, 480), (1280, 720), (1920, 1080)] {
        let stride = width as usize * 2;
        let frame = vec![128u8; stride * height as usize];

        let runs = 200;
        let mut convert = Vec::with_capacity(runs);
        let mut shot = None;
        for _ in 0..runs {
            let started = Instant::now();
            let scaled = preview::from_sink(&frame, width as usize, height as usize, stride);
            convert.push(started.elapsed());
            black_box(&scaled);
            shot = Some(scaled);
        }
        let shot = shot.expect("a frame");

        // The publish is a real file write; measured on the same record the UI reads.
        let path = preview::create_path()?;
        let mut producer = preview::Producer::open(&path)?;
        let mut publish = Vec::with_capacity(runs);
        for _ in 0..runs {
            let started = Instant::now();
            producer.publish(&shot)?;
            publish.push(started.elapsed());
        }
        std::fs::remove_file(&path).ok();

        let mean = |v: &[std::time::Duration]| {
            v.iter().map(|d| d.as_secs_f64()).sum::<f64>() / v.len() as f64
        };
        let (c, p) = (mean(&convert) * 1e6, mean(&publish) * 1e6);
        println!(
            "{width}x{height:<5}  {c:8.1} us  {p:8.1} us   {:6.2} % of one core",
            (c + p) * 10.0 / 1e6 * 100.0
        );
    }
    Ok(())
}
