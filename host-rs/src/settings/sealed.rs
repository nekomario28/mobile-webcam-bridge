//! At-rest protection for the Linux (Unix) credential files.
//!
//! The PC identity file and the settings file's `lanKey` value used to be stored
//! as plain text guarded only by mode 0600. With a desktop Secret Service
//! (GNOME Keyring, KWallet, KeePassXC, ...) available, they are now sealed with
//! ChaCha20-Poly1305 under a random 256-bit *storage key* that lives in the
//! user's keyring, not on disk. The files keep their 0600 mode and their
//! existing atomic-write rules; only the stored text changes, to
//! `mwss1:<hex(nonce || ciphertext || tag)>`.
//!
//! Without a Secret Service (a headless box, a session without D-Bus, the
//! `keyring` feature disabled, or `MOBILE_WEBCAM_KEYRING=off`) values are written
//! exactly as before, so nothing that worked stops working. Plain values that
//! are found while a keyring *is* available are sealed on the next save
//! (`migrate`), which is how existing installs move over.
//!
//! A sealed value never falls back to anything: if the storage key cannot be
//! read, the value cannot be opened and the caller fails closed, the same as
//! for a corrupt file.
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::path::Path;

/// Prefix of a sealed value. Never valid hex, so a sealed value cannot be
/// mistaken for a plain key and a plain key never starts with it.
pub(crate) const PREFIX: &str = "mwss1:";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Where the storage key lives.
pub(crate) trait KeyProvider {
    /// The existing storage key, `Ok(None)` if the keyring has none.
    fn get(&self) -> crate::Result<Option<[u8; KEY_LEN]>>;
    /// Store `key` as the storage key.
    fn put(&self, key: &[u8; KEY_LEN]) -> crate::Result<()>;
}

/// Whether a stored value is sealed.
pub(crate) fn is_sealed(stored: &str) -> bool {
    stored.starts_with(PREFIX)
}

/// Seal `plain` for `purpose` if a keyring is available; otherwise return it
/// unchanged (the pre-keyring format). `dir` holds the lock that serializes
/// creation of the storage key between processes.
pub(crate) fn seal(dir: &Path, purpose: &str, plain: &str) -> crate::Result<String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    match provider() {
        Some(provider) if sealing_enabled() => match storage_key(dir, provider.as_ref(), true) {
            Ok(Some(key)) => seal_with(&key, purpose, plain),
            // No usable keyring: keep the previous format rather than refuse to save.
            Ok(None) | Err(_) => Ok(plain.to_owned()),
        },
        _ => Ok(plain.to_owned()),
    }
}

/// Open a stored value. Plain (legacy or keyring-less) values pass through.
pub(crate) fn open(purpose: &str, stored: &str) -> crate::Result<String> {
    if !is_sealed(stored) {
        return Ok(stored.to_owned());
    }
    let provider = provider().ok_or(
        "a stored credential is protected by the desktop keyring, but this build cannot use it",
    )?;
    let key = provider
        .get()
        .map_err(|error| format!("the desktop keyring is unavailable ({error}); unlock it and retry"))?
        .ok_or("the desktop keyring no longer holds this app's storage key; explicit recovery required")?;
    open_with(&key, purpose, stored)
}

/// Whether a plain stored value should be rewritten sealed now.
pub(crate) fn should_migrate(stored: &str) -> bool {
    !stored.is_empty() && !is_sealed(stored) && sealing_enabled() && provider().is_some()
}

fn sealing_enabled() -> bool {
    !matches!(
        std::env::var("MOBILE_WEBCAM_KEYRING").as_deref(),
        Ok("off" | "0" | "false" | "no")
    )
}

fn aad(purpose: &str) -> Vec<u8> {
    format!("mobile-webcam/{purpose}").into_bytes()
}

pub(crate) fn seal_with(key: &[u8; KEY_LEN], purpose: &str, plain: &str) -> crate::Result<String> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| "random nonce unavailable")?;
    let sealing = LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| "invalid storage key")?,
    );
    let mut buffer = plain.as_bytes().to_vec();
    sealing
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(purpose)),
            &mut buffer,
        )
        .map_err(|_| "sealing failed")?;
    let mut out = String::with_capacity(PREFIX.len() + 2 * (NONCE_LEN + buffer.len()));
    out.push_str(PREFIX);
    for byte in nonce.iter().chain(buffer.iter()) {
        out.push_str(&format!("{byte:02x}"));
    }
    Ok(out)
}

