//! Follows the document's file: when it changes on disk, the new version is
//! opened in the background and handed to the app.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use noido_engine::{PdfBytes, PdfEngine};
use noido_render::{DocumentInfo, Linker, Renderer, TextSearcher, Thumbnailer};
use noido_state::DocKey;
use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode};

use crate::load::load_document;

/// How long the file must stay unchanged before it is read: LaTeX tools
/// write the file in several steps.
const QUIET: Duration = Duration::from_millis(300);

/// How long the watching thread sleeps with nothing pending.
const IDLE: Duration = Duration::from_secs(3600);

/// Wakes the UI.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A new version of the document, opened and ready to show.
pub struct Reload {
    pub renderer: Renderer,
    pub thumbnailer: Thumbnailer,
    /// Renders the sidebar's thumbnails.
    pub sidebar: Renderer,
    pub searcher: TextSearcher,
    pub linker: Linker,
    pub info: DocumentInfo,
    pub key: DocKey,
    /// The new version's file contents, for reading its properties.
    pub bytes: PdfBytes,
}

/// The newest version not shown yet. A newer one replaces it, so versions
/// don't pile up while the window is hidden and the app isn't taking them.
#[derive(Clone, Default)]
pub struct Newest(Arc<Mutex<Option<Reload>>>);

impl Newest {
    pub fn put(&self, reload: Reload) {
        // The older version, and its threads, go.
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(reload);
    }

    pub fn take(&self) -> Option<Reload> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// Collapses a burst of file events into one read, once the file has been
/// quiet for a while.
pub struct Debounce {
    quiet: Duration,
    /// When the last event of the current burst arrived.
    last: Option<Instant>,
}

impl Debounce {
    pub fn new(quiet: Duration) -> Self {
        Self { quiet, last: None }
    }

    pub fn event(&mut self, now: Instant) {
        self.last = Some(now);
    }

    /// How long until the file may be read: `None` with no change pending.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.last
            .map(|last| (last + self.quiet).saturating_duration_since(now))
    }

    /// Whether the file may be read now; true once per burst.
    pub fn fire(&mut self, now: Instant) -> bool {
        let due = self.wait(now) == Some(Duration::ZERO);
        if due {
            self.last = None;
        }
        due
    }
}

/// Opens the file at `path` if its content differs from `current`. A missing,
/// empty or unreadable file is no reload: the shown version stays. Nor is a
/// document without pages, which a file cut short mid-write can open as.
fn open_new(
    engine: &Arc<dyn PdfEngine>,
    path: &Path,
    current: &DocKey,
    waker: &Waker,
) -> Option<Reload> {
    let bytes = load_document(path).ok()?;
    let key = DocKey::new(&bytes, path);
    if key.hex() == current.hex() {
        return None;
    }
    let boxed = |waker: &Waker| -> Box<dyn Fn() + Send> {
        let waker = waker.clone();
        Box::new(move || waker())
    };
    let (renderer, info) = Renderer::start(engine.clone(), bytes.clone(), boxed(waker)).ok()?;
    if info.page_sizes.is_empty() {
        return None;
    }
    let thumbnailer = Thumbnailer::start(engine.clone(), bytes.clone(), boxed(waker));
    let sidebar = Renderer::start_background(engine.clone(), bytes.clone(), boxed(waker));
    let linker = Linker::start(engine.clone(), bytes.clone(), boxed(waker));
    let searcher = TextSearcher::start(engine.clone(), bytes.clone(), boxed(waker));
    Some(Reload {
        renderer,
        thumbnailer,
        sidebar,
        searcher,
        linker,
        info,
        key,
        bytes,
    })
}

/// Watches the document's file; dropping it stops watching.
pub struct Watcher {
    /// `None` for a fake.
    _watcher: Option<RecommendedWatcher>,
    newest: Newest,
}

