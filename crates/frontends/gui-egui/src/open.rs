//! Opening another document: the desktop's own file picker, and the
//! window a picked file opens in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Picks a PDF in the desktop's own dialog: noido's window goes along
/// as the dialog's parent, the dialog opens in the directory of the
/// open document when there is one, and a picked file is handed to
/// [`OnPicked`].
pub type Pick = Box<dyn Fn(Option<&eframe::Frame>, Option<PathBuf>, OnPicked) + Send>;

/// What a picked file opens in: a new window.
pub type OnPicked = Arc<dyn Fn(&Path) + Send + Sync>;

/// Picks a PDF in the desktop's own file dialog, handing it to
/// `on_picked`; nothing when cancelled.
///
/// The dialog is modal to noido's window: `set_parent` passes the window
/// to the backend, which on Windows owns the dialog to it and on Linux
/// hands it to the xdg portal as the `parent_window`, so the desktop
/// keeps the dialog above noido and the input with noido.
///
/// The call blocks its thread until the desktop answers, so it runs on a
/// thread of its own: Mod+O returns to the window at once, and the
/// window keeps painting while the dialog is up.
#[cfg(not(target_os = "macos"))]
pub fn pick_pdf(frame: Option<&eframe::Frame>, directory: Option<PathBuf>, on_picked: OnPicked) {
    // Built before the thread: borrowing the window ends here.
    let mut dialog = rfd::FileDialog::new().add_filter("PDF document", &["pdf"]);
    if let Some(frame) = frame {
        dialog = dialog.set_parent(frame);
    }
    if let Some(directory) = &directory {
        dialog = dialog.set_directory(directory);
    }
    std::thread::spawn(move || {
        if let Some(path) = dialog.pick_file() {
            on_picked(&path);
        }
    });
}

/// Picks PDFs in AppKit's Open panel, the one the app shows by itself
/// too, handing each to `on_picked`; nothing when cancelled. The panel
/// filters by the PDF type, and it does not block: this returns at once,
/// and every window keeps running while it is up. Main thread only, which
/// every caller is on.
#[cfg(target_os = "macos")]
pub fn pick_pdf(_frame: Option<&eframe::Frame>, directory: Option<PathBuf>, on_picked: OnPicked) {
    noido_macos::pick_pdfs(directory.as_deref(), move |paths| {
        for path in paths {
            on_picked(&path);
        }
    });
}

/// Opens `path` in a new noido window, a process of its own.
///
/// A file that is already open elsewhere is not opened twice: the new
/// process hands over to the window that has it and exits. Unused on
/// macOS, where every window opens files through the shell instead
/// ([`crate::app::NoidoApp::with_requests`]).
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
