//! The PC-side quality-switch controller: a pure event machine that owns the
//! apply request and the video epoch. Every transition emits actions; the
//! fixture script drives both implementations with the same events and the
//! same expected action trace.
//!
//! The apply contract (specs/video-quality-controls/slices/02-control.md):
//! prepare -> (rejected: keep streaming, report) -> switching: the phone
//! closes capture and returns Paused -> the worker releases outputs and
//! commits the new epoch -> the new-epoch VIDEO_CONFIG is verified and the
//! apply is reported. One apply in flight; stale requests, revisions and
//! responses are ignored; the 5-second deadline fails the apply by stopping
//! capture and closing — never by rolling back.

/// The whole apply is bounded; Stop keeps its existing one-second deadline.
pub const APPLY_DEADLINE_MS: u64 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Preparing,
    /// The phone staged the mode while nothing was streaming; the user's
    /// camera start (not a timer) completes it.
    Staged,
    Switching,
    AwaitingConfig,
    Applied,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Begin an apply (the GUI/SessionController entry point).
    Prepare {
        revision: u16,
        width: u16,
        height: u16,
        fps: u16,
    },
    /// QualityResult with status switching: the phone accepted the prepare.
    /// `streaming` distinguishes a live switch (Paused follows) from a
    /// staged mode that waits for the user's camera start.
    PrepareAccepted { request_id: u32, streaming: bool },
    /// QualityResult with status rejected and a reason code.
    Rejected { request_id: u32, reason: u8 },
    /// The phone reached the switch boundary; `epoch` is the ending epoch.
    Paused { request_id: u32, epoch: u32 },
    /// A VIDEO_CONFIG observed while an apply is awaiting its confirmation.
    VideoConfig { width: u16, height: u16 },
    /// The apply deadline expired.
    DeadlineExceeded,
    /// The user stopped the session mid-apply.
    UserStop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SendPrepareQuality {
        request_id: u32,
        revision: u16,
        width: u16,
        height: u16,
        fps: u16,
    },
    SendCommitQuality {
        request_id: u32,
        epoch: u32,
    },
    /// Free the decoder, conversion buffers and virtual-camera output.
    ReleaseOutputs,
    ReportApplied {
        width: u16,
        height: u16,
        fps: u16,
    },
    ReportFailed {
        reason: &'static str,
    },
    StopCapture,
    CloseConnection,
}

#[derive(Clone, Debug)]
pub struct Switch {
    phase: Phase,
    request_id: u32,
    revision: u16,
    epoch: u32,
    requested: Option<(u16, u16, u16)>,
    applied: Option<(u16, u16, u16)>,
    error: Option<&'static str>,
    deadline_ms: Option<u64>,
    next_request_id: u32,
}

impl Default for Switch {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            request_id: 0,
            revision: 0,
            epoch: 0,
            requested: None,
            applied: None,
            error: None,
            deadline_ms: None,
            next_request_id: 1,
        }
    }
}

impl Switch {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Begin an apply. Returns the prepare action, or None when an apply is
    /// already in flight (one at a time) or nothing changed.
    pub fn prepare(
        &mut self,
        now_ms: u64,
        revision: u16,
        width: u16,
        height: u16,
        fps: u16,
    ) -> Option<Action> {
        if !matches!(self.phase, Phase::Idle | Phase::Applied | Phase::Failed) {
            return None;
        }
        if self.applied == Some((width, height, fps)) {
            return None;
        }
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.phase = Phase::Preparing;
        self.request_id = request_id;
        self.revision = revision;
        self.requested = Some((width, height, fps));
        self.error = None;
        self.deadline_ms = Some(now_ms + APPLY_DEADLINE_MS);
        Some(Action::SendPrepareQuality {
            request_id,
            revision,
            width,
            height,
            fps,
        })
    }

