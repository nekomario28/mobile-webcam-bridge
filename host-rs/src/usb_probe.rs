//! USB end-to-end probe: the AOA transition, then a synchronous PING/PONG exchange.
//!
//! Gate G0 (AOA capability and transition) and G0.5 (framed bulk smoke test) are
//! described in `docs/gates.md` and, until 2026-10-24, **nothing in the Rust host could
//! run G0.5**: the `PING` type existed as a constant and a wire test, and the Android app
//! answers it, but no shipping code ever sent one. That is a gate with no driver.
//!
//! This is the driver. It uses the same pieces the real session uses — `usb::Usb` for the
//! transport (which performs the AOA switch in `connect`) and `client_auth::exchange` for
//! the gate the phone requires before it answers control messages — so the probe exercises
//! the shipping path rather than a copy of it.
//!
//! What it does NOT do: capture. G0.5 is a control-plane round trip, so the phone's camera
//! need not be started, and a run that needs no camera is a run that can be repeated.

use std::time::{Duration, Instant};

use crate::transport::Transport;
use crate::wire::{self, Frame};

/// What the caller asked for on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Round trips to attempt; G0.5 asks for 1000.
    pub frames: u64,
    /// Per-exchange budget.
    pub budget: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            frames: 1000,
            budget: Duration::from_millis(2000),
        }
    }
}

/// Run `Options` against an attached phone and print the gate's evidence.
pub fn cli(options: &Options) -> i32 {
    println!(
        "usb probe: {} round trips, {} ms budget each",
        options.frames,
        options.budget.as_millis()
    );
    let announced = std::cell::Cell::new(false);
    let result = end_to_end(None, options.frames, options.budget, |client_id| {
        if !announced.get() {
            announced.set(true);
            println!(
                "waiting for the phone to approve this PC (PC ID {client_id}); \
                 tap Allow on the phone if it asks"
            );
        }
    });
    match result {
        Ok((report, after)) => {
            println!(
                "device after the transition: {}:{:04x}:{:04x} (accessory: {})",
                after.bus,
                after.vid,
                after.pid,
                crate::usb::accessory(after.vid, after.pid)
            );
            println!("{}", report.summary(options.frames));
            if report.pass(options.frames) { 0 } else { 1 }
        }
        Err(error) => {
            eprintln!("usb probe could not run: {error}");
            2
        }
    }
}

/// What one PING/PONG exchange showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Sequence, flags and the 8-byte challenge all matched.
    Echoed,
    /// The reply came back with a different sequence: the framing lost its place.
    SequenceMismatch,
    /// The reply carried a different challenge: the payload was corrupted.
    PayloadMismatch,
    /// Nothing arrived inside the budget.
    TimedOut,
}

/// The counters one run reports, with the percentiles G0.5 asks for.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub sent: u64,
    pub echoed: u64,
    pub sequence_mismatch: u64,
    pub payload_mismatch: u64,
    pub timed_out: u64,
    /// Frames that arrived while waiting for a PONG but were not replies to this PING —
    /// the phone's capability advertisement, for instance. The real session ignores them
    /// too; the first version of this probe read one as "the reply" and reported a
    /// sequence mismatch for every single exchange.
    pub skipped: u64,
    pub rtts_us: Vec<u64>,
}

impl Report {
    pub fn pass(&self, expected: u64) -> bool {
        self.echoed == expected
            && self.sequence_mismatch == 0
            && self.payload_mismatch == 0
            && self.timed_out == 0
    }

    /// `(min, avg, p50, p95, max)` in microseconds; empty when nothing was echoed.
    pub fn rtt_percentiles(&self) -> Option<(u64, u64, u64, u64, u64)> {
        if self.rtts_us.is_empty() {
            return None;
        }
        let mut sorted = self.rtts_us.clone();
        sorted.sort_unstable();
        let at = |percent: usize| sorted[(sorted.len() - 1) * percent / 100];
        let total: u64 = sorted.iter().sum();
        Some((
            sorted[0],
            total / sorted.len() as u64,
            at(50),
            at(95),
            sorted[sorted.len() - 1],
        ))
    }

