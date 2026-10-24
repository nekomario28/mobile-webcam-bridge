use super::{Mode, Wish};
use crate::wire;
/// Per-connection PC state. Draft/saved wishes live in the GUI/settings;
/// pending and output-confirmed values are never inferred from those wishes.
#[derive(Clone, Debug)]
pub struct Client {
    pub modes: Option<Vec<Mode>>,
    pub permission: Option<u8>,
    pub pending: Option<Mode>,
    pub staged: bool,
    pub applied: Option<(u16, u16, u16)>,
    pub error: Option<String>,
    /// Set when advertise rewrote an unsupported saved wish to Auto.
    /// The GUI persists Auto and clears this after saving.
    pub migrated_to_auto: bool,
    initial_wish: Wish,
    initial_checked: bool,
}
impl Default for Client {
    fn default() -> Self {
        Self::new(Wish::default())
    }
}
impl Client {
    pub fn new(initial_wish: Wish) -> Self {
        Self {
            modes: None,
            permission: None,
            pending: None,
            staged: false,
            applied: None,
            error: None,
            migrated_to_auto: false,
            initial_wish,
            initial_checked: false,
        }
    }
    pub fn ready(&self) -> bool {
        self.permission == Some(wire::PERMISSION_READY)
            && self.modes.as_ref().is_some_and(|modes| !modes.is_empty())
    }
    pub fn resolve(&self, wish: Wish) -> Option<Mode> {
        self.modes.as_ref().and_then(|modes| wish.resolve(modes))
    }
    pub fn advertise(&mut self, permission: u8, modes: Vec<Mode>) -> Option<Mode> {
        self.permission = Some(permission);
        self.modes = Some(modes.into_iter().filter(|mode| mode.usable()).collect());
        if !self.ready() || self.initial_checked {
            return None;
        }
        self.initial_checked = true;
        // Phone advertises device ∩ edition only. An old saved 4K / 720p60
        // wish must not fail the first apply — fall back to Auto (best allowed).
        let wish = if self.resolve(self.initial_wish).is_some() {
            self.initial_wish
        } else {
            self.migrated_to_auto = true;
            self.initial_wish = Wish::default();
            Wish::default()
        };
        self.begin(wish)
    }
    pub fn begin(&mut self, wish: Wish) -> Option<Mode> {
        if self.pending.is_some() {
            return None;
        }
        if !self.ready() {
            self.error = Some("camera capabilities or permission are not ready".into());
            return None;
        }
        let Some(mode) = self.resolve(wish) else {
            self.error = Some("the saved/requested quality is not supported by this phone".into());
            return None;
        };
        self.error = None;
        self.initial_checked = true;
        if self.applied == Some(mode.tuple()) {
            return None;
        }
        self.pending = Some(mode);
        self.staged = false;
        Some(mode)
    }
    pub fn progress(&mut self, staged: bool) {
        if self.pending.is_some() {
            self.staged = staged;
            self.applied = None;
        }
    }
    pub fn confirmed(&mut self, width: u16, height: u16, fps: u16) -> bool {
        if self.pending.map(Mode::tuple) != Some((width, height, fps)) {
            self.failed("the receiver confirmed an unexpected quality".into());
            return false;
        }
        self.applied = Some((width, height, fps));
        self.pending = None;
        self.staged = false;
        self.error = None;
        true
    }
    pub fn failed(&mut self, reason: String) {
        self.pending = None;
        self.staged = false;
        self.error = Some(reason);
    }
    pub fn disconnected(&mut self) {
        self.modes = None;
        self.permission = None;
        self.pending = None;
        self.staged = false;
        self.applied = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn modes() -> Vec<Mode> {
        [(1280, 720, 30), (1280, 720, 60), (1920, 1080, 30)]
            .into_iter()
            .map(|(width, height, fps)| Mode {
                width,
                height,
                fps,
                bitrate: 1,
            })
            .collect()
    }
    #[test]
    fn permission_wait_then_one_validated_request_without_replaying_advertisements() {
        let mut client = Client::new((1920, 1080, 0).into());
        assert!(
            client
                .advertise(wire::PERMISSION_PENDING, modes())
                .is_none()
        );
        assert_eq!(
            client
                .advertise(wire::PERMISSION_READY, modes())
                .unwrap()
                .tuple(),
            (1920, 1080, 30)
        );
        assert!(client.advertise(wire::PERMISSION_READY, modes()).is_none());
        assert!(client.begin((1280, 720, 60).into()).is_none());
        assert_eq!(client.pending.unwrap().tuple(), (1920, 1080, 30));
        assert!(client.applied.is_none());
    }
    #[test]
    fn failure_preserves_actual_mode_and_allows_explicit_retry_of_identical_wish() {
        let mut client = Client::default();
        client.advertise(wire::PERMISSION_READY, modes());
        client.confirmed(1280, 720, 30);
        client.begin((1920, 1080, 30).into());
        client.failed("rejected".into());
        assert_eq!(client.applied, Some((1280, 720, 30)));
        assert!(
            client.advertise(wire::PERMISSION_READY, modes()).is_none(),
            "failure retried automatically"
        );
        assert_eq!(
            client.begin((1920, 1080, 30).into()).unwrap().tuple(),
            (1920, 1080, 30)
        );
        client.confirmed(1920, 1080, 30);
        assert_eq!(client.applied, Some((1920, 1080, 30)));
        assert!(client.error.is_none());
    }
    #[test]
    fn unsupported_saved_wish_falls_back_to_auto_and_new_connection_revalidates() {
        let wish = Wish::from((1920, 1080, 60));
        let mut client = Client::new(wish);
        let mode = client
            .advertise(wire::PERMISSION_READY, modes())
            .expect("Auto fallback should apply the best allowed mode");
        assert!(client.migrated_to_auto);
        assert_eq!(
            mode.tuple(),
            Wish::default().resolve(&modes()).unwrap().tuple()
        );
        assert!(client.error.is_none());
        client.disconnected();
        assert!(client.modes.is_none());
        assert!(client.pending.is_none());
        let mut new_connection = Client::new(wish);
        let replacement = vec![Mode {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate: 1,
        }];
        assert_eq!(
            new_connection
                .advertise(wire::PERMISSION_READY, replacement)
                .unwrap()
                .tuple(),
            wish.tuple()
        );
        assert!(!new_connection.migrated_to_auto);
    }

    #[test]
    fn explicit_begin_of_unsupported_wish_still_errors() {
        let mut client = Client::default();
        client.advertise(wire::PERMISSION_READY, modes());
        client.confirmed(1280, 720, 30);
        assert!(client.begin(Wish::from((3840, 2160, 30))).is_none());
        assert!(client.error.as_ref().unwrap().contains("not supported"));
    }
    #[test]
    fn staged_wait_is_not_applied_and_mismatched_confirmation_does_not_lie() {
        let mut client = Client::default();
        client.advertise(wire::PERMISSION_READY, modes());
        client.progress(true);
        assert!(client.staged);
        assert!(client.applied.is_none());
        client.confirmed(1280, 720, 25);
        assert!(client.applied.is_none());
        assert!(client.error.is_some());
        assert!(client.pending.is_none());
    }
}