    /// Drive the machine with one event; returns the actions in order.
    pub fn step(&mut self, now_ms: u64, event: Event) -> Vec<Action> {
        match event {
            Event::Prepare {
                revision,
                width,
                height,
                fps,
            } => self
                .prepare(now_ms, revision, width, height, fps)
                .into_iter()
                .collect(),
            Event::PrepareAccepted {
                request_id,
                streaming,
            } => {
                if request_id != self.request_id || self.phase != Phase::Preparing {
                    return Vec::new();
                }
                if streaming {
                    // The boundary follows; the deadline keeps running.
                    Vec::new()
                } else {
                    // Nothing streams: the mode is staged for the user's
                    // camera start. No timer can expire a human's wait.
                    self.phase = Phase::Staged;
                    self.deadline_ms = None;
                    Vec::new()
                }
            }
            Event::Rejected { request_id, reason } => {
                if request_id != self.request_id
                    || !matches!(self.phase, Phase::Preparing | Phase::Switching)
                {
                    return Vec::new();
                }
                self.phase = Phase::Failed;
                self.deadline_ms = None;
                self.error = Some(rejection_reason(reason));
                // Rejection keeps the streaming mode; no capture stop.
                vec![Action::ReportFailed {
                    reason: rejection_reason(reason),
                }]
            }
            Event::Paused { request_id, epoch } => {
                if request_id != self.request_id || self.phase != Phase::Preparing {
                    return Vec::new();
                }
                self.phase = Phase::Switching;
                self.epoch = epoch + 1;
                vec![
                    Action::ReleaseOutputs,
                    Action::SendCommitQuality {
                        request_id,
                        epoch: self.epoch,
                    },
                ]
            }
            Event::VideoConfig { width, height } => {
                if !matches!(
                    self.phase,
                    Phase::Switching | Phase::AwaitingConfig | Phase::Staged
                ) {
                    return Vec::new();
                }
                let Some((want_width, want_height, want_fps)) = self.requested else {
                    return Vec::new();
                };
                if width != want_width || height != want_height {
                    self.phase = Phase::Failed;
                    self.deadline_ms = None;
                    self.error = Some("the new mode did not match the request");
                    return vec![
                        Action::ReportFailed {
                            reason: "the new mode did not match the request",
                        },
                        Action::StopCapture,
                        Action::CloseConnection,
                    ];
                }
                self.phase = Phase::Applied;
                self.deadline_ms = None;
                self.applied = Some((want_width, want_height, want_fps));
                vec![Action::ReportApplied {
                    width,
                    height,
                    fps: want_fps,
                }]
            }
            Event::DeadlineExceeded => {
                let expired = self.deadline_ms.is_some_and(|deadline| now_ms >= deadline);
                if !expired
                    || !matches!(
                        self.phase,
                        Phase::Preparing | Phase::Switching | Phase::AwaitingConfig
                    )
                {
                    return Vec::new();
                }
                self.phase = Phase::Failed;
                self.deadline_ms = None;
                self.error = Some("the apply deadline expired");
                vec![
                    Action::ReportFailed {
                        reason: "the apply deadline expired",
                    },
                    Action::StopCapture,
                    Action::CloseConnection,
                ]
            }
            Event::UserStop => {
                if !matches!(
                    self.phase,
                    Phase::Preparing | Phase::Switching | Phase::AwaitingConfig
                ) {
                    return Vec::new();
                }
                self.phase = Phase::Failed;
                self.deadline_ms = None;
                self.error = Some("the apply was stopped");
                vec![
                    Action::ReportFailed {
                        reason: "the apply was stopped",
                    },
                    Action::StopCapture,
                ]
            }
        }
    }

    /// True while an apply is in flight (one at a time).
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Preparing | Phase::Switching | Phase::AwaitingConfig
        )
    }

    pub fn staged(&self) -> bool {
        self.phase == Phase::Staged
    }
}

/// Fixture reason names, so the script can pin the exact failure text.
fn reason_name(name: Option<&str>) -> &'static str {
    match name {
        Some("permission") => "camera permission is not granted",
        Some("foreground") => "the phone left the foreground",
        Some("unsupported-mode") => "the mode is not supported by the phone",
        Some("stale-revision") => "the capability list changed; re-pair the quality list",
        Some("deadline") => "the apply deadline expired",
        Some("mismatch") => "the new mode did not match the request",
        Some("stopped") => "the apply was stopped",
        _ => "the phone rejected the apply",
    }
}

fn rejection_reason(reason: u8) -> &'static str {
    match reason {
        1 => "camera permission is not granted",
        2 => "the phone left the foreground",
        3 => "the mode is not supported by the phone",
        4 => "the capability list changed; re-pair the quality list",
        _ => "the phone rejected the apply",
    }
}

/// Parse one fixture line into (event, expected actions). The fixture format
/// is shared with the Kotlin implementation.
pub fn parse_fixture_line(line: &str) -> Option<(Event, Vec<Action>)> {
    let line = line.split('#').next()?.trim();
    if line.is_empty() {
        return None;
    }
    let (event_text, actions_text) = line.split_once("->")?;
    let event = parse_event(event_text.trim())?;
    let actions = actions_text
        .split(';')
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(parse_action)
        .collect::<Option<Vec<_>>>()?;
    Some((event, actions))
}