impl Watcher {
    /// Watches `path`, whose shown version is `current`. New versions are
    /// opened with `engine`; `waker` is called when one is ready.
    pub fn start(
        path: &Path,
        current: DocKey,
        engine: Arc<dyn PdfEngine>,
        waker: Waker,
    ) -> Result<Watcher, String> {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let (dir, name) = match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => (dir.to_path_buf(), name.to_owned()),
            _ => return Err("not a file".into()),
        };
        let (events_tx, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(events_tx).map_err(|e| e.to_string())?;
        // The directory, since many tools replace the file by renaming a new
        // one over it.
        notify::Watcher::watch(&mut watcher, &dir, RecursiveMode::NonRecursive)
            .map_err(|e| e.to_string())?;
        let newest = Newest::default();
        let shared = Arc::downgrade(&newest.0);
        let concerns = move |event: &Event| {
            event
                .paths
                .iter()
                .any(|p| p.file_name() == Some(name.as_os_str()))
        };
        thread::Builder::new()
            .name("noido-watch".into())
            .spawn(move || {
                run(&path, current, &engine, &waker, &events, &shared, concerns);
            })
            .map_err(|e| e.to_string())?;
        Ok(Watcher {
            _watcher: Some(watcher),
            newest,
        })
    }

    pub fn try_recv(&self) -> Option<Reload> {
        self.newest.take()
    }

    /// A watcher that hands over what is sent to it, for tests.
    #[cfg(test)]
    pub fn fake() -> (Newest, Watcher) {
        let newest = Newest::default();
        let watcher = Watcher {
            _watcher: None,
            newest: newest.clone(),
        };
        (newest, watcher)
    }
}

