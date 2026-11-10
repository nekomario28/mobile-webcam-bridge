//! VIDEO_CONFIG parsing, independent of the GUI and FFmpeg backend.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl Config {
    pub fn parse(bytes: &[u8]) -> crate::Result<Self> {
        if bytes.len() != 16 || bytes[12] != 1 || bytes[13] != 1 {
            return Err("invalid VIDEO_CONFIG".into());
        }
        let c = Self {
            width: u16::from_le_bytes(bytes[0..2].try_into()?) as u32,
            height: u16::from_le_bytes(bytes[2..4].try_into()?) as u32,
            fps: u16::from_le_bytes(bytes[4..6].try_into()?) as u32,
        };
        if c.width == 0
            || c.height == 0
            || c.fps == 0
            || c.width > 8192
            || c.height > 8192
            || c.width as u64 * c.height as u64 > 33_554_432
        {
            return Err("invalid VIDEO_CONFIG dimensions or rate".into());
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    fn packet(width: u16, height: u16, fps: u16) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[0..2].copy_from_slice(&width.to_le_bytes());
        bytes[2..4].copy_from_slice(&height.to_le_bytes());
        bytes[4..6].copy_from_slice(&fps.to_le_bytes());
        bytes[12] = 1;
        bytes[13] = 1;
        bytes
    }

    #[test]
    fn accepts_valid_settings_without_video_backend() {
        let config = Config::parse(&packet(1280, 720, 30)).unwrap();
        assert_eq!((config.width, config.height, config.fps), (1280, 720, 30));
    }

    #[test]
    fn rejects_invalid_settings_without_video_backend() {
        assert!(Config::parse(&packet(0, 720, 30)).is_err());
        assert!(Config::parse(&packet(1280, 720, 0)).is_err());
        assert!(Config::parse(&packet(8192, 8192, 30)).is_err());
        let mut wrong_version = packet(1280, 720, 30);
        wrong_version[12] = 0;
        assert!(Config::parse(&wrong_version).is_err());
        assert!(Config::parse(&packet(1280, 720, 30)[..15]).is_err());
    }
}
