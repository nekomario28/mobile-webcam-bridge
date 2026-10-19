//! Real per-user identity persistence. Publish a completely written key with an
//! atomic no-replace hard link so concurrent first launches cannot rotate it.
use super::Identity;
#[cfg(unix)]
use std::fs::File;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_STORED_BYTES: u64 = 4096;

impl Identity {
    /// Load the per-user key, creating it only if missing. Corruption and I/O
    /// failures preserve existing data and require explicit recovery. Connection
    /// startup must consume this API before signing a real authorization proof.
    pub fn load_or_create() -> crate::Result<Self> {
        load_at(&crate::settings::Store::native()?.identity_path()?)
    }

    /// Move an unusable stored key aside and publish a fresh one.
    ///
    /// **Explicit only** — nothing on the normal startup path calls this, so a
    /// corrupt or unreadable key still fails closed until the user asks for this.
    /// The unusable file is *renamed*, never deleted and never overwritten: its
    /// bytes and their permissions survive at the returned path so the cause can
    /// still be inspected. Because the phone stores this PC's public key, every
    /// previously approved phone must approve this PC again — the caller is
    /// expected to say so before asking.
    pub fn replace_stored() -> crate::Result<std::path::PathBuf> {
        replace_at(&crate::settings::Store::native()?.identity_path()?)
    }
}

/// The quarantine name for a replaced key, never colliding with an existing file.
fn quarantine_path(path: &Path) -> crate::Result<std::path::PathBuf> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("identity path has no usable file name")?
        .to_owned();
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    for attempt in 0..1000u32 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let candidate = path.with_file_name(format!("{name}.corrupt-{since_epoch}{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("too many quarantined copies of the stored PC key; move them away first".into())
}

/// Testable form of [`Identity::replace_stored`].
fn replace_at(path: &Path) -> crate::Result<std::path::PathBuf> {
    let quarantine = quarantine_path(path)?;
    fs::rename(path, &quarantine)?;
    #[cfg(unix)]
    if let Some(parent) = quarantine.parent() {
        // The rename is the recovery's commit point; make it durable before the
        // new key is published, so a crash cannot lose both.
        let _ = File::open(parent).and_then(|dir| dir.sync_all());
    }
    load_at(path)?;
    Ok(quarantine)
}

fn read_at(path: &Path) -> crate::Result<Option<Identity>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Refuse symlinks and avoid blocking on a FIFO before checking its type.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_STORED_BYTES {
        return Err("invalid PC identity file type or size".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(
                "PC identity must be owned by this user and accessible only to this user".into(),
            );
        }
    }
    let mut stored = String::new();
    file.take(MAX_STORED_BYTES + 1)
        .read_to_string(&mut stored)?;
    if stored.len() as u64 > MAX_STORED_BYTES {
        return Err("PC identity exceeds its storage limit".into());
    }
    #[cfg(windows)]
    let stored = crate::settings::dpapi::unprotect(&stored)?;
    Identity::from_hex(&stored)
        .map(Some)
        .ok_or_else(|| "PC identity is corrupt; explicit recovery required (file preserved)".into())
}

