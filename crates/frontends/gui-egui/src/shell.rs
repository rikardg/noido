//! What a document window asks of the shell around it (on macOS, the one
//! process that holds every window).

use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use noido_core::Target;
use noido_settings::Settings;

/// What a document window asks the shell to do.
#[derive(Debug)]
#[allow(dead_code)]
pub enum Request {
    /// Open `path` in a window, going to `target`.
    Open {
        path: PathBuf,
        target: Option<Target>,
        /// Where to say how it went, for a file asked for from outside.
        reply: Option<crate::instance::Reply>,
    },
    /// Show the file picker.
    OpenPanel,
    /// Close every window and quit.
    Quit,
    /// The settings were saved in a window; the others take them.
    SettingsSaved(Settings),
}

/// A way to send [`Request`]s to the shell and wake it: the shell's
/// hidden root only runs when woken.
#[derive(Clone)]
pub struct Requests {
    tx: mpsc::Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Requests {
    #[allow(dead_code)] // until the shell uses it
    pub fn new(tx: mpsc::Sender<Request>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tx, wake }
    }

    /// Sends `request` and wakes the shell. A shell that has gone is
    /// not an error: the window is going too.
    pub fn send(&self, request: Request) {
        let _ = self.tx.send(request);
        (self.wake)();
    }
}
