//! Time-to-first-frame marks.
//!
//! One line, fixed tokens, two durations. No address, device name, key, or
//! error text: a phase that is not a token is reduced to one, and a transport
//! that is not `usb` or `lan` is recorded as `unknown`.

/// `ttff phase=host_first_submitted since_process_ms=12 since_connect_ms=4 transport=usb`
pub fn line(
    phase: &str,
    since_process_ms: u128,
    since_connect_ms: Option<u128>,
    transport: &str,
) -> String {
    let phase = token(phase);
    let transport = match transport {
        "usb" | "lan" => transport,
        _ => "unknown",
    };
    match since_connect_ms {
        Some(ms) => format!(
            "ttff phase={phase} since_process_ms={since_process_ms} since_connect_ms={ms} transport={transport}"
        ),
        None => {
            format!("ttff phase={phase} since_process_ms={since_process_ms} transport={transport}")
        }
    }
}

/// What may follow `phase=`. Empty input becomes `unknown` rather than a blank.
fn token(phase: &str) -> String {
    let token: String = phase
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(32)
        .collect();
    if token.is_empty() {
        "unknown".into()
    } else {
        token
    }
}

/// The first submitted frame of a connect attempt. Later frames return nothing.
pub fn first_frame(
    already_logged: &mut bool,
    submitted: u64,
    since_process_ms: u128,
    since_connect_ms: Option<u128>,
    transport: &str,
) -> Option<String> {
    if *already_logged || submitted == 0 {
        return None;
    }
    *already_logged = true;
    Some(line(
        "host_first_submitted",
        since_process_ms,
        since_connect_ms,
        transport,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_mark_names_only_the_clocks_and_the_transport() {
        assert_eq!(
            line("host_first_submitted", 12, Some(4), "usb"),
            "ttff phase=host_first_submitted since_process_ms=12 since_connect_ms=4 transport=usb"
        );
        assert_eq!(
            line("connect_started", 3, None, "lan"),
            "ttff phase=connect_started since_process_ms=3 transport=lan"
        );
    }

    #[test]
    fn free_form_text_cannot_add_a_field() {
        // Dots, spaces, newlines and '=' are dropped, so an address or a second
        // assignment cannot change the shape of the line. The transport is one
        // of two tokens; anything else is `unknown`.
        let line = line("host 192.168.1.5\nnote=secret", 1, Some(1), "tcp://phone");
        assert_eq!(
            line,
            "ttff phase=host19216815notesecret since_process_ms=1 since_connect_ms=1 transport=unknown"
        );
        assert!(!line.contains("192.168"));
        assert!(!line.contains('\n'));
        // phase, since_process_ms, since_connect_ms, transport. No extra assignment.
        assert_eq!(line.chars().filter(|c| *c == '=').count(), 4);
    }

    #[test]
    fn only_the_first_submitted_frame_is_a_mark() {
        let mut logged = false;
        assert!(first_frame(&mut logged, 0, 1, Some(1), "usb").is_none());
        assert!(!logged);
        let mark = first_frame(&mut logged, 1, 20, Some(5), "lan").expect("first frame");
        assert!(mark.contains("since_process_ms=20"));
        assert!(mark.contains("transport=lan"));
        assert!(first_frame(&mut logged, 30, 99, Some(9), "lan").is_none());
    }
}
