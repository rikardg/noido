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
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSRunningApplication, NSWorkspace,
    NSWorkspaceOpenConfiguration,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSError, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL,
};

use crate::open_panel::pdf_type;

/// How long the launcher waits for NSWorkspace before giving up.
const GRACE: Duration = Duration::from_secs(5);

/// How long the launcher stays after its last launch has finished, so a
/// click Launch Services sends it then is still launched.
const QUIET: Duration = Duration::from_millis(300);

/// How often the launcher checks whether it is time to exit.
const TICK: Duration = Duration::from_millis(50);

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
/// how many launches are still on their way, and when the last one began
/// and ended.
#[derive(Default)]
struct Launches {
    finished: bool,
    waiting: Vec<String>,
    running: usize,
    last_start: Option<Instant>,
    last_done: Option<Instant>,
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

    /// Whether any launch has started.
    fn started(&self) -> bool {
        self.last_start.is_some()
    }

    /// One launch has finished, at `now`.
    fn launched(&mut self, now: Instant) {
        self.running = self.running.saturating_sub(1);
        self.last_done = Some(now);
    }

    /// When the launcher exits: GRACE after the latest start while
    /// launches are on their way, QUIET after the last one finished once
    /// none are.
    fn exit_at(&self) -> Instant {
        let start = self.last_start.unwrap_or_else(Instant::now);
        match self.last_done {
            Some(done) if self.running == 0 => done + QUIET,
            _ => start + GRACE,
        }
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
            let mut to_launch = self.launches().finished_launching(Instant::now());
            if to_launch.is_empty() {
                // The app itself was opened, without a document: ask for one.
                let picked = pick_pdfs(MainThreadMarker::from(self));
                to_launch = self.launches().opened(picked, Instant::now());
            }
            // Cancelled, and Finder sent nothing while the panel was up.
            if !self.launches().started() {
                exit(0);
            }
            let launches = self.ivars().launches.clone();
            // The one place the launcher exits once it has launched. It
            // checks often: the last launch finishing brings the time
            // forward.
            thread::spawn(move || {
                loop {
                    let launches = lock(&launches);
                    let left = launches.exit_at().saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        // Under the lock: no URL is counted and then lost.
                        exit(0);
                    }
                    drop(launches);
                    thread::sleep(left.min(TICK));
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

    /// Starts one GUI per URL.
    fn launch(&self, urls: Vec<String>) {
        for url in urls {
            let launches = self.ivars().launches.clone();
            launch_in_bundle(&self.ivars().app, &[OsString::from(url)], move |result| {
                if let Err(e) = result {
                    eprintln!("noido: cannot open a document: {e}");
                }
                lock(&launches).launched(Instant::now());
            });
        }
    }
}

/// Asks for PDFs in the standard Open panel, in front: the picked files'
/// URLs, none on Cancel.
fn pick_pdfs(mtm: MainThreadMarker) -> Vec<String> {
    // As an accessory app the launcher is never activated, and it has no
    // menus, so the panel would get no focus and its text fields no ⌘V.
    // For the panel it becomes a regular app, with a Dock icon.
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.setMainMenu(Some(&main_menu(mtm)));
    // NSApp.activate needs macOS 14; noido.app supports 11.
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setAllowsMultipleSelection(true);
    panel.setCanChooseDirectories(false);
    if let Some(pdf) = pdf_type() {
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[pdf]));
    }
    if panel.runModal() != NSModalResponseOK {
        return Vec::new();
    }
    panel
        .URLs()
        .iter()
        .filter_map(|url| url.absoluteString().map(|text| text.to_string()))
        .collect()
}

/// The menus the Open panel needs: Quit, and Edit, whose key equivalents
/// are how ⌘X, ⌘C, ⌘V and ⌘A reach the panel's text fields.
fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let app_menu = NSMenu::new(mtm);
    add_item(&app_menu, "Quit noido", sel!(terminate:), "q");
    let edit = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    for (title, action, key) in [
        ("Cut", sel!(cut:), "x"),
        ("Copy", sel!(copy:), "c"),
        ("Paste", sel!(paste:), "v"),
        ("Select All", sel!(selectAll:), "a"),
    ] {
        add_item(&edit, title, action, key);
    }
    let bar = NSMenu::new(mtm);
    for menu in [app_menu, edit] {
        let item = NSMenuItem::new(mtm);
        item.setSubmenu(Some(&menu));
        bar.addItem(&item);
    }
    bar
}

fn add_item(menu: &NSMenu, title: &str, action: Sel, key: &str) {
    // SAFETY: The actions are standard responder messages; with no target,
    // AppKit sends them along the responder chain to whoever answers them.
    #[allow(unsafe_code)]
    unsafe {
        menu.addItemWithTitle_action_keyEquivalent(
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        );
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
    fn nothing_has_started_until_a_launch_does() {
        let now = Instant::now();
        let mut launches = Launches::default();
        launches.finished_launching(now);
        assert!(!launches.started());
        // Picked in the Open panel, or sent by Finder while it was up.
        assert_eq!(launches.opened(urls(&["a"]), now), urls(&["a"]));
        assert!(launches.started());
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
    fn while_launches_run_the_timeout_counts_from_the_latest_start() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a"]), start);
        launches.finished_launching(start);
        assert_eq!(launches.exit_at(), start + GRACE);
        let later = start + Duration::from_millis(80);
        launches.opened(urls(&["b"]), later);
        assert_eq!(launches.exit_at(), later + GRACE);
    }

    #[test]
    fn one_launch_done_keeps_waiting_for_the_other() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a", "b"]), start);
        launches.finished_launching(start);
        launches.launched(start + Duration::from_millis(50));
        assert_eq!(launches.exit_at(), start + GRACE);
    }

    #[test]
    fn after_the_last_launch_the_launcher_stays_a_quiet_spell() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a"]), start);
        launches.finished_launching(start);
        let done = start + Duration::from_millis(50);
        launches.launched(done);
        assert_eq!(launches.exit_at(), done + QUIET);
    }

    #[test]
    fn a_url_in_the_quiet_spell_is_launched_and_waited_for() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.opened(urls(&["a"]), start);
        launches.finished_launching(start);
        launches.launched(start + Duration::from_millis(50));
        let late = start + Duration::from_millis(200);
        assert_eq!(launches.opened(urls(&["b"]), late), urls(&["b"]));
        assert_eq!(launches.exit_at(), late + GRACE);
        let done = late + Duration::from_millis(40);
        launches.launched(done);
        assert_eq!(launches.exit_at(), done + QUIET);
    }
}
