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
