//! Launcher mode. Finder hands documents to an app as an
//! `application:openURLs:` Apple event, which winit drops. So a bundled
//! noido started without arguments runs this bare AppKit app instead: it
//! catches the event and starts one real noido per document through
//! NSWorkspace (which also brings its window to the front), then exits.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

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

/// The launcher's bookkeeping: which URLs wait for launching to finish,
/// how many launches are still on their way, and when the last one began.
#[derive(Default)]
struct Launches {
    finished: bool,
    waiting: Vec<String>,
    running: usize,
    last_start: Option<Instant>,
}

impl Launches {
    /// URLs from an open event: what to launch now. Before
    /// `applicationDidFinishLaunching` they wait for it; after, while the
    /// launcher still waits for its launches, Launch Services sends a
    /// second click here too, and it starts at once.
    fn opened(&mut self, urls: Vec<String>, now: Instant) -> Vec<String> {
        if self.finished {
            self.start(urls, now)
        } else {
            self.waiting.extend(urls);
            Vec::new()
        }
    }

    /// At `applicationDidFinishLaunching`: what to launch now. Empty means
    /// nothing was sent and the launcher is done.
    fn finished_launching(&mut self, now: Instant) -> Vec<String> {
        self.finished = true;
        let waiting = std::mem::take(&mut self.waiting);
        self.start(waiting, now)
    }

    fn start(&mut self, urls: Vec<String>, now: Instant) -> Vec<String> {
        if !urls.is_empty() {
            self.running += urls.len();
            self.last_start = Some(now);
        }
        urls
    }

    /// One launch has finished; true when it was the last one.
    fn launched(&mut self) -> bool {
        self.running = self.running.saturating_sub(1);
        self.running == 0
    }

    /// When the launcher gives up waiting for NSWorkspace.
    fn deadline(&self) -> Instant {
        self.last_start.unwrap_or_else(Instant::now) + GRACE
    }
}

struct Ivars {
    app: PathBuf,
    launches: Arc<Mutex<Launches>>,
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
            let texts = urls
                .iter()
                .filter_map(|url| url.absoluteString().map(|text| text.to_string()))
                .collect();
            let to_launch = self.launches().opened(texts, Instant::now());
            self.launch(to_launch);
        }

        // SAFETY: The signature is correct.
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let to_launch = self.launches().finished_launching(Instant::now());
            if to_launch.is_empty() {
                exit(0);
            }
            let launches = self.ivars().launches.clone();
            // Gives up on NSWorkspace once nothing has started for GRACE.
            thread::spawn(move || {
                loop {
                    let deadline = lock(&launches).deadline();
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        exit(0);
                    }
                    thread::sleep(left);
                }
            });
            self.launch(to_launch);
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, app: PathBuf) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            app,
            launches: Arc::new(Mutex::new(Launches::default())),
        });
        // SAFETY: The signature of NSObject's init is correct.
        #[allow(unsafe_code)]
        unsafe {
            msg_send![super(this), init]
        }
    }

    fn launches(&self) -> MutexGuard<'_, Launches> {
        lock(&self.ivars().launches)
    }

    /// Starts one GUI per URL; the launcher exits after the last one.
    fn launch(&self, urls: Vec<String>) {
        for url in urls {
            let launches = self.ivars().launches.clone();
            launch_in_bundle(&self.ivars().app, &[OsString::from(url)], move |result| {
                if let Err(e) = result {
                    eprintln!("noido: cannot open a document: {e}");
                }
                if lock(&launches).launched() {
                    exit(0);
                }
            });
        }
    }
}

/// The bookkeeping stays usable even if a panic poisoned its lock.
fn lock(launches: &Mutex<Launches>) -> MutexGuard<'_, Launches> {
    launches.lock().unwrap_or_else(|e| e.into_inner())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn urls_before_launching_finishes_wait_for_it() {
        let now = Instant::now();
        let mut launches = Launches::default();
        assert!(launches.opened(urls(&["a"]), now).is_empty());
        assert_eq!(launches.finished_launching(now), urls(&["a"]));
    }

    #[test]
    fn nothing_waiting_at_finish_launches_nothing() {
        let mut launches = Launches::default();
        assert!(launches.finished_launching(Instant::now()).is_empty());
    }

    #[test]
    fn a_url_after_launching_finished_is_launched_at_once() {
        let now = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a"]), now);
        launches.finished_launching(now);
        assert_eq!(launches.opened(urls(&["b", "c"]), now), urls(&["b", "c"]));
    }

    #[test]
    fn the_launcher_is_done_after_the_last_launch_late_ones_included() {
        let now = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a"]), now);
        launches.finished_launching(now);
        launches.opened(urls(&["b"]), now);
        assert!(!launches.launched());
        assert!(launches.launched());
    }

    #[test]
    fn the_timeout_counts_from_the_latest_start() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a"]), start);
        launches.finished_launching(start);
        assert_eq!(launches.deadline(), start + GRACE);
        let later = start + Duration::from_millis(80);
        launches.opened(urls(&["b"]), later);
        assert_eq!(launches.deadline(), later + GRACE);
    }
}
