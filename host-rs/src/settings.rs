use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(unix)]
use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub transport: String,
    pub lan_host: String,
    pub lan_fingerprint: String,
    pub lan_key: String,
    pub decode: String,
    pub output: String,
    pub rotation: u16,
    pub mirror: bool,
    pub vertical_flip: bool,
    /// The saved quality wish; 0 means auto (slice 03).
    pub quality_width: u16,
    pub quality_height: u16,
    pub quality_fps: u16,
    #[serde(skip)]
    pub quality_warning: Option<String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("transport", &self.transport)
            .field("lan_host", &self.lan_host)
            .field("lan_fingerprint", &self.lan_fingerprint)
            .field("lan_key", &"<redacted>")
            .field("decode", &self.decode)
            .field("output", &self.output)
            .field("rotation", &self.rotation)
            .field("mirror", &self.mirror)
            .field("vertical_flip", &self.vertical_flip)
            .finish()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            transport: "usb".into(),
            lan_host: String::new(),
            lan_fingerprint: String::new(),
            lan_key: String::new(),
            decode: "auto".into(),
            output: if cfg!(windows) {
                "Unity Video Capture"
            } else {
                "/dev/video10"
            }
            .into(),
            rotation: 0,
            mirror: true,
            vertical_flip: false,
            quality_width: 0,
            quality_height: 0,
            quality_fps: 0,
            quality_warning: None,
        }
    }
}
impl Settings {
    pub fn quality_wish(&self) -> crate::quality::Wish {
        (self.quality_width, self.quality_height, self.quality_fps).into()
    }
    pub fn set_quality_wish(&mut self, wish: crate::quality::Wish) {
        (self.quality_width, self.quality_height, self.quality_fps) = wish.tuple();
        self.quality_warning = None;
    }
    pub fn validate(&self) -> crate::Result<()> {
        if !["usb", "lan"].contains(&self.transport.as_str()) {
            return Err("invalid transport".into());
        }
        for (value, bytes) in [(&self.lan_fingerprint, 32), (&self.lan_key, 16)] {
            if !value.is_empty() && !crate::secure::is_hex(value, bytes) {
                return Err("invalid Wi-Fi pairing fingerprint or key".into());
            }
        }
        if ![0, 90, 180, 270].contains(&self.rotation) {
            return Err("invalid rotation".into());
        }
        if !self.quality_wish().valid() {
            return Err("invalid quality preference".into());
        }
        if !["auto", "off", "software", "vaapi", "cuda", "d3d11va"].contains(&self.decode.as_str())
        {
            return Err("invalid decoder".into());
        }
        if [&self.lan_host, &self.output]
            .iter()
            .any(|s| s.len() > 4096 || s.contains('\0') || s.contains('\n'))
        {
            return Err("invalid saved address or output".into());
        }
        Ok(())
    }
    fn values(&self) -> BTreeMap<String, String> {
        [
            ("transport", self.transport.clone()),
            ("lanHost", self.lan_host.trim().into()),
            ("lanFingerprint", self.lan_fingerprint.trim().into()),
            ("lanKey", self.lan_key.trim().into()),
            ("decode", self.decode.clone()),
            ("output", self.output.trim().into()),
            ("rotation", self.rotation.to_string()),
            ("mirror", self.mirror.to_string()),
            ("verticalFlip", self.vertical_flip.to_string()),
            ("qualityWidth", self.quality_width.to_string()),
            ("qualityHeight", self.quality_height.to_string()),
            ("qualityFps", self.quality_fps.to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect()
    }
    fn from_values(values: &BTreeMap<String, String>) -> crate::Result<Self> {
        let mut s = Self::default();
        let mut quality_invalid = false;
        for (key, value) in values {
            match key.as_str() {
                "transport" => s.transport = value.clone(),
                "lanHost" => s.lan_host = value.clone(),
                "lanFingerprint" => s.lan_fingerprint = value.clone(),
                "lanKey" => s.lan_key = value.clone(),
                "decode" => s.decode = value.clone(),
                "output" => s.output = value.clone(),
                "rotation" => s.rotation = value.parse()?,
                "mirror" => s.mirror = parse_bool(value)?,
                "verticalFlip" => s.vertical_flip = parse_bool(value)?,
                "qualityWidth" => s.quality_width = parse_quality(value, &mut quality_invalid),
                "qualityHeight" => s.quality_height = parse_quality(value, &mut quality_invalid),
                "qualityFps" => s.quality_fps = parse_quality(value, &mut quality_invalid),
                _ => {}
            }
        }
        if quality_invalid || !s.quality_wish().valid() {
            s.set_quality_wish(crate::quality::Wish::default());
            s.quality_warning = Some("invalid saved quality preference; using Auto".into());
        }
        s.validate()?;
        Ok(s)
    }
}
fn parse_quality(value: &str, invalid: &mut bool) -> u16 {
    value.parse().unwrap_or_else(|_| {
        *invalid = true;
        0
    })
}
fn parse_bool(value: &str) -> crate::Result<bool> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err("invalid saved boolean".into()),
    }
}

