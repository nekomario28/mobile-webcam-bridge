//! A session without a window.
//!
//! The GUI is the product, but a script — a CI job, a soak, a phone on a bench —
//! cannot press Connect. This starts the same worker the GUI starts, speaks the same
//! control protocol to it, and prints every event as one JSON line on stdout so the
//! caller can gate on what happened. A Wi-Fi pairing event's access key is replaced
//! with `<redacted>` before that print: the worker still sends the key to the GUI
//! over the same pipe, but this process's stdout is a log. No egui, no window, no
//! X server.
//!
//! Exit code: 0 if the session reached streaming, 1 if it did not, 2 if it could not
//! start at all (no settings, unreadable key, worker refused).

use std::io::{BufRead, BufReader};
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
    /// Ask the phone to switch capture mode once streaming has started.
    ///
    /// The gate for the quality controls (#8) needs a *repeatable* way to ask a device to
    /// change resolution or frame rate: the capture path is what runs, the worker drives
    /// the staged apply and sends the commit, and this only says what to want. Exits 1 if
    /// the phone confirms a different mode than the one asked for.
    pub quality: Option<(u16, u16, u16)>,
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
    let ttff_transport = settings.transport.clone();
    let ttff_origin = Instant::now();
    let mut ttff_logged = false;
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
    let connect_at = Instant::now();
    eprintln!(
        "{}",
        crate::ttff::line(
            "connect_started",
            ttff_origin.elapsed().as_millis(),
            None,
            &ttff_transport,
        )
    );
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
    let mut requested = false;
    let mut applied = None;
    let mut refused = None;
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
                        if let Event::Stats { submitted, .. } = &event
                            && let Some(mark) = crate::ttff::first_frame(
                                &mut ttff_logged,
                                *submitted,
                                ttff_origin.elapsed().as_millis(),
                                Some(connect_at.elapsed().as_millis()),
                                &ttff_transport,
                            )
                        {
                            // stderr: stdout is the event JSON a script parses.
                            eprintln!("{mark}");
                        }
                        if matches!(event, Event::Stats { .. }) {
                            reached_stream = true;
                            // Ask once the stream exists: a mode change on a session that
                            // has not started is staged by design, and this measures the
                            // switch, not the staging.
                            if let Some((width, height, fps)) = options.quality
                                && !requested
                            {
                                requested = true;
                                println!("requesting {width}x{height}@{fps} from the phone");
                                if let Err(error) = control::write(
                                    &mut stdin,
                                    &Command::PrepareQuality { width, height, fps },
                                ) {
                                    eprintln!("cannot ask for a mode change: {error}");
                                }
                            }
                        }
                        match &event {
                            Event::QualityApplied { width, height, fps } => {
                                applied = Some((*width, *height, *fps));
                            }
                            Event::QualityFailed { reason, .. }
                            | Event::Error { message: reason } => {
                                refused = Some(reason.clone());
                            }
                            _ => {}
                        }
                        println!("{}", printed_event_line(text, &event));
                        if matches!(event, Event::Stopped {}) {
                            break;
                        }
                    }
                    // The worker is the only writer, but a malformed line is worth
                    // showing rather than dropping: it is the one thing that would
                    // make a caller's JSON parse mysteriously fail. A pairing line
                    // is the exception: its body is the access key.
                    Err(error) => {
                        if text.contains("pairing_info") {
                            eprintln!("unreadable pairing event line: {error}");
                        } else {
                            eprintln!("unreadable event line: {error}: {text}");
                        }
                    }
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
    if !reached_stream {
        return 1;
    }
    let Some((width, height, fps)) = options.quality else {
        return 0;
    };
    match applied {
        Some(mode) if mode == (width, height, fps) => {
            println!("applied {width}x{height}@{fps}: PASS");
            0
        }
        Some(mode) => {
            println!(
                "asked for {width}x{height}@{fps} but the phone applied {}x{}@{}: FAIL",
                mode.0, mode.1, mode.2
            );
            1
        }
        None => {
            println!(
                "the phone never confirmed a mode{}: FAIL",
                refused
                    .map(|reason| format!(" ({reason})"))
                    .unwrap_or_default()
            );
            1
        }
    }
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

/// The line this process prints. Pairing events keep their address and
/// fingerprint and lose the access key; every other event is unchanged.
fn printed_event_line(text: &str, event: &Event) -> String {
    if !matches!(event, Event::PairingInfo { .. }) {
        return text.to_string();
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) else {
        return "pairing event omitted: unreadable".into();
    };
    let Some(object) = value.as_object_mut() else {
        return "pairing event omitted: unreadable".into();
    };
    object.insert("key".into(), serde_json::Value::String("<redacted>".into()));
    serde_json::to_string(&value).unwrap_or_else(|_| "pairing event omitted: unreadable".into())
}

/// `640x480@30` — the mode the caller wants the phone to switch to.
pub fn parse_quality(value: &str) -> Result<(u16, u16, u16), String> {
    let (size, fps) = value
        .split_once('@')
        .ok_or_else(|| format!("--quality wants WxH@fps, got {value:?}"))?;
    let (width, height) = size
        .split_once('x')
        .ok_or_else(|| format!("--quality wants WxH@fps, got {value:?}"))?;
    let parse = |text: &str, what: &str| {
        text.parse::<u16>()
            .map_err(|_| format!("--quality {what} is not a number: {text:?}"))
    };
    let (width, height, fps) = (
        parse(width, "width")?,
        parse(height, "height")?,
        parse(fps, "fps")?,
    );
    if width == 0 || height == 0 || fps == 0 {
        return Err(format!("--quality needs non-zero values, got {value:?}"));
    }
    Ok((width, height, fps))
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
    fn a_printed_pairing_event_does_not_contain_the_access_key() {
        let key = "0123456789abcdef0123456789abcdef";
        let event = Event::PairingInfo {
            addresses: vec!["192.168.1.9".into()],
            fingerprint: "ab".repeat(32),
            key: key.into(),
        };
        let raw = serde_json::to_string(&event).unwrap();
        assert!(raw.contains(key));
        let printed = printed_event_line(&raw, &event);
        assert!(!printed.contains(key), "{printed}");
        assert!(printed.contains("<redacted>"));
        assert!(printed.contains("192.168.1.9"));
        let stats_event = stats();
        let stats_line = serde_json::to_string(&stats_event).unwrap();
        assert_eq!(printed_event_line(&stats_line, &stats_event), stats_line);
        assert_eq!(
            printed_event_line("not-json", &event),
            "pairing event omitted: unreadable"
        );
    }

    #[test]
    fn a_quality_request_is_parsed_strictly() {
        assert_eq!(parse_quality("640x480@30"), Ok((640, 480, 30)));
        assert_eq!(parse_quality("1920x1080@60"), Ok((1920, 1080, 60)));
        // A typo must be refused rather than quietly asking for something else: this
        // argument decides what a device is told to capture.
        assert!(parse_quality("640x480").is_err());
        assert!(parse_quality("640@30").is_err());
        assert!(parse_quality("0x480@30").is_err());
        assert!(parse_quality("640x480@0").is_err());
        assert!(parse_quality("vga@30").is_err());
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
                quality: None,
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
