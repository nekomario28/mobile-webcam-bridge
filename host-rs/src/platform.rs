#[cfg(target_os = "linux")]
pub fn bind_parent(expected: u32) -> crate::Result<()> {
    let parent = unsafe { libc::getppid() };
    if parent <= 1 || parent as u32 != expected {
        return Err("worker parent exited".into());
    }
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { libc::getppid() } != parent {
        return Err("worker parent changed".into());
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub fn drop_to_parent(parent: u32) -> crate::Result<()> {
    use std::os::unix::fs::MetadataExt;
    bind_parent(parent)?;
    let owner = std::fs::metadata(format!("/proc/{parent}"))?;
    if owner.uid() == 0 {
        return Err("USB helper requires an unprivileged application parent".into());
    }
    if unsafe { libc::geteuid() } == 0
        && (unsafe { libc::setgroups(0, std::ptr::null()) } != 0
            || unsafe { libc::setresgid(owner.gid(), owner.gid(), owner.gid()) } != 0
            || unsafe { libc::setresuid(owner.uid(), owner.uid(), owner.uid()) } != 0)
    {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { libc::geteuid() } != owner.uid() {
        return Err("USB helper credential mismatch".into());
    }
    // Linux clears the death signal when credentials change.
    bind_parent(parent)
}
#[cfg(windows)]
pub fn bind_parent(_expected: u32) -> crate::Result<()> {
    Ok(())
}

#[cfg(windows)]
pub mod windows;
