#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--worker") {
        let result = args
            .next()
            .ok_or("missing parent PID")
            .and_then(|s| s.parse::<u32>().map_err(|_| "invalid parent PID"));
        match result {
            Ok(parent) => {
                if mobile_webcam::video::worker(parent).is_err() {
                    std::process::exit(1);
                }
            }
            Err(_) => std::process::exit(2),
        }
    } else if let Err(e) = mobile_webcam::gui::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
