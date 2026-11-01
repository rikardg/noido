//! Launcher mode. Finder hands documents to an app as an
//! `application:openURLs:` Apple event, which winit drops. So a bundled
//! noido started without arguments runs this bare AppKit app instead: it
//! catches the event and starts one real noido per document through
//! NSWorkspace (which also brings its window to the front), then exits.

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSRunningApplication,
    NSWorkspace, NSWorkspaceOpenConfiguration,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSError, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL,
};

/// How long the launcher waits for NSWorkspace before giving up.
const GRACE: Duration = Duration::from_secs(5);

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

struct Ivars {
    app: PathBuf,
    urls: RefCell<Vec<String>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Delegate has no Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: NSApplicationDelegate has no safety requirements.
    unsafe impl NSApplicationDelegate for Delegate {
        // SAFETY: The signature is correct.
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            let mut mine = self.ivars().urls.borrow_mut();
            for url in urls.iter() {
                if let Some(text) = url.absoluteString() {
                    mine.push(text.to_string());
                }
            }
        }

        // SAFETY: The signature is correct.
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let urls = self.ivars().urls.take();
            if urls.is_empty() {
                exit(0);
            }
            thread::spawn(|| {
                thread::sleep(GRACE);
                exit(0);
            });
            let remaining = Arc::new(AtomicUsize::new(urls.len()));
            for url in urls {
                let remaining = remaining.clone();
                launch_in_bundle(&self.ivars().app, &[OsString::from(url)], move |result| {
                    if let Err(e) = result {
                        eprintln!("noido: cannot open a document: {e}");
                    }
                    if remaining.fetch_sub(1, Ordering::SeqCst) == 1 {
                        exit(0);
                    }
                });
            }
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, app: PathBuf) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            app,
            urls: RefCell::new(Vec::new()),
        });
        // SAFETY: The signature of NSObject's init is correct.
        #[allow(unsafe_code)]
        unsafe {
            msg_send![super(this), init]
        }
    }
}

/// Runs the launcher for the bundle at `app` and exits the process.
pub fn run_launcher(app: &Path) -> ! {
    let mtm = MainThreadMarker::new().expect("the launcher runs on the main thread");
    let application = NSApplication::sharedApplication(mtm);
    // Keeps the launcher out of the Dock; it is gone within moments.
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    // AppKit holds its delegate weakly: this binding must outlive `run()`.
    let delegate = Delegate::new(mtm, app.to_owned());
    application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    application.run();
    exit(0)
}
