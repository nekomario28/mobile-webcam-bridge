pub mod cli;
pub mod client_auth;
pub mod control;
#[cfg(feature = "gui")]
pub mod gui;
pub mod headless;
pub mod identity;
#[cfg(feature = "video")]
pub mod output;
pub mod platform;
pub mod preview;
pub mod quality;
pub mod secure;
#[cfg(feature = "gui")]
pub mod session;
pub mod settings;
pub mod transport;
pub mod ttff;
pub mod usb;
pub mod usb_probe;
#[cfg(feature = "video")]
pub mod video;
pub mod wire;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Gui,
    Worker {
        parent: u32,
    },
    ReplaceIdentity {
        force: bool,
    },
    /// A session without a window, for scripts and CI.
    Headless(crate::headless::Options),
    /// USB end-to-end probe: AOA transition, authorization, PING/PONG round trips.
    UsbProbe(crate::usb_probe::Options),
    /// Print the raw bytes an attached phone sends, for diagnosing a frame that will not
    /// parse. Diagnostic only.
    UsbDump {
        seconds: u64,
    },
    /// Print the frames the real reader parses, for diagnosing a reader that cannot
    /// complete one. Diagnostic only.
    UsbFrames {
        seconds: u64,
    },
    Help,
}
