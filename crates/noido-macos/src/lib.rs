//! noido on macOS. Empty elsewhere.

#[cfg(target_os = "macos")]
mod launch;

#[cfg(target_os = "macos")]
pub use launch::{launch_in_bundle, run_launcher};
