//! noido on macOS. Empty elsewhere.

#[cfg(target_os = "macos")]
mod activate;

#[cfg(target_os = "macos")]
mod launch;

#[cfg(target_os = "macos")]
mod open_urls;

#[cfg(target_os = "macos")]
pub use activate::activate_pid;

#[cfg(target_os = "macos")]
pub use launch::{launch_in_bundle, run_launcher};

#[cfg(target_os = "macos")]
pub use open_urls::{install_open_urls, take_opened};