pub(crate) fn open_with(key: &[u8; KEY_LEN], purpose: &str, stored: &str) -> crate::Result<String> {
    const CORRUPT: &str = "a protected credential is corrupt or was sealed under another key";
    let hex = stored.strip_prefix(PREFIX).ok_or(CORRUPT)?;
    let mut bytes = decode_hex(hex).ok_or(CORRUPT)?;
    if bytes.len() < NONCE_LEN + CHACHA20_POLY1305.tag_len() {
        return Err(CORRUPT.into());
    }
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes[..NONCE_LEN]);
    let opening = LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| "invalid storage key")?,
    );
    let plain = opening
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(purpose)),
            &mut bytes[NONCE_LEN..],
        )
        .map_err(|_| CORRUPT)?;
    String::from_utf8(plain.to_vec()).map_err(|_| CORRUPT.into())
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Read the storage key, creating it when `create` and none exists. Creation
/// holds an exclusive lock in `dir`, so two first launches cannot each store a
/// different key and leave one of them sealing under a key that is gone.
fn storage_key(
    dir: &Path,
    provider: &dyn KeyProvider,
    create: bool,
) -> crate::Result<Option<[u8; KEY_LEN]>> {
    if let Some(key) = provider.get()? {
        return Ok(Some(key));
    }
    if !create {
        return Ok(None);
    }
    let _lock = DirLock::acquire(dir)?;
    if let Some(key) = provider.get()? {
        return Ok(Some(key));
    }
    let mut key = [0u8; KEY_LEN];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| "random storage key unavailable")?;
    provider.put(&key)?;
    // Use what the keyring now holds, and only if it reads back.
    match provider.get()? {
        Some(stored) if stored == key => Ok(Some(stored)),
        _ => Err("the desktop keyring did not keep the storage key".into()),
    }
}

struct DirLock(std::fs::File);
impl DirLock {
    fn acquire(dir: &Path) -> crate::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join(".storage-key.lock"))?;
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(file))
    }
}
impl Drop for DirLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // SAFETY: unlocking the descriptor locked in `acquire`.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(not(test))]
fn provider() -> Option<Box<dyn KeyProvider>> {
    #[cfg(all(target_os = "linux", feature = "keyring"))]
    {
        secret_service::SecretServiceProvider::connect()
            .map(|p| Box::new(p) as Box<dyn KeyProvider>)
    }
    #[cfg(not(all(target_os = "linux", feature = "keyring")))]
    {
        None
    }
}

#[cfg(all(target_os = "linux", feature = "keyring"))]
#[cfg_attr(test, allow(dead_code))]
mod secret_service {
    use super::{KEY_LEN, KeyProvider};
    use ::secret_service::EncryptionType;
    use ::secret_service::blocking::SecretService;
    use std::collections::HashMap;

    fn attributes() -> HashMap<&'static str, &'static str> {
        HashMap::from([("application", "mobile-webcam"), ("purpose", "storage-key")])
    }

