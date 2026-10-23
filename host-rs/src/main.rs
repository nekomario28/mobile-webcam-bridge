#![cfg_attr(windows, windows_subsystem = "windows")]
use mobile_webcam::Command;
use mobile_webcam::cli::{self, USAGE};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    match command {
        Command::Help => print!("{USAGE}"),
        Command::Gui => {
            if let Err(error) = mobile_webcam::gui::run() {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Command::Worker { parent } => {
            if mobile_webcam::video::worker(parent).is_err() {
                std::process::exit(1);
            }
        }
        // A session without a window: the same worker, the same control protocol,
        // events as JSON lines on stdout. The exit code is the answer a script gates
        // on, so it comes from whether a stream actually happened.
        Command::Headless(options) => std::process::exit(mobile_webcam::headless::run(options)),
        // Explicit recovery, and the only path that ever replaces a stored key:
        // startup fails closed instead, so an unreadable key is never rotated
        // behind the user's back. The old file is renamed, not deleted.
        Command::ReplaceIdentity { force } => {
            // Replacing a key that still reads is a different, destructive act:
            // it invalidates every approval for no reason. Require --force.
            //
            // `stored_is_readable`, not `load_or_create`: the latter publishes a key
            // when none exists, so refusing to replace would itself have created one
            // and "nothing was replaced" would have been false.
            let readable = mobile_webcam::identity::Identity::stored_is_readable().unwrap_or(false);
            if !force && readable {
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
    }
}
