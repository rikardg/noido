//! Starting noido.app through Launch Services.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};

/// Starts a new instance of the app at `app` with `args`, in front. `done`
/// runs from the completion handler, on some thread.
pub fn launch_in_bundle(
    app: &Path,
    args: &[OsString],
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&app.to_string_lossy()));
    let args: Vec<Retained<NSString>> = args
        .iter()
        .map(|a| NSString::from_str(&String::from_utf8_lossy(a.as_bytes())))
        .collect();
    let config = NSWorkspaceOpenConfiguration::configuration();
    config.setArguments(&NSArray::from_retained_slice(&args));
    config.setCreatesNewApplicationInstance(true);
    config.setActivates(true);
    let done = Mutex::new(Some(done));
    let handler = RcBlock::new(
        move |_app: *mut NSRunningApplication, error: *mut NSError| {
            let result = if error.is_null() {
                Ok(())
            } else {
                // SAFETY: AppKit passes a valid NSError when it is not null.
                #[allow(unsafe_code)]
                let error = unsafe { &*error };
                Err(error.localizedDescription().to_string())
            };
            if let Some(done) = done.lock().ok().and_then(|mut d| d.take()) {
                done(result);
            }
        },
    );
    NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
        &url,
        &config,
        Some(&handler),
    );
}
