//! Strict test-only fixture interpreter. Bad lines must never disappear.
use super::{Action, Event, Switch};
fn number<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value.parse().expect("invalid fixture number")
}
fn event(text: &str) -> Event {
    let words: Vec<_> = text.split_whitespace().collect();
    match words.as_slice() {
        ["epoch-observed", epoch] => Event::EpochObserved(number(epoch)),
        ["prepare", revision, w, h, f] => Event::Prepare {
            revision: number(revision),
            width: number(w),
            height: number(h),
            fps: number(f),
        },
        ["prepare-accepted", id, streaming] => Event::PrepareAccepted {
            request_id: number(id),
            streaming: match *streaming {
                "0" => false,
                "1" => true,
                _ => panic!("invalid fixture boolean"),
            },
        },
        ["rejected", id, reason] => Event::Rejected {
            request_id: number(id),
            reason: number(reason),
        },
        ["paused", id, epoch] => Event::Paused {
            request_id: number(id),
            epoch: number(epoch),
        },
        ["video-config", w, h, f] => Event::VideoConfig {
            width: number(w),
            height: number(h),
            fps: number(f),
        },
        ["capture-started", id, w, h, f] => Event::CaptureStarted {
            request_id: number(id),
            width: number(w),
            height: number(h),
            fps: number(f),
        },
        ["output-ready"] => Event::OutputReady,
        ["deadline-exceeded", time] => {
            let _: u64 = number(time);
            Event::DeadlineExceeded
        }
        ["user-stop"] => Event::UserStop,
        _ => panic!("invalid fixture event: {text}"),
    }
}
fn reason(label: &str) -> &'static str {
    match label {
        "permission" => "camera permission is not granted",
        "unsupported-mode" => "the mode is not supported by the phone",
        "deadline" => "the apply deadline expired",
        "edition" => "the mode exceeds this edition's quality cap",
        "mismatch" => "the new mode did not match the request",
        "stopped" => "the apply was stopped",
        _ => panic!("invalid fixture reason: {label}"),
    }
}
fn action(text: &str) -> Action {
    let words: Vec<_> = text.split_whitespace().collect();
    match words.as_slice() {
        ["send-prepare-quality", id, r, w, h, f] => Action::SendPrepareQuality {
            request_id: number(id),
            revision: number(r),
            width: number(w),
            height: number(h),
            fps: number(f),
        },
        ["send-commit-quality", id, epoch] => Action::SendCommitQuality {
            request_id: number(id),
            epoch: number(epoch),
        },
        ["release-outputs"] => Action::ReleaseOutputs,
        ["report-applied", w, h, f] => Action::ReportApplied {
            width: number(w),
            height: number(h),
            fps: number(f),
        },
        ["report-failed", label] => Action::ReportFailed {
            reason: reason(label),
        },
        ["stop-capture"] => Action::StopCapture,
        ["close-connection"] => Action::CloseConnection,
        _ => panic!("invalid fixture action: {text}"),
    }
}
pub(super) fn run_fixture(script: &str) {
    let mut machine = Switch::default();
    let mut now = 0;
    let mut count = 0;
    for line in script.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let (input, expected) = line.split_once("->").expect("fixture arrow is missing");
        let parsed = event(input.trim());
        if parsed == Event::DeadlineExceeded {
            let next: u64 = number(input.split_whitespace().nth(1).unwrap());
            assert!(next >= now, "fixture time moved backward");
            now = next;
        }
        let expected = if expected.trim() == "-" {
            Vec::new()
        } else {
            expected
                .split(';')
                .map(|text| action(text.trim()))
                .collect()
        };
        assert_eq!(machine.step(now, parsed), expected, "fixture line: {line}");
        count += 1;
    }
    assert!(count > 0, "empty fixture");
}
#[test]
fn malformed_fixtures_fail_instead_of_skipping_assertions() {
    for bad in [
        "prepare 7 typo 720 30 -> -",
        "prepare 7 999999 720 30 -> -",
        "output-ready extra -> -",
        "unknown -> -",
        "output-ready -> unknown",
        "output-ready -",
        "# empty",
    ] {
        assert!(
            std::panic::catch_unwind(|| run_fixture(bad)).is_err(),
            "accepted malformed fixture: {bad}"
        );
    }
}
