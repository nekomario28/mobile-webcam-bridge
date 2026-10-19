pub mod cli;
pub mod client_auth;
pub mod control;
#[cfg(feature = "gui")]
pub mod gui;
pub mod identity;
#[cfg(feature = "video")]
pub mod output;
pub mod platform;
pub mod quality;
pub mod secure;
#[cfg(feature = "gui")]
pub mod session;
pub mod settings;
pub mod transport;
pub mod usb;
#[cfg(feature = "video")]
pub mod video;
pub mod wire;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// What the process was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Gui,
    Worker { parent: u32 },
    ReplaceIdentity { force: bool },
    Help,
}