pub struct Store {
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(unix)]
    fallbacks: Vec<PathBuf>,
}
impl Store {
    pub fn native() -> crate::Result<Self> {
        #[cfg(unix)]
        {
            let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| PathBuf::from(home).join(".config"));
            let mut fallbacks = vec![config.join("nekomario28.conf")];
            for dir in std::env::var("XDG_CONFIG_DIRS")
                .unwrap_or_else(|_| "/etc/xdg".into())
                .split(':')
                .filter(|d| d.starts_with('/'))
            {
                fallbacks.push(PathBuf::from(dir).join("nekomario28/Mobile Webcam.conf"));
                fallbacks.push(PathBuf::from(dir).join("nekomario28.conf"));
            }
            Ok(Self {
                path: config.join("nekomario28/Mobile Webcam.conf"),
                fallbacks,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {})
        }
    }
    #[cfg(unix)]
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            fallbacks: Vec::new(),
        }
    }
    pub(crate) fn identity_path(&self) -> crate::Result<std::path::PathBuf> {
        #[cfg(unix)]
        let path = self.path.with_file_name("Mobile Webcam.identity");
        #[cfg(windows)]
        let path = std::path::PathBuf::from(
            std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable for PC identity")?,
        )
        .join("nekomario28/Mobile Webcam/client-identity.dpapi");
        if !path.is_absolute() {
            return Err("PC identity requires an absolute user configuration path".into());
        }
        Ok(path)
    }

    pub fn load(&self) -> crate::Result<Settings> {
        #[cfg(unix)]
        {
            let mut values = BTreeMap::new();
            for (primary, path) in std::iter::once((true, &self.path))
                .chain(self.fallbacks.iter().map(|path| (false, path)))
            {
                let data = read_file(path)?;
                for (key, value) in parse_ini(&data)? {
                    // Pairing credentials bind to one user's own phone. A
                    // system-wide fallback may provide non-secret defaults but
                    // never a fingerprint or access key — the same scope rule
                    // the Windows loader applies to machine-scope values.
                    if !primary && matches!(key.as_str(), "lanKey" | "lanFingerprint") {
                        continue;
                    }
                    values.entry(key).or_insert(value);
                }
            }
            Settings::from_values(&values)
        }
        #[cfg(windows)]
        {
            Settings::from_values(&registry::read()?)
        }
    }
    pub fn save(&self, settings: &Settings) -> crate::Result<()> {
        settings.validate()?;
        #[cfg(unix)]
        {
            use std::io::Write;
            let data = read_file(&self.path)?;
            // Refuse to replace damaged settings. Unknown lines and sections survive.
            Settings::from_values(&parse_ini(&data)?)?;
            let values = settings.values();
            let mut lines = Vec::new();
            let mut general = false;
            for line in data.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    general = trimmed == "[General]";
                }
                let known = general
                    && trimmed
                        .split_once('=')
                        .is_some_and(|(k, _)| values.contains_key(k.trim()));
                if !known {
                    lines.push(line.to_owned());
                }
            }
            let additions: Vec<_> = values
                .into_iter()
                .map(|(key, value)| format!("{key}={}", encode_ini(&value)))
                .collect();
            if let Some(index) = lines.iter().position(|line| line.trim() == "[General]") {
                lines.splice(index + 1..index + 1, additions);
            } else {
                lines.push("[General]".into());
                lines.extend(additions);
            }
            let parent = self.path.parent().ok_or("settings directory unavailable")?;
            std::fs::create_dir_all(parent)?;
            let temporary = parent.join(format!(".Mobile-Webcam-{}.tmp", std::process::id()));
            let result = (|| -> crate::Result<()> {
                use std::os::unix::fs::OpenOptionsExt;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)?;
                file.write_all((lines.join("\n") + "\n").as_bytes())?;
                file.sync_all()?;
                if read_file(&self.path)? != data {
                    return Err("settings changed in another process; reopen the app".into());
                }
                std::fs::rename(&temporary, &self.path)?;
                std::fs::File::open(parent)?.sync_all()?;
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&temporary);
            }
            result
        }
        #[cfg(windows)]
        {
            registry::save(&settings.values())
        }
    }
}

