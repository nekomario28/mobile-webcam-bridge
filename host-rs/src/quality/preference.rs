use super::Mode;
/// A saved wish, not an applied mode. Size and fps have independent Auto (0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wish {
    pub width: u16,
    pub height: u16,
    pub fps: u16,
}
impl From<(u16, u16, u16)> for Wish {
    fn from((width, height, fps): (u16, u16, u16)) -> Self {
        Self { width, height, fps }
    }
}
impl Wish {
    pub fn tuple(self) -> (u16, u16, u16) {
        (self.width, self.height, self.fps)
    }
    pub fn valid(self) -> bool {
        (self.width == 0) == (self.height == 0)
            && self.width <= 3840
            && self.height <= 2160
            && self.width.is_multiple_of(2)
            && self.fps <= 60
    }
    pub fn resolve(self, modes: &[Mode]) -> Option<Mode> {
        if !self.valid() {
            return None;
        }
        modes
            .iter()
            .copied()
            .filter(|mode| {
                mode.usable()
                    && (self.width == 0 || (mode.width, mode.height) == (self.width, self.height))
                    && (self.fps == 0 || mode.fps == self.fps)
            })
            .min_by(|a, b| {
                let fps_rank = |fps: u16| if fps <= 30 { (0, 30 - fps) } else { (1, fps) };
                let aspect_error = |mode: &Mode| {
                    (f64::from(mode.width) / f64::from(mode.height) - 1280.0 / 720.0).abs()
                };
                let area_error = |mode: &Mode| {
                    (u32::from(mode.width) * u32::from(mode.height)).abs_diff(1280 * 720)
                };
                fps_rank(a.fps)
                    .cmp(&fps_rank(b.fps))
                    .then_with(|| aspect_error(a).total_cmp(&aspect_error(b)))
                    .then_with(|| area_error(a).cmp(&area_error(b)))
                    .then_with(|| (a.width, a.height).cmp(&(b.width, b.height)))
            })
    }
}
impl Mode {
    pub fn tuple(self) -> (u16, u16, u16) {
        (self.width, self.height, self.fps)
    }
    pub fn usable(self) -> bool {
        self.width != 0
            && self.height != 0
            && self.fps != 0
            && Wish::from(self.tuple()).valid()
            && self.bitrate != 0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn modes() -> Vec<Mode> {
        [
            (1920, 1080, 15),
            (1920, 1080, 30),
            (1280, 720, 60),
            (1280, 720, 30),
        ]
        .into_iter()
        .map(|(width, height, fps)| Mode {
            width,
            height,
            fps,
            bitrate: 6_000_000,
        })
        .collect()
    }
    #[test]
    fn shared_android_pc_preference_fixture() {
        let bytes = include_bytes!("../../../protocol/fixtures/quality-capabilities-v1.bin");
        let modes: Vec<_> = crate::wire::Capabilities::decode(&bytes[crate::wire::HEADER_SIZE..])
            .unwrap()
            .modes
            .into_iter()
            .map(Mode::from)
            .collect();
        for line in include_str!("../../../protocol/fixtures/quality-preferences-v1.txt")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        {
            let (wish, expected) = line.split_once(" -> ").unwrap();
            let numbers = |s: &str| {
                let values: Vec<u16> = s.split_whitespace().map(|n| n.parse().unwrap()).collect();
                assert_eq!(values.len(), 3);
                (values[0], values[1], values[2])
            };
            let result = Wish::from(numbers(wish)).resolve(&modes).map(Mode::tuple);
            assert_eq!(
                result,
                if expected == "rejected" {
                    None
                } else {
                    Some(numbers(expected))
                },
                "{line}"
            );
        }
    }
    #[test]
    fn auto_dimensions_and_fps_resolve_independently_and_explicit_never_substitutes() {
        let modes = modes();
        for (wish, expected) in [
            ((0, 0, 0), (1280, 720, 30)),
            ((1920, 1080, 0), (1920, 1080, 30)),
            ((0, 0, 60), (1280, 720, 60)),
        ] {
            assert_eq!(Wish::from(wish).resolve(&modes).unwrap().tuple(), expected);
        }
        assert!(Wish::from((1920, 1080, 60)).resolve(&modes).is_none());
        assert!(Wish::from((0, 0, 0)).resolve(&[]).is_none());
    }
    #[test]
    fn auto_prefers_available_fps_below_30_then_aspect_and_area() {
        let modes = [(640, 480, 25), (1920, 1080, 25), (1280, 720, 60)]
            .into_iter()
            .map(|(width, height, fps)| Mode {
                width,
                height,
                fps,
                bitrate: 1,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            Wish::default().resolve(&modes).unwrap().tuple(),
            (1920, 1080, 25)
        );
    }
    #[test]
    fn unusable_advertised_modes_cannot_become_a_request() {
        for mode in [
            Mode {
                width: 1281,
                height: 720,
                fps: 30,
                bitrate: 1,
            },
            Mode {
                width: 1280,
                height: 720,
                fps: 0,
                bitrate: 1,
            },
            Mode {
                width: 4096,
                height: 2160,
                fps: 30,
                bitrate: 1,
            },
            Mode {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate: 0,
            },
        ] {
            assert!(Wish::default().resolve(&[mode]).is_none());
        }
    }
}
