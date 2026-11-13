//! noido on macOS. Empty elsewhere.

#[cfg(target_os = "macos")]
mod activate;

#[cfg(target_os = "macos")]
mod defaults;

#[cfg(target_os = "macos")]
mod launch;

#[cfg(target_os = "macos")]
mod menu;

#[cfg(target_os = "macos")]
mod open_panel;

#[cfg(target_os = "macos")]
mod open_urls;

#[cfg(target_os = "macos")]
mod title_bar;

#[cfg(target_os = "macos")]
pub use activate::activate_pid;

#[cfg(target_os = "macos")]
pub use defaults::keeps_windows;

#[cfg(target_os = "macos")]
pub use launch::launch_in_bundle;

#[cfg(target_os = "macos")]
pub use menu::{MenuCommand, clear_recent, install_menu, note_recent, set_recent, take_commands};

#[cfg(target_os = "macos")]
pub use open_panel::pick_pdfs;

#[cfg(target_os = "macos")]
pub use open_urls::{install_open_urls, take_opened};

#[cfg(target_os = "macos")]
pub use title_bar::unify_title_bars;
