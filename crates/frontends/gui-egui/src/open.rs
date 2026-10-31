//! Opening another document: the desktop's own file picker, and the
//! window a picked file opens in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What a picked file opens in: a new window.
pub type OnPicked = Arc<dyn Fn(&Path) + Send + Sync>;

/// Picks a PDF in the desktop's own file dialog, handing it to
/// `on_picked`; nothing when cancelled.
///
/// The call blocks its thread until the desktop answers, so it runs on a
/// thread of its own: Mod+O returns to the window at once, and the window
/// keeps painting while the dialog is up.
pub fn pick_pdf(on_picked: OnPicked) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .add_filter("PDF document", &["pdf"])
            .pick_file();
        if let Some(path) = picked {
            on_picked(&path);
        }
    });
}

/// Opens `path` in a new noido window.
///
/// A file that is already open elsewhere is not opened twice: the new
/// process hands over to the window that has it and exits.
pub fn open(path: &Path) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("noido"));
    // Detached: the new window's output is not ours to show.
    let _ = std::process::Command::new(exe)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