    pub fn summary(&self, expected: u64) -> String {
        let rtt = match self.rtt_percentiles() {
            Some((min, avg, p50, p95, max)) => {
                format!("rtt us: min {min} avg {avg} p50 {p50} p95 {p95} max {max}")
            }
            None => "rtt: no replies".to_string(),
        };
        format!(
            "sent {} echoed {} sequence-mismatch {} payload-mismatch {} timed-out {} skipped {}\n{rtt}\n{}",
            self.sent,
            self.echoed,
            self.sequence_mismatch,
            self.payload_mismatch,
            self.timed_out,
            self.skipped,
            if self.pass(expected) { "PASS" } else { "FAIL" }
        )
    }
}

/// The 8-byte challenge for a sequence number: deterministic, so a run is reproducible.
pub fn challenge(sequence: u32) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&sequence.to_le_bytes());
    bytes[4..].copy_from_slice(&(!sequence).to_le_bytes());
    bytes
}

/// Classify one reply against the request that produced it.
pub fn classify(sent_sequence: u32, sent_challenge: &[u8; 8], reply: Option<&Frame>) -> Verdict {
    let Some(reply) = reply else {
        return Verdict::TimedOut;
    };
    if reply.header.kind != wire::PONG || reply.header.sequence != sent_sequence {
        return Verdict::SequenceMismatch;
    }
    if reply.payload.as_slice() != sent_challenge.as_slice() {
        return Verdict::PayloadMismatch;
    }
    Verdict::Echoed
}

/// Print the frames the real reader parses, with the error it hits.
///
/// Between "the bytes are there" (`--usb-dump`) and "the exchange failed" sits the reader;
/// this is the only surface that says which of the two is at fault.
pub fn frames(seconds: u64) -> i32 {
    let target = match crate::usb::list() {
        Ok(candidates) => candidates
            .iter()
            .find(|candidate| crate::usb::accessory(candidate.id.vid, candidate.id.pid))
            .or_else(|| candidates.iter().find(|candidate| candidate.suggested))
            .map(|candidate| candidate.id.clone()),
        Err(error) => {
            eprintln!("cannot list devices: {error}");
            return 2;
        }
    };
    let Some(target) = target else {
        eprintln!("no USB device is attached");
        return 2;
    };
    let mut transport = match crate::usb::Usb::connect(&target) {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("cannot open the device: {error}");
            return 2;
        }
    };
    println!("parsing frames for {seconds}s");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut parsed = 0usize;
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        match transport.poll_receive(left.min(Duration::from_millis(500))) {
            Ok(Some(frame)) => {
                parsed += 1;
                println!(
                    "frame kind=0x{:02x} seq={} len={}",
                    frame.header.kind,
                    frame.header.sequence,
                    frame.payload.len()
                );
            }
            Ok(None) => continue,
            Err(error) => {
                eprintln!("reader error after {parsed} frame(s): {error}");
                return 2;
            }
        }
    }
    println!("total {parsed} frame(s)");
    if parsed == 0 { 1 } else { 0 }
}

/// Print the first bytes the phone sends, without parsing them.
///
/// The gate says what the phone must send; this shows what it does. Added because a run
/// ended at "incomplete frame deadline expired", which is a fact about the *host's* reader
/// and could be either a truncated frame or a reader that asked for the wrong size.
pub fn dump(seconds: u64) -> i32 {
    let target = match crate::usb::list() {
        Ok(candidates) => candidates
            .iter()
            .find(|candidate| crate::usb::accessory(candidate.id.vid, candidate.id.pid))
            .or_else(|| candidates.iter().find(|candidate| candidate.suggested))
            .map(|candidate| candidate.id.clone()),
        Err(error) => {
            eprintln!("cannot list devices: {error}");
            return 2;
        }
    };
    let Some(target) = target else {
        eprintln!("no USB device is attached");
        return 2;
    };
    let transport = match crate::usb::Usb::connect(&target) {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("cannot open the device: {error}");
            return 2;
        }
    };
    println!("dumping raw bytes for {seconds}s (up to 512 bytes)");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut buffer = [0u8; 512];
    let mut total = 0usize;
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        match transport.read_raw(left.min(Duration::from_millis(500)), &mut buffer) {
            Ok(0) => continue,
            Ok(count) => {
                total += count;
                let hex: Vec<String> = buffer[..count].iter().map(|b| format!("{b:02x}")).collect();
                println!("{count} bytes: {}", hex.join(" "));
                if total >= 512 {
                    break;
                }
            }
            Err(error) => {
                eprintln!("read failed: {error}");
                return 2;
            }
        }
    }
    println!("total {total} bytes");
    if total == 0 { 1 } else { 0 }
}

