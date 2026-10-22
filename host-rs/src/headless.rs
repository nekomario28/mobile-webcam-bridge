//! A session without a window.
//!
//! The GUI is the product, but a script — a CI job, a soak, a phone on a bench —
//! cannot press Connect. This starts the same worker the GUI starts, speaks the same
//! control protocol to it, and prints every event as one JSON line on stdout so the
//! caller can gate on what happened. No egui, no window, no X server.
//!
//! Exit code: 0 if the session reached streaming, 1 if it did not, 2 if it could not
//! start at all (no settings, unreadable key, worker refused).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command as Process, Stdio};
use std::time::{Duration, Instant};

use crate::control::{self, Command, Event};
use crate::settings::{Settings, Store};

/// What the caller asked for on the command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Override the stored transport ("usb" or "lan").
    pub transport: Option<String>,
    /// Override the stored Wi-Fi address.
    pub host: Option<String>,
    /// Stop after this many seconds; `None` runs until the session ends.
    pub seconds: Option<u64>,
}

/// Did this run of events reach a stream?
///
/// Extracted from the loop so the exit-code rule can be tested without a phone, a
/// worker or a socket.
pub fn streamed(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::Stats { .. }))
}

pub fn run(options: Options) -> i32 {
    let settings = match Store::native().and_then(|store| store.load()) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("cannot read settings: {error}");
            return 2;
        }
    };
    let settings = merge(settings, &options);
    if let Err(error) = settings.validate() {
        eprintln!("settings are not usable: {error}");
        return 2;
    }

    let mut child = match spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("cannot start the worker: {error}");
            return 2;
        }
    };
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            eprintln!("worker input unavailable");
            let _ = child.kill();
            return 2;
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            eprintln!("worker output unavailable");
            let _ = child.kill();
            return 2;
        }
    };
    if let Err(error) = control::write(
        &mut stdin,
        &Command::Start {
            settings: Box::new(settings),
            device: None,
        },
    ) {
        eprintln!("cannot start the session: {error}");
        let _ = child.kill();
        return 2;
    }

    let deadline = options
        .seconds
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut reached_stream = false;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        if let Some(deadline) = deadline
            && Instant::now() >= deadline
        {
            let _ = control::write(&mut stdin, &Command::Stop {});
            break;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let text = line.trim_end();
                if text.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Event>(text) {
                    Ok(event) => {
                        if matches!(event, Event::Stats { .. }) {
                            reached_stream = true;
                        }
                        println!("{text}");
                        if matches!(event, Event::Stopped {}) {
                            break;
                        }
                    }
                    // The worker is the only writer, but a malformed line is worth
                    // showing rather than dropping: it is the one thing that would
                    // make a caller's JSON parse mysteriously fail.
                    Err(error) => eprintln!("unreadable event line: {error}: {text}"),
                }
            }
            Err(error) => {
                eprintln!("cannot read events: {error}");
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    if reached_stream { 0 } else { 1 }
}

/// Apply the command-line overrides to the stored settings.
pub fn merge(mut settings: Settings, options: &Options) -> Settings {
    if let Some(transport) = &options.transport {
        settings.transport = transport.clone();
    }
    if let Some(host) = &options.host {
        settings.lan_host = host.clone();
    }
    settings
}

fn spawn() -> std::io::Result<Child> {
    let executable = std::env::current_exe()?;
    let mut command = Process::new(executable);
    command
        .args(["--worker", &std::process::id().to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Event {
        Event::Stats {
            decoded: 1,
            submitted: 1,
            active: true,
            width: 1280,
            height: 720,
            target_fps: 30,
            decoder: "software".into(),
        }
    }

    #[test]
    fn only_a_streamed_run_exits_zero() {
        // The exit code is what a CI job gates on, so the rule lives in one function:
        // connecting and authorizing is not streaming, and a failure is not either.
        assert!(!streamed(&[Event::Connecting {}, Event::Waiting {}]));
        assert!(!streamed(&[Event::Error {
            message: "connection refused".into()
        }]));
        assert!(streamed(&[Event::Connecting {}, stats()]));
    }

    #[test]
    fn the_command_line_overrides_only_what_it_names() {
        let base = Settings::default();
        let merged = merge(
            base.clone(),
            &Options {
                transport: Some("lan".into()),
                host: Some("192.168.1.14".into()),
                seconds: None,
            },
        );
        assert_eq!(merged.transport, "lan");
        assert_eq!(merged.lan_host, "192.168.1.14");
        // Everything else stays as stored.
        assert_eq!(merged.output, base.output);
        assert_eq!(merged.decode, base.decode);

        let untouched = merge(base.clone(), &Options::default());
        assert_eq!(untouched, base);
    }
}
