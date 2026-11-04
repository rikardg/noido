//! Bringing the running noido to the front. On macOS 14 and later
//! activation is cooperative: an app can't bring itself forward, but the
//! process that handed a file over to it can.

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

/// Activates the process `pid`, bringing its key window to the front
/// and none of its others (the app focused the window it was asked
/// for); nothing when there is no such application.
pub fn activate_pid(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}