fn parse_event(text: &str) -> Option<Event> {
    let mut parts = text.split_whitespace();
    let name = parts.next()?;
    let numbers: Vec<u64> = parts.filter_map(|part| part.parse().ok()).collect();
    Some(match name {
        "prepare" => Event::Prepare {
            revision: *numbers.first()? as u16,
            width: *numbers.get(1)? as u16,
            height: *numbers.get(2)? as u16,
            fps: *numbers.get(3)? as u16,
        },
        "prepare-accepted" => Event::PrepareAccepted {
            request_id: *numbers.first()? as u32,
            streaming: *numbers.get(1)? != 0,
        },
        "rejected" => Event::Rejected {
            request_id: *numbers.first()? as u32,
            reason: *numbers.get(1)? as u8,
        },
        "paused" => Event::Paused {
            request_id: *numbers.first()? as u32,
            epoch: *numbers.get(1)? as u32,
        },
        "video-config" => Event::VideoConfig {
            width: *numbers.first()? as u16,
            height: *numbers.get(1)? as u16,
        },
        "deadline-exceeded" => Event::DeadlineExceeded,
        "user-stop" => Event::UserStop,
        _ => return None,
    })
}

fn parse_action(text: &str) -> Option<Action> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let name = *words.first()?;
    let numbers: Vec<u64> = words
        .iter()
        .skip(1)
        .filter_map(|part| part.parse().ok())
        .collect();
    let label = words.get(1).copied();
    Some(match name {
        "send-prepare-quality" => Action::SendPrepareQuality {
            request_id: *numbers.first()? as u32,
            revision: *numbers.get(1)? as u16,
            width: *numbers.get(2)? as u16,
            height: *numbers.get(3)? as u16,
            fps: *numbers.get(4)? as u16,
        },
        "send-commit-quality" => Action::SendCommitQuality {
            request_id: *numbers.first()? as u32,
            epoch: *numbers.get(1)? as u32,
        },
        "release-outputs" => Action::ReleaseOutputs,
        "report-applied" => Action::ReportApplied {
            width: *numbers.first()? as u16,
            height: *numbers.get(1)? as u16,
            fps: *numbers.get(2)? as u16,
        },
        "report-failed" => Action::ReportFailed {
            reason: reason_name(label),
        },
        "stop-capture" => Action::StopCapture,
        "close-connection" => Action::CloseConnection,
        _ => return None,
    })
}

