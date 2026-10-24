//! Edition quality cap — mirrors Android `EditionPolicy` for host-side tests
//! and documentation. The phone is the source of truth for advertised modes
//! (device capabilities ∩ edition); the host UI lists only those modes.
//!
//! Private Play Pro replaces the phone policy; this module stays OssCap in
//! public OSS so unit tests pin the free-tier rule without Android.

use super::Mode;

/// Long edge ≤ 1920, short edge ≤ 1080, fps ≤ 30 (landscape and portrait 1080p).
/// 720p60 and 4K are above the shipped OSS / Play free cap.
pub fn oss_cap_1080p30_allows(width: u16, height: u16, fps: u16) -> bool {
    let long_edge = width.max(height);
    let short_edge = width.min(height);
    (1..=1920).contains(&long_edge) && (1..=1080).contains(&short_edge) && (1..=30).contains(&fps)
}

pub fn filter_oss_cap_1080p30(modes: &[Mode]) -> Vec<Mode> {
    modes
        .iter()
        .copied()
        .filter(|mode| oss_cap_1080p30_allows(mode.width, mode.height, mode.fps))
        .collect()
}

/// If `wish` resolves against `modes`, return it unchanged.
/// Otherwise return Auto `(0,0,0)`, which resolves to the best allowed mode.
pub fn coerce_wish_to_available(wish: super::Wish, modes: &[Mode]) -> (super::Wish, bool) {
    if wish.resolve(modes).is_some() {
        (wish, false)
    } else {
        (super::Wish::default(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::Wish;

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
}