fn load_at(path: &Path) -> crate::Result<Identity> {
    if let Some(identity) = read_at(path)? {
        return Ok(identity);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("identity needs a parent directory")?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(parent)?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err("PC identity directory is not protected from other users' writes".into());
        }
    }
    let identity = Identity::generate();
    let temporary = parent.join(format!(
        ".pc-identity-{}.tmp",
        super::hex(&identity.client_id())
    ));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let _cleanup = Cleanup(temporary.clone());
    #[cfg(windows)]
    let stored = crate::settings::dpapi::protect(&identity.to_hex())?;
    #[cfg(unix)]
    let stored = identity.to_hex();
    file.write_all(stored.as_bytes())?;
    file.sync_all()?;
    drop(file);
    match fs::hard_link(&temporary, path) {
        Ok(()) => {
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            Ok(identity)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_at(path)?
            .ok_or_else(|| "PC identity changed during creation; reconnect explicitly".into()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_a_corrupt_key_quarantines_its_bytes_and_publishes_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private/identity");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let unusable = b"this is not a stored key";
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path).unwrap().write_all(unusable).unwrap();

        // Nothing recovers on its own: startup still fails closed.
        assert!(load_at(&path).is_err());

        let quarantine = replace_at(&path).unwrap();
        // The unusable bytes and their permissions survive at the reported path.
        assert_eq!(fs::read(&quarantine).unwrap(), unusable);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&quarantine).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // A fresh, usable key is published in its place.
        let identity = load_at(&path).unwrap();
        assert!(Identity::verify(
            identity.public_key(),
            b"after recovery",
            &identity.sign(b"after recovery").unwrap()
        ));
        assert_ne!(
            fs::read(&path).unwrap(),
            unusable,
            "the stored file must now hold the new key"
        );
    }

    #[test]
    fn replacing_never_overwrites_an_existing_quarantine_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity");
        fs::write(&path, b"unusable").unwrap();
        // Take the name the first attempt would use, with a sentinel inside it.
        let taken = quarantine_path(&path).unwrap();
        fs::write(&taken, b"sentinel").unwrap();
        let quarantine = replace_at(&path).unwrap();
        assert_ne!(
            quarantine, taken,
            "an existing quarantine file must be left alone"
        );
        assert_eq!(fs::read(&taken).unwrap(), b"sentinel");
        assert_eq!(fs::read(&quarantine).unwrap(), b"unusable");
        assert!(load_at(&path).is_ok());
    }

    #[test]
    fn replacing_missing_storage_creates_a_key_without_quarantining_anything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private/identity");
        // No file yet: the rename cannot happen, and recovery must fail rather
        // than pretend it moved something aside.
        assert!(replace_at(&path).is_err());
        assert!(
            load_at(&path).is_ok(),
            "an ordinary first launch still creates a key"
        );
    }

    #[test]
    fn actual_file_roundtrip_preserves_the_key_and_signatures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private/identity");
        let identity = load_at(&path).unwrap();
        let reloaded = load_at(&path).unwrap();
        assert_eq!(identity.client_id(), reloaded.client_id());
        assert!(Identity::verify(
            reloaded.public_key(),
            b"persisted identity",
            &identity.sign(b"persisted identity").unwrap()
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        {
            let ciphertext = fs::read_to_string(&path).unwrap();
            assert!(!ciphertext.contains(&identity.to_hex()));
            assert!(Identity::from_hex(&ciphertext).is_none());
        }
    }

    #[test]
    fn concurrent_first_launches_return_the_same_published_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    load_at(&path).unwrap().client_id()
                })
            })
            .collect();
        let ids: Vec<_> = workers.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(ids.iter().all(|id| *id == ids[0]));
        assert_eq!(load_at(&path).unwrap().client_id(), ids[0]);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "temporary keys are removed"
        );
    }

    #[test]
    fn corruption_never_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity");
        load_at(&path).unwrap();
        for corrupt in [b"0\xc3\xa90".as_slice(), b"not a key", b""] {
            fs::write(&path, corrupt).unwrap();
            assert!(load_at(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), corrupt);
        }
    }

    #[test]
    fn invalid_destination_and_oversized_files_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_at(dir.path()).is_err());
        let path = dir.path().join("identity");
        load_at(&path).unwrap();
        fs::write(&path, vec![b'a'; MAX_STORED_BYTES as usize + 1]).unwrap();
        assert!(load_at(&path).is_err());
        assert!(load_at(&path.join("child")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_other_user_readable_keys_are_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity");
        let identity = load_at(&path).unwrap();
        let link = dir.path().join("symlink");
        symlink(&path, &link).unwrap();
        assert!(load_at(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_at(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), identity.to_hex());
    }
}