/// Run a fixture script against a fresh controller and return the action
/// trace. The prepare line's leading `prepare` word is expanded by the
/// machine, so the script drives `prepare` through its own event spelling.
pub fn run_fixture(script: &str) -> Vec<Vec<Action>> {
    let mut machine = Switch::default();
    let mut trace = Vec::new();
    let mut now_ms = 0u64;
    for line in script.lines() {
        let Some((event, expected)) = parse_fixture_line(line) else {
            continue;
        };
        // A deadline line carries the current time: deadline-exceeded 5000.
        if line.contains("deadline-exceeded") {
            now_ms = line
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse().ok())
                .unwrap_or(now_ms + APPLY_DEADLINE_MS);
        }
        let actions = machine.step(now_ms, event.clone());
        assert_eq!(actions, expected, "fixture line mismatch: {line}");
        trace.push(actions);
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        include_str!("../../protocol/fixtures/quality-switch-pc-v1.txt")
    }

    #[test]
    fn fixture_script_drives_the_expected_action_trace() {
        // The script itself asserts each step; a mismatch panics with the line.
        let trace = run_fixture(fixture());
        assert!(!trace.is_empty());
    }

    #[test]
    fn one_apply_in_flight_and_stale_events_ignored() {
        let mut machine = Switch::default();
        let prepare = machine.prepare(0, 7, 1280, 720, 30).unwrap();
        assert!(matches!(
            prepare,
            Action::SendPrepareQuality { request_id: 1, .. }
        ));
        // A second prepare while busy emits nothing.
        assert!(machine.prepare(10, 7, 1920, 1080, 30).is_none());
        // A stale request id never moves the machine.
        assert!(
            machine
                .step(
                    10,
                    Event::Paused {
                        request_id: 99,
                        epoch: 41
                    }
                )
                .is_empty()
        );
        assert!(
            machine
                .step(
                    10,
                    Event::Rejected {
                        request_id: 99,
                        reason: 3
                    }
                )
                .is_empty()
        );
        assert!(
            machine
                .step(
                    10,
                    Event::PrepareAccepted {
                        request_id: 99,
                        streaming: true,
                    }
                )
                .is_empty()
        );
        // The real boundary releases outputs and commits the next epoch.
        let actions = machine.step(
            10,
            Event::Paused {
                request_id: 1,
                epoch: 41,
            },
        );
        assert_eq!(
            actions,
            vec![
                Action::ReleaseOutputs,
                Action::SendCommitQuality {
                    request_id: 1,
                    epoch: 42
                },
            ]
        );
        // A duplicate Paused for the same request does nothing.
        assert!(
            machine
                .step(
                    11,
                    Event::Paused {
                        request_id: 1,
                        epoch: 41
                    }
                )
                .is_empty()
        );
        // The matching config completes the apply.
        assert_eq!(
            machine.step(
                12,
                Event::VideoConfig {
                    width: 1280,
                    height: 720
                }
            ),
            vec![Action::ReportApplied {
                width: 1280,
                height: 720,
                fps: 30
            }]
        );
        assert_eq!(machine.phase(), Phase::Applied);
        // The same mode requested again is a no-op prepare.
        assert!(machine.prepare(20, 7, 1280, 720, 30).is_none());
    }

    #[test]
    fn rejection_keeps_streaming_without_stopping_capture() {
        let mut machine = Switch::default();
        machine.prepare(0, 7, 1920, 1080, 30).unwrap();
        let actions = machine.step(
            5,
            Event::Rejected {
                request_id: 1,
                reason: 1,
            },
        );
        assert_eq!(
            actions,
            vec![Action::ReportFailed {
                reason: "camera permission is not granted"
            }]
        );
        assert!(!actions.contains(&Action::StopCapture));
        // A later prepare is allowed again.
        assert!(machine.prepare(10, 7, 1920, 1080, 30).is_some());
    }

    #[test]
    fn deadline_expiry_stops_capture_and_closes_without_rollback() {
        let mut machine = Switch::default();
        machine.prepare(0, 7, 1920, 1080, 30).unwrap();
        assert!(machine.step(4_000, Event::DeadlineExceeded).is_empty());
        let actions = machine.step(5_000, Event::DeadlineExceeded);
        assert_eq!(
            actions,
            vec![
                Action::ReportFailed {
                    reason: "the apply deadline expired"
                },
                Action::StopCapture,
                Action::CloseConnection,
            ]
        );
        assert_eq!(machine.phase(), Phase::Failed);
    }

    #[test]
    fn mismatched_new_mode_fails_the_apply() {
        let mut machine = Switch::default();
        machine.prepare(0, 7, 1280, 720, 30).unwrap();
        machine.step(
            1,
            Event::Paused {
                request_id: 1,
                epoch: 41,
            },
        );
        let actions = machine.step(
            2,
            Event::VideoConfig {
                width: 1920,
                height: 1080,
            },
        );
        assert_eq!(
            actions,
            vec![
                Action::ReportFailed {
                    reason: "the new mode did not match the request"
                },
                Action::StopCapture,
                Action::CloseConnection,
            ]
        );
    }
}

#[cfg(test)]
mod staged_tests {
    use super::*;

    #[test]
    fn staged_mode_waits_for_the_user_without_a_deadline() {
        let mut machine = Switch::default();
        machine.step(
            0,
            Event::Prepare {
                revision: 7,
                width: 1920,
                height: 1080,
                fps: 30,
            },
        );
        // Nothing streams: the phone stages the mode.
        assert!(
            machine
                .step(
                    10,
                    Event::PrepareAccepted {
                        request_id: 1,
                        streaming: false
                    }
                )
                .is_empty()
        );
        assert!(machine.staged());
        // Time alone never fails a staged apply.
        assert!(machine.step(600_000, Event::DeadlineExceeded).is_empty());
        // The user starts the camera; the new-epoch config completes it.
        assert_eq!(
            machine.step(
                600_001,
                Event::VideoConfig {
                    width: 1920,
                    height: 1080
                }
            ),
            vec![Action::ReportApplied {
                width: 1920,
                height: 1080,
                fps: 30
            }]
        );
    }
}
