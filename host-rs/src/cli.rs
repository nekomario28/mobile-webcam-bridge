//! Command-line entry points.
//!
//! Kept out of `main` so the recovery command's arguments can be tested without
//! launching a GUI or touching real storage: `--replace-identity` is the one
//! command a user runs when the app will not start, so a typo must not silently
//! open the app instead of naming the mistake.

use crate::Command;

pub const USAGE: &str = "\
Usage: mobile-webcam [--help]
       mobile-webcam --worker <parent-pid>
       mobile-webcam --replace-identity [--force]
       mobile-webcam --headless [--transport usb|lan] [--host ADDRESS] [--seconds N]

  --help               Show this text.
  --headless           Run a session without a window and print every event as one
                       JSON line. Exits 0 if it streamed, 1 if it did not, 2 if it
                       could not start.
  --transport MODE     Override the stored transport for this run.
  --host ADDRESS       Override the stored Wi-Fi address for this run.
  --seconds N          Stop after N seconds.
  --replace-identity   Move an unreadable stored PC key aside and publish a new
                       one. Every phone that approved this PC must approve it
                       again. Refused while the stored key is still readable,
                       because that replacement destroys working approvals.
  --force              Replace the stored key even though it still reads.
";

/// Parse `args` (the program name already removed).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => Ok(Command::Gui),
        ["--help"] | ["-h"] => Ok(Command::Help),
        ["--worker", parent] => parent
            .parse::<u32>()
            .map(|parent| Command::Worker { parent })
            .map_err(|_| format!("--worker needs a numeric parent PID, got {parent:?}")),
        ["--headless", rest @ ..] => headless(rest),
        ["--replace-identity"] => Ok(Command::ReplaceIdentity { force: false }),
        ["--replace-identity", "--force"] => Ok(Command::ReplaceIdentity { force: true }),
        ["--force"] => Err("--force only applies together with --replace-identity".into()),
        // `["--worker", parent]` above already covers every two-word worker form.
        ["--replace-identity", other] => Err(format!("unexpected argument {other:?}")),
        [unknown] if unknown.starts_with('-') => Err(format!("unknown option {unknown:?}")),
        [unknown, ..] => Err(format!("unexpected argument {unknown:?}")),
    }
}

/// `--headless` followed by any order of `--transport`, `--host` and `--seconds`.
fn headless(rest: &[&str]) -> Result<Command, String> {
    let mut options = crate::headless::Options::default();
    let mut index = 0;
    while index < rest.len() {
        let value = rest.get(index + 1).copied();
        match (rest[index], value) {
            ("--transport", Some(mode)) if mode == "usb" || mode == "lan" => {
                options.transport = Some(mode.to_string());
            }
            ("--transport", Some(mode)) => {
                return Err(format!(
                    "--transport needs \"usb\" or \"lan\", got {mode:?}"
                ));
            }
            ("--host", Some(host)) if !host.is_empty() => options.host = Some(host.to_string()),
            ("--host", Some(_)) => return Err("--host needs an address".into()),
            ("--seconds", Some(n)) => match n.parse::<u64>() {
                Ok(seconds) if seconds > 0 => options.seconds = Some(seconds),
                _ => return Err(format!("--seconds needs a positive number, got {n:?}")),
            },
            (flag, None) => return Err(format!("{flag} needs a value")),
            (flag, _) => return Err(format!("unexpected argument after --headless: {flag:?}")),
        }
        index += 2;
    }
    Ok(Command::Headless(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(words: &[&str]) -> Result<Command, String> {
        parse(
            &words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn no_arguments_open_the_gui() {
        assert!(matches!(parse_words(&[]), Ok(Command::Gui)));
    }

    #[test]
    fn help_is_asked_for_explicitly() {
        assert!(matches!(parse_words(&["--help"]), Ok(Command::Help)));
        assert!(matches!(parse_words(&["-h"]), Ok(Command::Help)));
    }

    #[test]
    fn the_worker_takes_one_numeric_parent() {
        assert!(matches!(
            parse_words(&["--worker", "4242"]),
            Ok(Command::Worker { parent: 4242 })
        ));
        assert!(parse_words(&["--worker", "not-a-pid"]).is_err());
        assert!(parse_words(&["--worker"]).is_err());
    }

    #[test]
    fn recovery_replaces_only_when_asked_and_forced_only_with_it() {
        assert!(matches!(
            parse_words(&["--replace-identity"]),
            Ok(Command::ReplaceIdentity { force: false })
        ));
        assert!(matches!(
            parse_words(&["--replace-identity", "--force"]),
            Ok(Command::ReplaceIdentity { force: true })
        ));
        let alone = parse_words(&["--force"]).unwrap_err();
        assert!(alone.contains("--replace-identity"), "{alone}");
    }

    #[test]
    fn headless_takes_overrides_in_any_order_and_refuses_nonsense() {
        let parsed = parse_words(&["--headless"]).unwrap();
        assert_eq!(
            parsed,
            Command::Headless(crate::headless::Options::default())
        );

        let parsed = parse_words(&["--headless", "--seconds", "30", "--transport", "lan"]).unwrap();
        let Command::Headless(options) = parsed else {
            panic!("expected headless");
        };
        assert_eq!(options.seconds, Some(30));
        assert_eq!(options.transport.as_deref(), Some("lan"));
        assert_eq!(options.host, None);

        // A script that typos a flag must be told, not run for the wrong duration.
        assert!(parse_words(&["--headless", "--transport", "wifi"]).is_err());
        assert!(parse_words(&["--headless", "--seconds", "0"]).is_err());
        assert!(parse_words(&["--headless", "--seconds"]).is_err());
        assert!(parse_words(&["--headless", "stray"]).is_err());
    }

    #[test]
    fn a_mistyped_recovery_flag_is_named_rather_than_opening_the_gui() {
        // The failure this exists for: `--replace-identiy` used to fall through to
        // the GUI, which cannot start for the very reason the user ran it.
        let error = parse_words(&["--replace-identiy"]).unwrap_err();
        assert!(error.contains("unknown option"), "{error}");
        assert!(parse_words(&["--replace-identity", "extra"]).is_err());
        assert!(parse_words(&["stray"]).is_err());
    }
}
