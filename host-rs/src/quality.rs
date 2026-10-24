//! PC quality-switch state. Metadata is necessary, not sufficient: Applied
//! follows a matching size/rate config and a decoded, submitted keyframe.
//! Human-staged requests have no timer until capture actually starts.
use serde::{Deserialize, Serialize};
/// One advertised capture mode from the phone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mode {
    pub width: u16,
    pub height: u16,
    pub fps: u16,
    pub bitrate: u32,
}

impl From<crate::wire::QualityMode> for Mode {
    fn from(mode: crate::wire::QualityMode) -> Self {
        Self {
            width: mode.width,
            height: mode.height,
            fps: mode.target_fps,
            bitrate: mode.bitrate,
        }
    }
}
mod preference;
pub use preference::Wish;
mod client;
pub use client::Client;
mod edition;
pub use edition::{
    EditionCap, OSS_CAP_1080P30, active as active_edition, coerce_wish_to_available,
    filter_oss_cap_1080p30, oss_cap_1080p30_allows,
};
pub const APPLY_DEADLINE_MS: u64 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Preparing,
    Staged,
    Switching,
    AwaitingOutput,
    Applied,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    EpochObserved(u32),
    Prepare {
        revision: u16,
        width: u16,
        height: u16,
        fps: u16,
    },
    PrepareAccepted {
        request_id: u32,
        streaming: bool,
    },
    Rejected {
        request_id: u32,
        reason: u8,
    },
    Paused {
        request_id: u32,
        epoch: u32,
    },
    VideoConfig {
        width: u16,
        height: u16,
        fps: u16,
    },
    CaptureStarted {
        request_id: u32,
        width: u16,
        height: u16,
        fps: u16,
    },
    OutputReady,
    DeadlineExceeded,
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
    epoch: Option<u32>,
    requested: Option<(u16, u16, u16)>,
    applied: Option<(u16, u16, u16)>,
    deadline_ms: Option<u64>,
    next_request_id: u32,
}
impl Default for Switch {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            request_id: 0,
            epoch: None,
            requested: None,
            applied: None,
            deadline_ms: None,
            next_request_id: 1,
        }
    }
}
impl Switch {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Preparing | Phase::Switching | Phase::AwaitingOutput
        )
    }
    pub fn staged(&self) -> bool {
        self.phase == Phase::Staged
    }
    pub fn awaiting_video(&self) -> bool {
        matches!(
            self.phase,
            Phase::Switching | Phase::Staged | Phase::AwaitingOutput
        )
    }
    pub fn prepare(
        &mut self,
        now_ms: u64,
        revision: u16,
        width: u16,
        height: u16,
        fps: u16,
    ) -> Option<Action> {
        if !matches!(self.phase, Phase::Idle | Phase::Applied | Phase::Failed)
            || width == 0
            || height == 0
            || fps == 0
        {
            return None;
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.checked_add(1)?;
        self.phase = Phase::Preparing;
        self.request_id = request_id;
        self.requested = Some((width, height, fps));
        self.deadline_ms = Some(now_ms + APPLY_DEADLINE_MS);
        Some(Action::SendPrepareQuality {
            request_id,
            revision,
            width,
            height,
            fps,
        })
    }
    fn fail(&mut self, reason: &'static str) -> Vec<Action> {
        self.phase = Phase::Failed;
        self.deadline_ms = None;
        vec![
            Action::ReportFailed { reason },
            Action::StopCapture,
            Action::CloseConnection,
        ]
    }
    pub fn step(&mut self, now_ms: u64, event: Event) -> Vec<Action> {
        match event {
            Event::EpochObserved(epoch) => {
                if matches!(self.phase, Phase::Idle | Phase::Applied | Phase::Failed) {
                    self.epoch = Some(epoch);
                }
                Vec::new()
            }
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
                if request_id != self.request_id {
                    return Vec::new();
                }
                if self.phase == Phase::Preparing && !streaming {
                    self.phase = Phase::Staged;
                    self.deadline_ms = None;
                    return vec![Action::ReleaseOutputs];
                }
                if self.phase == Phase::Staged && streaming {
                    self.phase = Phase::Switching;
                    self.deadline_ms = Some(now_ms + APPLY_DEADLINE_MS);
                }
                Vec::new()
            }
            Event::Rejected { request_id, reason } => {
                if request_id != self.request_id
                    || !matches!(
                        self.phase,
                        Phase::Preparing | Phase::Staged | Phase::Switching | Phase::AwaitingOutput
                    )
                {
                    return Vec::new();
                }
                let after_boundary = matches!(self.phase, Phase::Switching | Phase::AwaitingOutput);
                if after_boundary {
                    return self.fail(rejection_reason(reason));
                }
                self.phase = Phase::Failed;
                self.deadline_ms = None;
                vec![Action::ReportFailed {
                    reason: rejection_reason(reason),
                }]
            }
            Event::Paused { request_id, epoch } => {
                if request_id != self.request_id
                    || self.phase != Phase::Preparing
                    || self.epoch.is_some_and(|current| current != epoch)
                {
                    return Vec::new();
                }
                let Some(next) = epoch.checked_add(1) else {
                    return self.fail("the video epoch overflowed");
                };
                self.phase = Phase::Switching;
                self.epoch = Some(next);
                vec![
                    Action::ReleaseOutputs,
                    Action::SendCommitQuality {
                        request_id,
                        epoch: next,
                    },
                ]
            }
            Event::VideoConfig { width, height, fps } => {
                if !self.awaiting_video() {
                    return Vec::new();
                }
                if self.requested != Some((width, height, fps)) {
                    return self.fail("the new mode did not match the request");
                }
                if self.phase == Phase::Staged {
                    self.deadline_ms = Some(now_ms + APPLY_DEADLINE_MS);
                }
                self.phase = Phase::AwaitingOutput;
                Vec::new()
            }
            Event::CaptureStarted {
                request_id,
                width,
                height,
                fps,
            } => {
                if request_id != self.request_id || !self.awaiting_video() {
                    return Vec::new();
                }
                if self.requested != Some((width, height, fps)) {
                    return self.fail("the new mode did not match the request");
                }
                if self.phase == Phase::Staged {
                    self.phase = Phase::Switching;
                    self.deadline_ms = Some(now_ms + APPLY_DEADLINE_MS);
                }
                Vec::new()
            }
            Event::OutputReady => {
                if self.phase != Phase::AwaitingOutput {
                    return Vec::new();
                }
                let Some((width, height, fps)) = self.requested else {
                    return Vec::new();
                };
                self.phase = Phase::Applied;
                self.applied = self.requested;
                self.deadline_ms = None;
                vec![Action::ReportApplied { width, height, fps }]
            }
            Event::DeadlineExceeded => {
                if self.busy() && self.deadline_ms.is_some_and(|deadline| now_ms >= deadline) {
                    self.fail("the apply deadline expired")
                } else {
                    Vec::new()
                }
            }
            Event::UserStop => {
                if !self.busy() && !self.staged() {
                    return Vec::new();
                }
                self.phase = Phase::Failed;
                self.deadline_ms = None;
                vec![
                    Action::ReportFailed {
                        reason: "the apply was stopped",
                    },
                    Action::StopCapture,
                ]
            }
        }
    }
}
fn rejection_reason(reason: u8) -> &'static str {
    match reason {
        1 => "camera permission is not granted",
        2 => "the phone left the foreground",
        3 => "the mode is not supported by the phone",
        4 => "the capability list changed; refresh the quality list",
        5 => "another quality apply is already in flight",
        6 => "the apply deadline expired",
        7 => "camera capture could not start",
        8 => "this edition does not allow that quality",
        _ => "the phone rejected the apply",
    }
}

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    fn waiting() -> Switch {
        let mut machine = Switch::default();
        machine.step(0, Event::EpochObserved(41));
        assert!(machine.prepare(0, 7, 1280, 720, 30).is_some());
        assert_eq!(
            machine.step(
                10,
                Event::Paused {
                    request_id: 1,
                    epoch: 41
                }
            ),
            vec![
                Action::ReleaseOutputs,
                Action::SendCommitQuality {
                    request_id: 1,
                    epoch: 42
                }
            ]
        );
        machine
    }
    #[test]
    fn fixture_script_drives_the_expected_action_trace() {
        fixture::run_fixture(include_str!(
            "../../protocol/fixtures/quality-switch-pc-v1.txt"
        ));
    }
    #[test]
    fn metadata_waits_for_output_and_one_apply_remains_in_flight() {
        let mut machine = waiting();
        assert!(machine.prepare(11, 7, 1920, 1080, 30).is_none());
        assert!(
            machine
                .step(
                    12,
                    Event::VideoConfig {
                        width: 1280,
                        height: 720,
                        fps: 30
                    }
                )
                .is_empty()
        );
        assert_eq!(machine.phase(), Phase::AwaitingOutput);
        assert!(machine.prepare(13, 7, 1920, 1080, 30).is_none());
        assert_eq!(
            machine.step(14, Event::OutputReady),
            vec![Action::ReportApplied {
                width: 1280,
                height: 720,
                fps: 30
            }]
        );
        // An explicit parent request can reapply the current mode (e.g. Use
        // auto after a failed different wish). It must receive a transaction,
        // not disappear and leave the parent's pending state to time out.
        assert!(matches!(
            machine.prepare(15, 7, 1280, 720, 30),
            Some(Action::SendPrepareQuality { request_id: 2, .. })
        ));
        assert!(machine.prepare(16, 7, 1280, 720, 30).is_none());
    }
    #[test]
    fn fps_mismatch_fails_even_with_matching_dimensions() {
        let mut machine = waiting();
        let actions = machine.step(
            12,
            Event::VideoConfig {
                width: 1280,
                height: 720,
                fps: 25,
            },
        );
        assert!(actions.contains(&Action::CloseConnection));
        assert_eq!(machine.phase(), Phase::Failed);
    }
    #[test]
    fn stale_requests_epochs_and_duplicate_pause_cannot_release_output() {
        let mut machine = Switch::default();
        machine.step(0, Event::EpochObserved(41));
        machine.prepare(0, 7, 1280, 720, 30);
        for event in [
            Event::Paused {
                request_id: 99,
                epoch: 41,
            },
            Event::Paused {
                request_id: 1,
                epoch: 40,
            },
            Event::Rejected {
                request_id: 99,
                reason: 3,
            },
            Event::OutputReady,
        ] {
            assert!(machine.step(1, event).is_empty());
            assert_eq!(machine.phase(), Phase::Preparing);
        }
        assert!(
            !machine
                .step(
                    2,
                    Event::Paused {
                        request_id: 1,
                        epoch: 41
                    }
                )
                .is_empty()
        );
        assert!(
            machine
                .step(
                    3,
                    Event::Paused {
                        request_id: 1,
                        epoch: 41
                    }
                )
                .is_empty()
        );
    }
    #[test]
    fn rejection_before_boundary_preserves_stream_but_after_boundary_closes() {
        let mut machine = Switch::default();
        machine.prepare(0, 7, 1280, 720, 30);
        assert_eq!(
            machine.step(
                1,
                Event::Rejected {
                    request_id: 1,
                    reason: 1
                }
            ),
            vec![Action::ReportFailed {
                reason: "camera permission is not granted"
            }]
        );
        assert!(machine.prepare(2, 7, 1280, 720, 30).is_some());
        let mut machine = waiting();
        assert!(
            machine
                .step(
                    11,
                    Event::Rejected {
                        request_id: 1,
                        reason: 6
                    }
                )
                .contains(&Action::CloseConnection)
        );
    }
    #[test]
    fn deadline_still_runs_after_matching_config_without_output() {
        let mut machine = waiting();
        machine.step(
            12,
            Event::VideoConfig {
                width: 1280,
                height: 720,
                fps: 30,
            },
        );
        assert!(machine.step(4_999, Event::DeadlineExceeded).is_empty());
        assert!(
            machine
                .step(5_000, Event::DeadlineExceeded)
                .contains(&Action::CloseConnection)
        );
    }
    #[test]
    fn staged_request_waits_for_human_then_bounds_camera_start() {
        let mut machine = Switch::default();
        machine.prepare(0, 7, 1280, 720, 30);
        machine.step(
            10,
            Event::PrepareAccepted {
                request_id: 1,
                streaming: false,
            },
        );
        assert!(machine.staged());
        assert!(machine.step(600_000, Event::DeadlineExceeded).is_empty());
        machine.step(
            600_001,
            Event::CaptureStarted {
                request_id: 1,
                width: 1280,
                height: 720,
                fps: 30,
            },
        );
        assert!(
            machine
                .step(605_001, Event::DeadlineExceeded)
                .contains(&Action::CloseConnection)
        );
    }
}
