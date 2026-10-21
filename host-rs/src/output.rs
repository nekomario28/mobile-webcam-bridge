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
#[cfg(all(target_os = "linux", feature = "pipewire"))]
mod pipewire;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Camera;

/// Which virtual camera the frames go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// A `v4l2loopback` device: works for every application, needs a kernel module.
    Loopback,
    /// A PipeWire `Video/Source` node: no module, but only PipeWire-aware
    /// consumers see it.
    Pipewire,
}

impl Backend {
    /// The name stored in settings; unknown values are refused by validation.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "loopback" => Some(Self::Loopback),
            "pipewire" => Some(Self::Pipewire),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::Pipewire => "pipewire",
        }
    }

    /// Whether this build can actually serve that backend.
    pub fn available(self) -> bool {
        match self {
            Self::Loopback => cfg!(any(target_os = "linux", windows)),
            Self::Pipewire => cfg!(all(target_os = "linux", feature = "pipewire")),
        }
    }
}

/// The selected backend, erased behind the `Sink` seam.
pub enum Chosen {
    #[cfg(any(target_os = "linux", windows))]
    Loopback(Camera),
    #[cfg(all(target_os = "linux", feature = "pipewire"))]
    Pipewire(pipewire::Camera),
}

impl Sink for Chosen {
    fn layout(&self) -> (usize, usize) {
        match self {
            #[cfg(any(target_os = "linux", windows))]
            Self::Loopback(camera) => camera.layout(),
            #[cfg(all(target_os = "linux", feature = "pipewire"))]
            Self::Pipewire(camera) => camera.layout(),
        }
    }
    fn ready(&mut self) -> crate::Result<bool> {
        match self {
            #[cfg(any(target_os = "linux", windows))]
            Self::Loopback(camera) => camera.ready(),
            #[cfg(all(target_os = "linux", feature = "pipewire"))]
            Self::Pipewire(camera) => camera.ready(),
        }
    }
    fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        match self {
            #[cfg(any(target_os = "linux", windows))]
            Self::Loopback(camera) => camera.submit(bytes, width, height),
            #[cfg(all(target_os = "linux", feature = "pipewire"))]
            Self::Pipewire(camera) => camera.submit(bytes, width, height),
        }
    }
}

/// Open the configured virtual camera.
impl Chosen {
    /// Frame layout this backend expects: (stride, buffer size).
    pub fn frame_layout(&self) -> (usize, usize) {
        self.layout()
    }

    /// Whether a consumer is attached right now.
    pub fn consuming(&mut self) -> crate::Result<bool> {
        self.ready()
    }

    /// Hand over one converted frame.
    pub fn push(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        self.submit(bytes, width, height)
    }
}

pub fn open(
    backend: Backend,
    path: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> crate::Result<Chosen> {
    match backend {
        Backend::Loopback => Ok(Chosen::Loopback(Camera::open(path, width, height, fps)?)),
        Backend::Pipewire => {
            #[cfg(all(target_os = "linux", feature = "pipewire"))]
            {
                Ok(Chosen::Pipewire(pipewire::Camera::open(
                    path, width, height, fps,
                )?))
            }
            #[cfg(not(all(target_os = "linux", feature = "pipewire")))]
            {
                let _ = (path, width, height, fps);
                Err("this build has no PipeWire camera backend".into())
            }
        }
    }
}