/// Ask the phone to start a fresh authorization exchange.
///
/// One frame, no payload, no identity: it says "a host is here". A phone whose accessory
/// session is already spent answers it by ending that session and issuing a new challenge
/// (`HOST_HELLO` in `wire.rs`, and the rule is in `protocol/client-authorization-v2.md`).
pub fn reset_exchange(transport: &mut dyn Transport, budget: Duration) -> crate::Result<()> {
    transport.send(&Frame::new(wire::HOST_HELLO, 0, Vec::new())?, budget)
}

/// Does this failure describe a phone whose accessory session is already spent?
///
/// Only these: a session that has authorised somebody is quiet (the host waits out its
/// whole budget) or still answering with the older connection's frames ("capabilities
/// before the outcome", or a stream that will not parse because the older reader still owns
/// the endpoint). A *rejected* outcome, a bad fingerprint, a missing cable or a Windows
/// driver are all answers, and repeating them costs the user time while telling them
/// nothing. Pure so the decision can be tested without a phone.
pub fn a_hello_would_help(error: &str) -> bool {
    let lowered = error.to_lowercase();
    // "receive poll expired" is the USB read path's own wording for the same thing: the
    // budget ran out with nothing (or nothing usable) arriving.
    lowered.contains("deadline expired")
        || lowered.contains("poll expired")
        || lowered.contains("incomplete frame")
        || lowered.contains("does not support required per-pc authorization")
        || lowered.contains("expected authorization outcome before")
}

/// Run the whole gate against a real phone: AOA transition, authorization, then the
/// synchronous exchange.
///
/// The AOA switch happens inside `Usb::connect`, so the transition the gate describes
/// (requests 51/52/53, re-enumeration as an accessory) is exercised by the shipping code
/// rather than re-implemented here. Authorization goes through `client_auth::exchange`
/// because the phone answers control messages only on an authorized session — a PING sent
/// earlier is read by the authorization gate, not by the control handler.
pub fn end_to_end(
    device: Option<crate::usb::DeviceId>,
    frames: u64,
    budget: Duration,
    on_waiting: impl Fn(&str),
) -> crate::Result<(Report, crate::usb::DeviceId)> {
    let target = match device {
        Some(id) => id,
        None => {
            // Prefer an accessory (already switched), else the phone in its normal mode.
            let candidates = crate::usb::list()?;
            let chosen = candidates
                .iter()
                .find(|candidate| crate::usb::accessory(candidate.id.vid, candidate.id.pid))
                .or_else(|| candidates.iter().find(|candidate| candidate.suggested))
                .or_else(|| candidates.first())
                .ok_or("no USB device is attached")?;
            chosen.id.clone()
        }
    };
    let before = target.clone();
    let mut transport = crate::usb::Usb::connect(&target)?;
    let after = crate::usb::switched_identity(&before).unwrap_or(before.clone());

    let identity = crate::identity::Identity::load_or_create()?;
    let client_id = identity
        .client_id()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let stop = std::sync::atomic::AtomicBool::new(false);
    // The whole attempt, authorization and the exchange together: a reader failure after
    // authorization is the same kind of spent-session symptom, and the retry has to cover it
    // (measured 2026-10-24: the retry wrapped the exchange only, so a failure two seconds
    // into the round trips was reported without a second attempt).
    let attempt = |transport: &mut crate::usb::Usb| -> crate::Result<Report> {
        match crate::client_auth::exchange(
            transport,
            &identity,
            crate::client_auth::Route::Usb,
            budget,
            &stop,
            || {
                on_waiting(&client_id);
                Ok(())
            },
        )? {
            crate::client_auth::Outcome::Authorized { .. } => {}
            crate::client_auth::Outcome::Rejected(reason) => return Err(reason.into()),
        }
        run(transport, frames, budget)
    };
    let mut outcome = attempt(&mut transport);
    if let Err(error) = &outcome
        && a_hello_would_help(&error.to_string())
    {
        // Self-heal instead of asking the user to restart the app: say hello, which ends a
        // spent session on the phone, then try the whole attempt once more.
        println!("retrying after a host hello: {error}");
        reset_exchange(&mut transport, Duration::from_secs(2))?;
        // A failed attempt can end mid-frame; a reader holding a partial header cannot
        // resynchronise on its own, so forget that state before listening again.
        if !transport.reset_stream()? {
            println!("warning: the stream did not show a frame boundary after the reset");
        }
        outcome = attempt(&mut transport);
    }
    let report = outcome?;

    Ok((report, after))
}

