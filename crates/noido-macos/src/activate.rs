//! Bringing another noido's windows to the front. On macOS 14 and later
//! activation is cooperative: a window can't bring itself forward, but the
//! process that handed a file over to it can.

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

/// Brings all windows of the process `pid` to the front; nothing when
/// there is no such application.
pub fn activate_pid(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
}