#[cfg(unix)]
fn read_file(path: &std::path::Path) -> crate::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(data) if data.len() <= 1024 * 1024 => {
            use std::os::unix::fs::PermissionsExt;
            // A legacy/external config may have been written as mode 0644.
            // Never load a bearer token from a file readable by other users.
            let contains_key = data
                .lines()
                .any(|line| line.trim_start().starts_with("lanKey="));
            if contains_key && std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
                return Err("Wi-Fi credentials are readable by other users; run chmod 600 on the settings file".into());
            }
            Ok(data)
        }
        Ok(_) => Err("settings file exceeds 1 MiB".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}
#[cfg(unix)]
fn parse_ini(data: &str) -> crate::Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    let mut general = false;
    for line in data.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            general = line == "[General]";
            continue;
        }
        if !general || line.starts_with(';') || line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && Settings::default().values().contains_key(key.trim())
        {
            values.insert(key.trim().into(), decode_ini(value.trim())?);
        }
    }
    Ok(values)
}
#[cfg(unix)]
fn decode_ini(value: &str) -> crate::Result<String> {
    let mut result = String::new();
    let mut chars = value.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => match chars.next().ok_or("truncated INI escape")? {
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                'a' => result.push('\u{7}'),
                'b' => result.push('\u{8}'),
                'f' => result.push('\u{c}'),
                'v' => result.push('\u{b}'),
                '\\' => result.push('\\'),
                '"' => result.push('"'),
                'x' => {
                    let mut hex = String::new();
                    while chars.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                        hex.push(chars.next().unwrap());
                    }
                    result.push(
                        char::from_u32(u32::from_str_radix(&hex, 16)?)
                            .ok_or("invalid INI character")?,
                    );
                }
                _ => return Err("unsupported INI escape in saved setting".into()),
            },
            _ => result.push(c),
        }
    }
    if quoted {
        return Err("unterminated INI quote".into());
    }
    if result.starts_with("@@") {
        result.remove(0);
    } else if result.starts_with('@') {
        return Err("unsupported Qt variant in scalar setting".into());
    }
    Ok(result)
}
#[cfg(unix)]
fn encode_ini(value: &str) -> String {
    let value = if value.starts_with('@') {
        format!("@{value}")
    } else {
        value.into()
    };
    let mut result = String::from("\"");
    let mut escape_next_hex = false;
    for ch in value.chars() {
        match ch {
            '\\' | '"' => {
                result.push('\\');
                result.push(ch);
                escape_next_hex = false;
            }
            '\n' => {
                result.push_str("\\n");
                escape_next_hex = false;
            }
            '\r' => {
                result.push_str("\\r");
                escape_next_hex = false;
            }
            _ if ch < ' ' || escape_next_hex && ch.is_ascii_hexdigit() => {
                result.push_str(&format!("\\x{:x}", ch as u32));
                escape_next_hex = true;
            }
            _ => {
                result.push(ch);
                escape_next_hex = false;
            }
        }
    }
    result.push('"');
    result
}