/// Run `frames` PING/PONG exchanges on an already-authorised transport.
pub fn run(transport: &mut dyn Transport, frames: u64, budget: Duration) -> crate::Result<Report> {
    let mut report = Report::default();
    for index in 0..frames {
        let sequence = (index as u32).wrapping_add(1);
        let challenge = challenge(sequence);
        let mut frame = Frame::new(wire::PING, sequence, challenge.to_vec())?;
        // Sequence and PTS both travel; the app echoes them, which is what makes a lost
        // frame visible instead of merely slow.
        frame.header.pts_us = sequence as u64;
        transport.send(&frame, budget)?;
        report.sent += 1;

        let started = Instant::now();
        let deadline = started + budget;
        let reply = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break None;
            }
            match transport.poll_receive(left)? {
                // Anything that is not a reply to this PING is traffic the probe is not
                // waiting for — the phone advertises its capabilities right after
                // authorizing. Counting them and continuing is what the shipping session
                // does; treating one as the reply makes every exchange look mismatched.
                Some(candidate) if candidate.header.kind != wire::PONG => {
                    report.skipped += 1;
                    continue;
                }
                Some(candidate) => break Some(candidate),
                None => continue,
            }
        };
        match classify(sequence, &challenge, reply.as_ref()) {
            Verdict::Echoed => {
                report.echoed += 1;
                report.rtts_us.push(started.elapsed().as_micros() as u64);
            }
            Verdict::SequenceMismatch => report.sequence_mismatch += 1,
            Verdict::PayloadMismatch => report.payload_mismatch += 1,
            Verdict::TimedOut => report.timed_out += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A transport that answers PINGs the way the phone does — or wrongly, on request.
    struct Fake {
        replies: VecDeque<Frame>,
        sent: Vec<u32>,
        break_sequence: bool,
        break_payload: bool,
    }

    impl Transport for Fake {
        fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
            self.poll_receive(timeout)?.ok_or_else(|| "no reply".into())
        }
        fn send(&mut self, frame: &Frame, _timeout: Duration) -> crate::Result<()> {
            self.sent.push(frame.header.sequence);
            let sequence = frame.header.sequence;
            let payload = if self.break_payload {
                vec![0u8; 8]
            } else {
                frame.payload.clone()
            };
            let sequence = if self.break_sequence {
                sequence + 1
            } else {
                sequence
            };
            self.replies
                .push_back(Frame::new(wire::PONG, sequence, payload).unwrap());
            Ok(())
        }
        fn poll_receive(&mut self, _timeout: Duration) -> crate::Result<Option<Frame>> {
            Ok(self.replies.pop_front())
        }
    }

    fn fake(break_sequence: bool, break_payload: bool) -> Fake {
        Fake {
            replies: VecDeque::new(),
            sent: Vec::new(),
            break_sequence,
            break_payload,
        }
    }

    #[test]
    fn only_a_spent_session_is_worth_a_hello() {
        // The failures a hello can reset: a session that authorised somebody is quiet, or
        // still answering with the older connection's frames.
        assert!(a_hello_would_help("receive poll expired"));
        assert!(a_hello_would_help("deadline expired"));
        assert!(a_hello_would_help("incomplete frame deadline expired"));
        assert!(a_hello_would_help(
            "phone does not support required per-PC authorization v2"
        ));
        assert!(a_hello_would_help(
            "expected authorization outcome before capabilities/media"
        ));
        // Answers, not symptoms: repeating them costs the user time and tells them nothing.
        assert!(!a_hello_would_help(
            "the phone rejected this PC's signature"
        ));
        assert!(!a_hello_would_help(
            "this PC's access was revoked on the phone"
        ));
        assert!(!a_hello_would_help("connection refused"));
        assert!(!a_hello_would_help(
            "Wi-Fi phone certificate fingerprint mismatch"
        ));
        assert!(!a_hello_would_help("WinUSB access unavailable"));
    }

    #[test]
    fn a_clean_exchange_passes_and_reports_percentiles() {
        let mut transport = fake(false, false);
        let report = run(&mut transport, 20, Duration::from_millis(50)).unwrap();
        assert!(report.pass(20), "{report:?}");
        assert_eq!(report.sent, 20);
        assert_eq!(transport.sent, (1..=20).collect::<Vec<_>>());
        let (min, avg, p50, p95, max) = report.rtt_percentiles().unwrap();
        assert!(
            min <= p50 && p50 <= p95 && p95 <= max,
            "percentiles must be ordered"
        );
        assert!(avg >= min && avg <= max);
        assert!(report.summary(20).contains("PASS"));
    }

    #[test]
    fn a_lost_sequence_fails_instead_of_being_averaged_away() {
        // The gate says "sequence matches on every exchange", so one mismatch must fail the
        // run: a mean RTT cannot express a framing error.
        let mut transport = fake(true, false);
        let report = run(&mut transport, 5, Duration::from_millis(50)).unwrap();
        assert_eq!(report.sequence_mismatch, 5);
        assert!(!report.pass(5));
        assert!(report.summary(5).contains("FAIL"));
    }

    #[test]
    fn a_changed_payload_is_its_own_failure() {
        let mut transport = fake(false, true);
        let report = run(&mut transport, 5, Duration::from_millis(50)).unwrap();
        assert_eq!(report.payload_mismatch, 5);
        assert_eq!(report.sequence_mismatch, 0, "the sequence was fine");
        assert!(!report.pass(5));
    }

    #[test]
    fn a_silent_peer_times_out_every_frame_and_still_reports() {
        // A transport that never answers: the probe must terminate, not hang, and must not
        // report zero-of-zero as a pass.
        struct Silent;
        impl Transport for Silent {
            fn receive(&mut self, _timeout: Duration) -> crate::Result<Frame> {
                Err("no reply".into())
            }
            fn send(&mut self, _frame: &Frame, _timeout: Duration) -> crate::Result<()> {
                Ok(())
            }
            fn poll_receive(&mut self, _timeout: Duration) -> crate::Result<Option<Frame>> {
                Ok(None)
            }
        }
        let mut transport = Silent;
        let report = run(&mut transport, 3, Duration::from_millis(10)).unwrap();
        assert_eq!(report.timed_out, 3);
        assert_eq!(report.echoed, 0);
        assert!(!report.pass(3));
        assert!(report.rtt_percentiles().is_none());
    }

    #[test]
    fn a_pong_for_another_sequence_is_not_an_echo() {
        let challenge = challenge(7);
        let wrong = Frame::new(wire::PONG, 9, challenge.to_vec()).unwrap();
        assert_eq!(
            classify(7, &challenge, Some(&wrong)),
            Verdict::SequenceMismatch
        );
        let right = Frame::new(wire::PONG, 7, challenge.to_vec()).unwrap();
        assert_eq!(classify(7, &challenge, Some(&right)), Verdict::Echoed);
        // A PING answered with a PING is not a reply.
        let ping = Frame::new(wire::PING, 7, challenge.to_vec()).unwrap();
        assert_eq!(
            classify(7, &challenge, Some(&ping)),
            Verdict::SequenceMismatch
        );
        assert_eq!(classify(7, &challenge, None), Verdict::TimedOut);
    }

    #[test]
    fn the_challenge_is_deterministic_and_distinct_per_sequence() {
        assert_eq!(challenge(1), challenge(1));
        assert_ne!(challenge(1), challenge(2));
        // And never a single repeated byte, so a payload that arrives zeroed is visible.
        for sequence in [1u32, 2, 0xffff_ffff] {
            let bytes = challenge(sequence);
            assert!(bytes.iter().any(|byte| *byte != 0), "{bytes:?}");
        }
    }
}
