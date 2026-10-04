#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Camera;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Camera;
