/// Output-device seam: tests can prove pipeline acceptance without claiming
/// that a native virtual camera or consuming application was exercised.
pub(crate) trait Sink {
    fn layout(&self) -> (usize, usize);
    fn ready(&mut self) -> crate::Result<bool>;
    fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()>;
}
impl Sink for Camera {
    fn layout(&self) -> (usize, usize) {
        (self.stride, self.size)
    }
    fn ready(&mut self) -> crate::Result<bool> {
        Camera::ready(self)
    }
    fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        Camera::submit(self, bytes, width, height)
    }
}
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Camera;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Camera;