    pub(super) struct SecretServiceProvider(SecretService<'static>);

    impl SecretServiceProvider {
        /// `None` when no Secret Service answers on the session bus.
        pub(super) fn connect() -> Option<Self> {
            std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
                .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))?;
            let service = SecretService::connect(EncryptionType::Dh).ok()?;
            // A bus without a Secret Service fails here rather than on first use.
            service.get_any_collection().ok()?;
            Some(Self(service))
        }
    }

    impl KeyProvider for SecretServiceProvider {
        fn get(&self) -> crate::Result<Option<[u8; KEY_LEN]>> {
            let found = self
                .0
                .search_items(attributes())
                .map_err(|error| format!("keyring search failed: {error}"))?;
            let item = match found.unlocked.into_iter().next() {
                Some(item) => item,
                None => match found.locked.into_iter().next() {
                    Some(item) => {
                        item.unlock()
                            .map_err(|error| format!("keyring is locked: {error}"))?;
                        item
                    }
                    None => return Ok(None),
                },
            };
            let secret = item
                .get_secret()
                .map_err(|error| format!("keyring read failed: {error}"))?;
            let key: [u8; KEY_LEN] = secret
                .as_slice()
                .try_into()
                .map_err(|_| "the keyring's storage key has the wrong length")?;
            Ok(Some(key))
        }

        fn put(&self, key: &[u8; KEY_LEN]) -> crate::Result<()> {
            let collection = self
                .0
                .get_any_collection()
                .map_err(|error| format!("keyring unavailable: {error}"))?;
            if collection.is_locked().unwrap_or(true) {
                collection
                    .unlock()
                    .map_err(|error| format!("keyring is locked: {error}"))?;
            }
            collection
                .create_item(
                    "Mobile Webcam storage key",
                    attributes(),
                    key,
                    true,
                    "application/octet-stream",
                )
                .map_err(|error| format!("keyring write failed: {error}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_PROVIDER: std::cell::RefCell<Option<std::rc::Rc<test_support::MemoryKeyring>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn provider() -> Option<Box<dyn KeyProvider>> {
    TEST_PROVIDER.with(|cell| {
        cell.borrow()
            .clone()
            .map(|keyring| Box::new(test_support::Shared(keyring)) as Box<dyn KeyProvider>)
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{KEY_LEN, KeyProvider};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// An in-memory keyring for this thread's tests; real keyrings are never touched.
    #[derive(Default)]
    pub(crate) struct MemoryKeyring {
        pub(crate) key: RefCell<Option<[u8; KEY_LEN]>>,
        pub(crate) unavailable: RefCell<bool>,
    }
    pub(super) struct Shared(pub(super) Rc<MemoryKeyring>);
    impl KeyProvider for Shared {
        fn get(&self) -> crate::Result<Option<[u8; KEY_LEN]>> {
            if *self.0.unavailable.borrow() {
                return Err("test keyring unavailable".into());
            }
            Ok(*self.0.key.borrow())
        }
        fn put(&self, key: &[u8; KEY_LEN]) -> crate::Result<()> {
            if *self.0.unavailable.borrow() {
                return Err("test keyring unavailable".into());
            }
            *self.0.key.borrow_mut() = Some(*key);
            Ok(())
        }
    }

    /// Install a fresh in-memory keyring for the current thread.
    pub(crate) fn install() -> Rc<MemoryKeyring> {
        let keyring = Rc::new(MemoryKeyring::default());
        super::TEST_PROVIDER.with(|cell| *cell.borrow_mut() = Some(keyring.clone()));
        keyring
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_values_round_trip_and_bind_their_purpose() {
        let key = [7u8; KEY_LEN];
        let plain = "plain text that must not survive sealing";
        let sealed = seal_with(&key, "identity", plain).unwrap();
        assert!(is_sealed(&sealed));
        assert!(!sealed.contains(plain));
        assert_eq!(open_with(&key, "identity", &sealed).unwrap(), plain);
        assert!(
            open_with(&key, "lanKey", &sealed).is_err(),
            "purpose is authenticated"
        );
        assert!(open_with(&[8u8; KEY_LEN], "identity", &sealed).is_err());
        let mut tampered = sealed.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == '0' { '1' } else { '0' });
        assert!(open_with(&key, "identity", &tampered).is_err());
        for bad in ["mwss1:", "mwss1:zz", "mwss1:0", "mwss1:00"] {
            assert!(open_with(&key, "identity", bad).is_err());
        }
    }

    #[test]
    fn without_a_keyring_values_stay_in_the_previous_format() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(seal(dir.path(), "lanKey", "abcd").unwrap(), "abcd");
        assert_eq!(open("lanKey", "abcd").unwrap(), "abcd");
        assert!(!should_migrate("abcd"));
        // ...but a sealed value never opens without one.
        let sealed = seal_with(&[1; KEY_LEN], "lanKey", "abcd").unwrap();
        assert!(open("lanKey", &sealed).is_err());
    }

    #[test]
    fn a_keyring_seals_and_creates_the_storage_key_once() {
        let dir = tempfile::tempdir().unwrap();
        let keyring = test_support::install();
        assert!(should_migrate("abcd"));
        let first = seal(dir.path(), "lanKey", "abcd").unwrap();
        let key = keyring.key.borrow().expect("storage key created");
        let second = seal(dir.path(), "lanKey", "abcd").unwrap();
        assert_eq!(keyring.key.borrow().unwrap(), key, "never rotated");
        assert_ne!(first, second, "fresh nonce per seal");
        assert_eq!(open("lanKey", &first).unwrap(), "abcd");
        assert_eq!(open("lanKey", &second).unwrap(), "abcd");
        assert_eq!(seal(dir.path(), "lanKey", "").unwrap(), "");
    }

    #[test]
    fn an_unavailable_keyring_saves_plain_but_cannot_open_sealed() {
        let dir = tempfile::tempdir().unwrap();
        let keyring = test_support::install();
        let sealed = seal(dir.path(), "identity", "abcd").unwrap();
        *keyring.unavailable.borrow_mut() = true;
        assert_eq!(seal(dir.path(), "identity", "abcd").unwrap(), "abcd");
        assert!(open("identity", &sealed).is_err());
        *keyring.unavailable.borrow_mut() = false;
        *keyring.key.borrow_mut() = None;
        let error = open("identity", &sealed).unwrap_err().to_string();
        assert!(error.contains("no longer holds"), "{error}");
    }
}
