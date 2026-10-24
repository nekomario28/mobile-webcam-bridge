//! Edition quality cap — mirrors Android `EditionPolicy` for host-side tests
//! and documentation. The phone is the source of truth for advertised modes
//! (device capabilities ∩ edition); the host UI lists only those modes.
//!
//! Private Play Pro replaces the phone policy; this module stays OssCap in
//! public OSS so unit tests pin the free-tier rule without Android.

use super::{Mode, Wish};

/// Edition quality cap on the PC host. Mirrors Android `EditionPolicy`.
/// Everything below reads [`active`]; a private Pro fork changes the one
/// `ACTIVE` constant (or links its own edition crate) — no Pro/billing code here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditionCap {
    pub max_long_edge: u16,
    pub max_short_edge: u16,
    pub max_fps: u16,
}

impl EditionCap {
    pub const fn new(max_long_edge: u16, max_short_edge: u16, max_fps: u16) -> Self {
        Self {
            max_long_edge,
            max_short_edge,
            max_fps,
        }
    }
    /// A (non-zero) size within the edge caps; orientation-independent.
    pub fn allows_size(self, width: u16, height: u16) -> bool {
        let (long, short) = (width.max(height), width.min(height));
        (1..=self.max_long_edge).contains(&long) && (1..=self.max_short_edge).contains(&short)
    }
    pub fn allows(self, width: u16, height: u16, fps: u16) -> bool {
        self.allows_size(width, height) && (1..=self.max_fps).contains(&fps)
    }
    pub fn filter(self, modes: &[Mode]) -> Vec<Mode> {
        modes
            .iter()
            .copied()
            .filter(|mode| self.allows(mode.width, mode.height, mode.fps))
            .collect()
    }
    /// Clamp a saved wish to this cap. An explicit size or fps above the cap is
    /// replaced by Auto for that field only (Auto then resolves to the best
    /// published mode); fields within the cap are kept. Returns whether anything
    /// changed. Never invents a size the phone may not support.
    pub fn clamp_wish(self, wish: Wish) -> (Wish, bool) {
        let mut out = wish;
        if wish.width != 0 && !self.allows_size(wish.width, wish.height) {
            out.width = 0;
            out.height = 0;
        }
        if wish.fps > self.max_fps {
            out.fps = 0;
        }
        (out, out != wish)
    }
}

/// Shipped OSS / Play free: long ≤ 1920, short ≤ 1080, fps ≤ 30.
/// 720p60 and 4K are above it.
pub const OSS_CAP_1080P30: EditionCap = EditionCap::new(1920, 1080, 30);

/// The edition this binary is built as. Public OSS = [`OSS_CAP_1080P30`].
const ACTIVE: EditionCap = OSS_CAP_1080P30;

pub fn active() -> EditionCap {
    ACTIVE
}

pub fn oss_cap_1080p30_allows(width: u16, height: u16, fps: u16) -> bool {
    OSS_CAP_1080P30.allows(width, height, fps)
}

pub fn filter_oss_cap_1080p30(modes: &[Mode]) -> Vec<Mode> {
    OSS_CAP_1080P30.filter(modes)
}

/// If `wish` resolves against `modes`, return it unchanged.
/// Otherwise return Auto `(0,0,0)`, which resolves to the best allowed mode.
pub fn coerce_wish_to_available(wish: Wish, modes: &[Mode]) -> (Wish, bool) {
    if wish.resolve(modes).is_some() {
        (wish, false)
    } else {
        (Wish::default(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: u16, height: u16, fps: u16) -> Mode {
        Mode {
            width,
            height,
            fps,
            bitrate: 6_000_000,
        }
    }

    #[test]
    fn oss_cap_matches_android_edition_policy() {
        assert!(oss_cap_1080p30_allows(1920, 1080, 30));
        assert!(oss_cap_1080p30_allows(1080, 1920, 30));
        assert!(oss_cap_1080p30_allows(1280, 720, 30));
        assert!(!oss_cap_1080p30_allows(3840, 2160, 30));
        assert!(!oss_cap_1080p30_allows(1920, 1080, 60));
        assert!(!oss_cap_1080p30_allows(1280, 720, 60));
    }

    #[test]
    fn filter_is_device_intersect_edition() {
        let device = vec![
            mode(1280, 720, 30),
            mode(1280, 720, 60),
            mode(1920, 1080, 30),
            mode(3840, 2160, 30),
        ];
        let published = filter_oss_cap_1080p30(&device);
        assert_eq!(
            published
                .iter()
                .map(|m| (m.width, m.height, m.fps))
                .collect::<Vec<_>>(),
            vec![(1280, 720, 30), (1920, 1080, 30)]
        );
    }

    #[test]
    fn coerce_falls_back_to_auto_for_4k_or_60fps_wish() {
        let modes = filter_oss_cap_1080p30(&[
            mode(1280, 720, 30),
            mode(1280, 720, 60),
            mode(1920, 1080, 30),
            mode(3840, 2160, 30),
        ]);
        let (wish, migrated) = coerce_wish_to_available(Wish::from((3840, 2160, 30)), &modes);
        assert!(migrated);
        assert_eq!(wish, Wish::default());
        assert_eq!(
            wish.resolve(&modes).unwrap().tuple(),
            (1280, 720, 30),
            "Auto resolves to best allowed among published modes"
        );
        let (wish, migrated) = coerce_wish_to_available(Wish::from((1280, 720, 60)), &modes);
        assert!(migrated);
        assert_eq!(wish, Wish::default());
        let (wish, migrated) = coerce_wish_to_available(Wish::from((1920, 1080, 30)), &modes);
        assert!(!migrated);
        assert_eq!(wish.tuple(), (1920, 1080, 30));
    }

    #[test]
    fn clamp_wish_replaces_above_cap_fields_with_auto() {
        let cap = OSS_CAP_1080P30;
        for (wish, expected) in [
            ((3840, 2160, 30), (0, 0, 30)),
            ((3840, 2160, 60), (0, 0, 0)),
            ((2160, 3840, 0), (0, 0, 0)),
            ((1920, 1081, 30), (0, 0, 30)),
            ((1280, 720, 60), (1280, 720, 0)),
            ((0, 0, 60), (0, 0, 0)),
            ((1920, 1080, 30), (1920, 1080, 30)),
            ((1080, 1920, 30), (1080, 1920, 30)),
            ((0, 0, 0), (0, 0, 0)),
            ((1280, 720, 0), (1280, 720, 0)),
        ] {
            let (clamped, changed) = cap.clamp_wish(Wish::from(wish));
            assert_eq!(clamped.tuple(), expected, "{wish:?}");
            assert_eq!(changed, wish != expected, "{wish:?}");
        }
    }

    #[test]
    fn seam_lets_a_larger_edition_cap_keep_a_pro_wish() {
        let pro = EditionCap::new(3840, 2160, 60);
        let (wish, changed) = pro.clamp_wish(Wish::from((3840, 2160, 60)));
        assert!(!changed);
        assert_eq!(wish.tuple(), (3840, 2160, 60));
        assert!(pro.allows(1280, 720, 60));
        assert!(!OSS_CAP_1080P30.allows(1280, 720, 60));
        assert_eq!(active(), OSS_CAP_1080P30);
    }
}