#[cfg(windows)]
pub(crate) mod dpapi;
#[cfg(windows)]
mod registry;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_auto_dimensions_and_fps_are_valid_preferences() {
        for (width, height, fps) in [(1280, 720, 0), (0, 0, 60)] {
            let settings = Settings {
                quality_width: width,
                quality_height: height,
                quality_fps: fps,
                ..Default::default()
            };
            assert!(
                settings.validate().is_ok(),
                "valid independent Auto preference rejected: {settings:?}"
            );
        }
    }
    #[test]
    fn malformed_quality_keys_reset_only_quality_not_saved_connection() {
        for bad in ["broken", "-1", "999999"] {
            let values = [
                ("lanHost".into(), "192.168.4.7:48527".into()),
                ("rotation".into(), "90".into()),
                ("qualityWidth".into(), bad.into()),
            ]
            .into_iter()
            .collect();
            let settings = Settings::from_values(&values)
                .expect("quality corruption discarded the saved connection");
            assert_eq!(settings.lan_host, "192.168.4.7:48527");
            assert_eq!(settings.rotation, 90);
            assert_eq!(settings.quality_wish(), crate::quality::Wish::default());
            assert!(
                settings
                    .quality_warning
                    .as_ref()
                    .unwrap()
                    .contains("using Auto")
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn refuses_world_readable_wifi_secret() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.conf");
        std::fs::write(
            &path,
            "[General]\nlanKey=00112233445566778899aabbccddeeff\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Store::at(path.clone()).load().is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            Store::at(path).load().unwrap().lan_key,
            "00112233445566778899aabbccddeeff"
        );
    }

    #[test]
    #[cfg(unix)]
    fn system_fallback_never_supplies_pairing_credentials() {
        use std::os::unix::fs::PermissionsExt;
        let user_dir = tempfile::tempdir().unwrap();
        let system_dir = tempfile::tempdir().unwrap();
        let user_path = user_dir.path().join("settings.conf");
        let system_path = system_dir.path().join("settings.conf");
        std::fs::write(&user_path, "[General]\ntransport=usb\n").unwrap();
        std::fs::write(
            &system_path,
            "[General]\ntransport=lan\nlanHost=203.0.113.7\nlanFingerprint=5331e886b4b4a360fb7ae16a0afc15791c595354a3c9bf329bf8b4a3eef25a25\nlanKey=00112233445566778899aabbccddeeff\n",
        )
        .unwrap();
        std::fs::set_permissions(&system_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let store = Store {
            path: user_path,
            fallbacks: vec![system_path],
        };
        let settings = store.load().unwrap();
        // Non-secret defaults still inherit; credentials never do.
        assert_eq!(settings.transport, "usb");
        assert_eq!(settings.lan_host, "203.0.113.7");
        assert!(settings.lan_fingerprint.is_empty());
        assert!(settings.lan_key.is_empty());
    }

    // Store::at is the unix file store; this round trip is a Qt fixture test.
    #[cfg(unix)]
    #[test]
    fn qt_fixture_unknown_keys_and_failed_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.conf");
        std::fs::write(&path,"[General]\nlanHost=192.168.4.42\ntransport=lan\nrotation=90\nmirror=false\nverticalFlip=true\nunknown=@ByteArray(opaque)\n[other]\nx=1\n").unwrap();
        let store = Store::at(path.clone());
        let mut s = store.load().unwrap();
        assert_eq!(s.lan_host, "192.168.4.42");
        assert!(!s.mirror);
        assert!(s.vertical_flip);
        s.lan_host = " 192.168.4.43 ".into();
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap().lan_host, "192.168.4.43");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("unknown=@ByteArray(opaque)")
        );
        std::fs::write(&path, "[General]\nrotation=broken\n").unwrap();
        assert!(store.save(&s).is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "[General]\nrotation=broken\n"
        );
    }
}
