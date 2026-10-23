//! Positional file I/O for the preview record.
//!
//! Kept apart from the record's logic so the logic is what gets mutated and the
//! platform shims are what gets skipped: these are the same two operations on two
//! ABIs, and a mutant in the one your platform does not compile can never be caught
//! by a test running on the other.

use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use super::{CAPACITY, HEADER};

/// Create a fresh private record file and return its path.
///
/// `create_new` refuses any existing path, so a pre-planted symlink or file in the
/// temp directory cannot redirect the preview.
pub fn create_path() -> io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "mobile-webcam-preview-{}-{nanos}.bin",
        std::process::id()
    ));
    let file = private_create(&path)?;
    file.set_len((HEADER + CAPACITY) as u64)?;
    Ok(path)
}

#[cfg(unix)]
pub(super) fn private_create(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Re-open a record this process already created, refusing anything that is not a
/// regular file we own.
///
/// The normal path is GUI-creates-then-worker-opens, so this is the path that runs. The
/// first version used a plain `open`, which follows a symlink: an attacker able to
/// replace the file between the two steps would have the worker write frames through it
/// (found in the 2026-10-23 review as Q1). `O_NOFOLLOW` plus an owner and mode re-check
/// closes it for the cost of two syscalls.
#[cfg(unix)]
pub(super) fn private_reopen(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "preview record is not a regular file",
        ));
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "preview record is owned by another user",
        ));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "preview record is readable by other users",
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
pub(super) fn private_reopen(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}

#[cfg(not(unix))]
pub(super) fn private_create(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
pub(super) fn write_at(file: &File, offset: u64, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(bytes, offset)
}

#[cfg(unix)]
pub(super) fn read_at(file: &File, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(bytes, offset)
}

#[cfg(windows)]
pub(super) fn write_at(file: &File, offset: u64, bytes: &[u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < bytes.len() {
        done += file.seek_write(&bytes[done..], offset + done as u64)?;
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn read_at(file: &File, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < bytes.len() {
        let read = file.seek_read(&mut bytes[done..], offset + done as u64)?;
        if read == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        done += read;
    }
    Ok(())
}
