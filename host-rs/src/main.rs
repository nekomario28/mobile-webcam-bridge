#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--worker") => {
            let result = args
                .get(1)
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
        }
        // Explicit recovery, and the only path that ever replaces a stored key:
        // startup fails closed instead, so an unreadable key is never rotated
        // behind the user's back. The old file is renamed, not deleted.
        Some("--replace-identity") => {
            // Replacing a key that still reads is a different, destructive act:
            // it invalidates every approval for no reason. Require --force.
            let forced = args.get(1).map(String::as_str) == Some("--force");
            if !forced && mobile_webcam::identity::Identity::load_or_create().is_ok() {
                eprintln!(
                    "The stored PC key is readable, so nothing was replaced. Replacing it \
                     invalidates every phone approval; add --force if that is what you want."
                );
                std::process::exit(1);
            }
            match mobile_webcam::identity::Identity::replace_stored() {
                Ok(moved) => println!(
                    "The stored PC key was moved to {} and a new one was created.\n\
                     Every phone that approved this PC must approve it again.",
                    moved.display()
                ),
                Err(error) => {
                    eprintln!("The stored PC key was not replaced: {error}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            if let Err(e) = mobile_webcam::gui::run() {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
}
