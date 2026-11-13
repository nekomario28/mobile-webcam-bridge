pub mod cli;
pub mod client_auth;
pub mod control;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
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
pub mod video_config;
pub mod wire;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Whether a developer trace variable (`AMB_AUTH_TRACE`, `AMB_USB_TRACE`) is set.
///
/// Traces exist only in debug builds or with the non-default `trace` feature; a
/// release build ignores the variables and contains no trace output at all.
#[cfg(any(debug_assertions, feature = "trace"))]
pub(crate) fn trace_enabled(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// Release builds without the `trace` feature: never.
#[cfg(not(any(debug_assertions, feature = "trace")))]
#[inline(always)]
pub(crate) fn trace_enabled(_name: &str) -> bool {
    false
}

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
        device: Option<(u16, u16)>,
    },
    /// Print the frames the real reader parses, for diagnosing a reader that cannot
    /// complete one. Diagnostic only.
    UsbFrames {
        seconds: u64,
        device: Option<(u16, u16)>,
    },
    Help,
}
