//! Opening another document: the desktop's own file picker, and the
//! window a picked file opens in.

use std::path::{Path, PathBuf};

/// Picks a PDF in the desktop's own file dialog: `None` when cancelled.
pub fn pick_pdf() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("PDF document", &["pdf"])
        .pick_file()
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