/// The watching thread: ends when the watcher or the app goes away.
fn run(
    path: &Path,
    mut current: DocKey,
    engine: &Arc<dyn PdfEngine>,
    waker: &Waker,
    events: &Receiver<notify::Result<Event>>,
    newest: &Weak<Mutex<Option<Reload>>>,
    concerns: impl Fn(&Event) -> bool,
) {
    let mut debounce = Debounce::new(QUIET);
    loop {
        let timeout = debounce.wait(Instant::now()).unwrap_or(IDLE);
        match events.recv_timeout(timeout) {
            // Reading the file ourselves causes access events; only a write
            // counts.
            Ok(Ok(event)) if is_change(&event.kind) && concerns(&event) => {
                debounce.event(Instant::now());
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if debounce.fire(Instant::now())
            && let Some(reload) = open_new(engine, path, &current, waker)
        {
            current = reload.key.clone();
            // The app has gone.
            let Some(slot) = newest.upgrade() else {
                return;
            };
            Newest(slot).put(reload);
            waker();
        }
    }
}

fn is_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};
    use noido_state::DocKey;

    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// A document with as many pages as its file has bytes; files starting
    /// with "bad" don't open, and files starting with "none" have no pages.
    struct ByteEngine;

    struct ByteDocument(usize);

    impl PdfEngine for ByteEngine {
        fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            if bytes.starts_with(b"bad") {
                return Err(EngineError::Open("bad".into()));
            }
            if bytes.starts_with(b"none") {
                return Ok(Box::new(ByteDocument(0)));
            }
            Ok(Box::new(ByteDocument(bytes.len())))
        }
    }

    impl Document for ByteDocument {
        fn page_count(&self) -> usize {
            self.0
        }
        fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
            Ok(Size {
                width: 100.0,
                height: 100.0,
            })
        }
        fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
            Ok(Rgba8Image::from_premultiplied(1, 1, vec![255; 4]).unwrap())
        }
    }

    fn engine() -> Arc<dyn PdfEngine> {
        Arc::new(ByteEngine)
    }

    fn no_waker() -> Waker {
        Arc::new(|| {})
    }

    #[test]
    fn events_within_quiet_period_fire_once() {
        let start = Instant::now();
        let mut debounce = Debounce::new(300 * MS);
        for at in [0, 100, 250] {
            debounce.event(start + at * MS);
        }
        assert!(!debounce.fire(start + 500 * MS));
        assert!(debounce.fire(start + 551 * MS));
        assert!(!debounce.fire(start + 600 * MS));
        assert_eq!(debounce.wait(start + 600 * MS), None);
    }

    #[test]
    fn wait_reports_time_left() {
        let start = Instant::now();
        let mut debounce = Debounce::new(300 * MS);
        assert_eq!(debounce.wait(start), None);
        debounce.event(start);
        assert_eq!(debounce.wait(start + 100 * MS), Some(200 * MS));
        assert_eq!(debounce.wait(start + 400 * MS), Some(Duration::ZERO));
    }

    #[test]
    fn debounce_rearms_after_fire() {
        let start = Instant::now();
        let mut debounce = Debounce::new(300 * MS);
        debounce.event(start);
        assert!(debounce.fire(start + 300 * MS));
        debounce.event(start + 400 * MS);
        assert!(!debounce.fire(start + 600 * MS));
        assert!(debounce.fire(start + 700 * MS));
    }

    #[test]
    fn unchanged_hash_is_not_a_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        fs::write(&path, b"four").unwrap();
        let current = DocKey::new(b"four", &path);
        assert!(open_new(&engine(), &path, &current, &no_waker()).is_none());
    }

    #[test]
    fn missing_file_is_not_a_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        let current = DocKey::new(b"four", &path);
        assert!(open_new(&engine(), &path, &current, &no_waker()).is_none());
    }

    #[test]
    fn unopenable_file_is_not_a_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        fs::write(&path, b"bad pdf").unwrap();
        let current = DocKey::new(b"four", &path);
        assert!(open_new(&engine(), &path, &current, &no_waker()).is_none());
    }

    /// While the window is hidden, versions wait unshown: only the newest
    /// is kept, each holding render threads and an open document.
    #[test]
    fn newest_replaces_unshown_version() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        let current = DocKey::new(b"one", &path);
        let newest = Newest::default();
        for bytes in [&b"three"[..], b"four!!"] {
            fs::write(&path, bytes).unwrap();
            newest.put(open_new(&engine(), &path, &current, &no_waker()).unwrap());
        }
        assert_eq!(newest.take().unwrap().info.page_sizes.len(), 6);
        assert!(newest.take().is_none());
    }

    /// A file cut short can open with no pages at all.
    #[test]
    fn document_without_pages_is_not_a_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        fs::write(&path, b"none yet").unwrap();
        let current = DocKey::new(b"four", &path);
        assert!(open_new(&engine(), &path, &current, &no_waker()).is_none());
    }

    #[test]
    fn changed_file_reloads() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        fs::write(&path, b"seven!!").unwrap();
        let current = DocKey::new(b"four", &path);
        let reload = open_new(&engine(), &path, &current, &no_waker()).unwrap();
        assert_ne!(reload.key.hex(), current.hex());
        assert_eq!(reload.key.hex(), DocKey::new(b"seven!!", &path).hex());
        assert_eq!(reload.info.page_sizes.len(), 7);
    }

    /// The next reload from `watcher` within `limit`.
    fn next_reload(watcher: &Watcher, limit: Duration) -> Option<Reload> {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Some(reload) = watcher.try_recv() {
                return Some(reload);
            }
            std::thread::sleep(10 * MS);
        }
        None
    }

    fn write_by_rename(path: &Path, bytes: &[u8]) {
        let temp = path.with_extension("tmp");
        fs::write(&temp, bytes).unwrap();
        fs::rename(&temp, path).unwrap();
    }

    #[test]
    fn watcher_follows_rename_over_and_recreate() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.pdf");
        fs::write(&path, b"one").unwrap();
        let watcher =
            Watcher::start(&path, DocKey::new(b"one", &path), engine(), no_waker()).unwrap();

        write_by_rename(&path, b"two!");
        let reload = next_reload(&watcher, Duration::from_secs(5)).expect("rename over");
        assert_eq!(reload.info.page_sizes.len(), 4);
        assert!(next_reload(&watcher, 500 * MS).is_none(), "reloaded twice");

        fs::remove_file(&path).unwrap();
        assert!(
            next_reload(&watcher, 500 * MS).is_none(),
            "reloaded a missing file"
        );
        fs::write(&path, b"three").unwrap();
        let reload = next_reload(&watcher, Duration::from_secs(5)).expect("recreated");
        assert_eq!(reload.info.page_sizes.len(), 5);
    }
}
