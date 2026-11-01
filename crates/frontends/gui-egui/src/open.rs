//! Opening another document: the desktop's own file picker, and the
//! window a picked file opens in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Picks a PDF in the desktop's own dialog: noido's window goes along
/// as the dialog's parent, the dialog opens in the directory of the
/// open document when there is one, and a picked file is handed to
/// [`OnPicked`].
pub type Pick = Box<dyn Fn(Option<&eframe::Frame>, Option<PathBuf>, OnPicked)>;

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
pub fn pick_pdf(frame: Option<&eframe::Frame>, directory: Option<PathBuf>, on_picked: OnPicked) {
    // Built before the thread: borrowing the window ends here. macOS
    // builds its own inside instead — its raw window handle isn't
    // `Send`, so the dialog can't cross threads.
    #[cfg(not(target_os = "macos"))]
    let mut dialog = rfd::FileDialog::new().add_filter("PDF document", &["pdf"]);
    #[cfg(not(target_os = "macos"))]
    if let Some(frame) = frame {
        dialog = dialog.set_parent(frame);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(directory) = &directory {
        dialog = dialog.set_directory(directory);
    }
    #[cfg(target_os = "macos")]
    let _ = frame;
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let dialog = rfd::FileDialog::new().add_filter("PDF document", &["pdf"]);
        #[cfg(target_os = "macos")]
        let dialog = match directory {
            Some(directory) => dialog.set_directory(directory),
            None => dialog,
        };
        let picked = dialog.pick_file();
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
    // In the app bundle, NSWorkspace starts the window, in front.
    #[cfg(target_os = "macos")]
    if let Some(app) = crate::app_bundle::bundle_of(&exe) {
        noido_macos::launch_in_bundle(&app, &[path.into()], |_| {});
        return;
    }
    // Detached: the new window's output is not ours to show.
    let _ = std::process::Command::new(exe)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
