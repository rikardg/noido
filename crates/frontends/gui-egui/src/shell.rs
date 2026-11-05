//! What a document window asks of the shell around it, and the shell: on
//! macOS, the one process that holds every document window, each a
//! deferred viewport of a hidden root.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportBuilder, ViewportCommand, ViewportId};
use noido_core::Target;
use noido_settings::Settings;
use noido_state::{Frame, Slot, Store};

use crate::app::{Incoming, NoidoApp};
use crate::instance::{Activation, Answer, AppRequests, Reply};
use crate::windows::Windows;

/// What a document window asks the shell to do.
#[derive(Debug)]
pub enum Request {
    /// Open `path` in a window, going to `target`.
    Open {
        path: PathBuf,
        target: Option<Target>,
        /// Where to say how it went, for a file asked for from outside.
        reply: Option<Reply>,
    },
    /// Show the file picker.
    OpenPanel,
    /// Close every window and quit.
    Quit,
    /// The settings were saved in a window; the others take them.
    SettingsSaved(Settings),
    /// The window was asked to close (its close button, Mod+W).
    Closed(ViewportId),
    /// A document's load, done on a worker, has finished.
    Loaded(Box<Loaded>),
}

/// A finished load: the document's app, or why it couldn't be built.
pub struct Loaded {
    /// The file, as the shell keys its windows.
    canonical: PathBuf,
    app: Result<NoidoApp, String>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("canonical", &self.canonical)
            .field("app", &self.app.as_ref().map(|_| "NoidoApp"))
            .finish()
    }
}

/// What the native menu (or a click on the Dock icon) asks of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum MenuCommand {
    /// The focused window's document info.
    About,
    /// The focused window's settings.
    Settings,
    /// The Open panel.
    Open,
    /// The app was opened again with no window showing.
    Reopen,
}

/// Where the shell takes the menu's commands from.
pub type TakeMenu = Box<dyn FnMut() -> Vec<MenuCommand>>;

/// A way to send [`Request`]s to the shell and wake it: the shell's
/// hidden root only runs when woken.
#[derive(Clone)]
pub struct Requests {
    tx: mpsc::Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Requests {
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

/// Builds a document's app for the window `viewport`; the message says
/// why it couldn't. Production: [`crate::document::open`]; tests: fakes.
/// It reads and hashes the whole file, so it runs on a worker thread.
pub type OpenDoc =
    Arc<dyn Fn(&Path, Option<Target>, ViewportId) -> Result<NoidoApp, String> + Send + Sync>;

/// A load to run off the main thread.
pub type Job = Box<dyn FnOnce() + Send>;

/// Runs a [`Job`]: on a thread of its own, or in tests when they say.
pub type Spawn = Box<dyn FnMut(Job)>;

/// Where to say how an open went, once the file was tried.
pub type Answerer = Box<dyn FnOnce(Answer)>;

/// A document being loaded, and who asked for it.
struct Loading {
    /// Every open asked while it loads, the one that started it first:
    /// where it goes, and where to answer.
    waiting: Vec<(Option<Target>, Option<Answerer>)>,
    /// For a document restored at the start: whether the Open panel was
    /// wanted before, to go back to if no restored document opens.
    panel_before: Option<bool>,
}

/// How many of its first frames the shell hides the root in: eframe shows
/// the root after its first frame, and hiding it every frame would keep
/// waking it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const HIDE_FRAMES: u32 = 3;

/// The documents to open at the start: the ones left open at the last
/// quit when the system keeps windows (`keeps`), then the one the launch
/// `named`, unless it is among them already.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn to_open(restored: Vec<PathBuf>, keeps: bool, named: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = if keeps { restored } else { Vec::new() };
    if let Some(named) = named {
        let canonical = Windows::<Window>::canonical(named);
        if !paths
            .iter()
            .any(|p| Windows::<Window>::canonical(p) == canonical)
        {
            paths.push(named.to_path_buf());
        }
    }
    paths
}

/// How long after launching the shell still waits for documents before
/// it shows the Open panel by itself: Finder's opens can come just after
/// launching has finished.
const START_QUIET: Duration = Duration::from_millis(300);

/// Whether an app opened by itself, with no document, shows the Open
/// panel: once launching has settled, if nothing was asked to open by
/// then (Finder's opens, a launch handing over, documents restored).
#[derive(Clone, Debug, Default)]
struct StartPanel {
    /// Still to decide: the app was opened with no document, and nothing
    /// has been asked to open since.
    wanted: bool,
    /// When launching has settled.
    settled_at: Option<Instant>,
}

impl StartPanel {
    /// For an app opened with no document.
    fn wanted() -> Self {
        Self {
            wanted: true,
            settled_at: None,
        }
    }

    /// Launching finished at `now`; only the first call counts. Whether
    /// the shell has to be woken when launching has settled.
    fn launched(&mut self, now: Instant) -> bool {
        if self.settled_at.is_some() {
            return false;
        }
        self.settled_at = Some(now + START_QUIET);
        self.wanted
    }

    /// Wanted again (`wanted`), as before the documents restored at the
    /// start, none of which opened. How long until the shell has to be
    /// woken to show it, if launching hasn't settled yet at `now`.
    fn restore(&mut self, wanted: bool, now: Instant) -> Option<Duration> {
        self.wanted = wanted;
        let at = self.settled_at.filter(|_| wanted)?;
        (at > now).then(|| at - now)
    }

    /// Something was asked to open: no panel.
    fn opened(&mut self) {
        self.wanted = false;
    }

    /// Whether to show the panel now; true once at most.
    fn due(&mut self, now: Instant) -> bool {
        let due = self.wanted && self.settled_at.is_some_and(|at| now >= at);
        if due {
            self.wanted = false;
        }
        due
    }
}

/// One document window.
struct Window {
    /// Locked by the window's own callback, which must be `Send + Sync`;
    /// every callback runs on the main thread, so it is never contended.
    app: Arc<Mutex<NoidoApp>>,
    title: String,
    activation: Activation,
    /// Where the document's state, and so its frame, is kept.
    kept: Option<(Store, Slot)>,
    /// The window's latest frame, written by its own callback every frame:
    /// when it closes its viewport info may already be gone.
    frame: Arc<Mutex<Option<Frame>>>,
    /// The frame the window opens with.
    opens_at: Option<Frame>,
    /// Whether the window takes the focus when it opens: a restored one
    /// doesn't while a document asked for is open or loading.
    active: bool,
    /// The window to give the focus back to once this one is up, which
    /// AppKit puts in front even when it opens without the focus.
    hand_focus: Arc<Mutex<Option<ViewportId>>>,
}

impl Window {
    /// Keeps the window's latest frame in its store.
    fn save_frame(&self) {
        let Some(frame) = *lock(&self.frame) else {
            return;
        };
        if let Some((store, slot)) = &self.kept
            && let Err(e) = store.save_frame(slot, frame)
        {
            eprintln!("noido: cannot save the window's place: {e}");
        }
    }
}

/// How a new window opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opens {
    /// In front, with the focus.
    InFront,
    /// Without the focus, behind a document asked for: the window given,
    /// once this one is up, or one still loading, which comes in front
    /// by itself.
    Behind(Option<ViewportId>),
}

/// The size a window opens with when nothing says otherwise.
const DEFAULT_WINDOW_SIZE: [f32; 2] = [900.0, 1100.0];

/// `builder` with the place and size a window opens with: `frame` if
/// there is one (its size at least the minimum), else the default size
/// wherever the system puts it.
fn window_builder(builder: ViewportBuilder, frame: Option<Frame>) -> ViewportBuilder {
    let builder = builder.with_min_inner_size(crate::MIN_WINDOW_SIZE);
    let Some(frame) = frame else {
        return builder.with_inner_size(DEFAULT_WINDOW_SIZE);
    };
    let min = egui::Vec2::from(crate::MIN_WINDOW_SIZE);
    let builder = builder.with_inner_size(egui::vec2(frame.width, frame.height).max(min));
    if frame.has_position() {
        builder.with_position(egui::pos2(frame.x, frame.y))
    } else {
        builder
    }
}

/// A viewport's frame as it is kept: the outer position and the inner
/// size, which is what [`window_builder`] gives back. None while the
/// window is minimized or full screen, or not yet placed.
fn frame_of(info: &egui::ViewportInfo) -> Option<Frame> {
    if info.minimized == Some(true) || info.fullscreen == Some(true) {
        return None;
    }
    let (outer, inner) = (info.outer_rect?, info.inner_rect?);
    Some(Frame {
        x: outer.min.x,
        y: outer.min.y,
        width: inner.width(),
        height: inner.height(),
    })
}

/// The app on macOS: every document window, and what feeds them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Shell {
    open_doc: OpenDoc,
    spawn: Spawn,
    /// Documents loading on workers, by canonical path, oldest first.
    loading: Vec<(PathBuf, Loading)>,
    requests: Requests,
    received: mpsc::Receiver<Request>,
    windows: Windows<Window>,
    /// Finder's opens.
    incoming: Option<Incoming>,
    /// The native menu's commands.
    menu: Option<TakeMenu>,
    /// Launches handing their file over.
    app_requests: Option<AppRequests>,
    theme: Option<crate::theme::Live>,
    /// Root frames run so far, up to [`HIDE_FRAMES`].
    frames: u32,
    /// The documents open at the last quit.
    open_at_quit: Option<Vec<PathBuf>>,
    /// Where the open documents are kept, to reopen at the next start.
    open_list: Option<noido_state::Store>,
    /// Whether to show the Open panel once launching has settled.
    start_panel: StartPanel,
    /// The document asked for last (not restored), by canonical path:
    /// restored windows stay behind it.
    requested: Option<PathBuf>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Shell {
    /// A shell with no windows. `requests` is the channel windows ask it
    /// through; sending on it wakes the root.
    pub fn new(
        ctx: &egui::Context,
        open_doc: OpenDoc,
        requests: (mpsc::Sender<Request>, mpsc::Receiver<Request>),
    ) -> Shell {
        let (tx, received) = requests;
        Shell {
            open_doc,
            spawn: Box::new(|job| {
                std::thread::Builder::new()
                    .name("noido-load".into())
                    .spawn(job)
                    .expect("cannot start a thread to load a document");
            }),
            loading: Vec::new(),
            requests: Requests::new(tx, root_waker(ctx)),
            received,
            windows: Windows::new(),
            incoming: None,
            menu: None,
            app_requests: None,
            theme: None,
            frames: 0,
            open_at_quit: None,
            open_list: None,
            start_panel: StartPanel::default(),
            requested: None,
        }
    }

    /// Runs loads with `spawn` instead of on threads of their own.
    #[cfg(test)]
    fn with_spawn(mut self, spawn: Spawn) -> Self {
        self.spawn = spawn;
        self
    }

    /// Opens what Finder sends.
    pub fn with_incoming(mut self, incoming: Incoming) -> Self {
        self.incoming = Some(incoming);
        self
    }

    /// Carries out the native menu's commands.
    pub fn with_menu(mut self, take: TakeMenu) -> Self {
        self.menu = Some(take);
        self
    }

    /// Shows the Open panel once launching has settled, unless a document
    /// was asked to open by then: for an app opened with no document.
    pub fn with_open_panel_at_start(mut self) -> Self {
        self.start_panel = StartPanel::wanted();
        self
    }

    /// Keeps the open documents in `store` at every quit.
    pub fn with_open_list(mut self, store: noido_state::Store) -> Self {
        self.open_list = Some(store);
        self
    }

    /// Opens what later launches hand over.
    pub fn with_app_requests(mut self, requests: AppRequests) -> Self {
        self.app_requests = Some(requests);
        self
    }

    /// Follows the OS theme for every window.
    pub fn with_theme(mut self, live: crate::theme::Live) -> Self {
        self.theme = Some(live);
        self
    }

    /// Where to ask the shell things from outside a window (Ctrl-C).
    pub fn requests(&self) -> Requests {
        self.requests.clone()
    }

    /// The open documents, in open order.
    pub fn open_paths(&self) -> Vec<PathBuf> {
        self.windows.paths()
    }

    /// Carries out `request`.
    pub fn handle(&mut self, ctx: &egui::Context, request: Request) {
        match request {
            Request::Open {
                path,
                target,
                reply,
            } => {
                let answer = reply.map(|reply| -> Answerer { Box::new(move |a| reply.send(a)) });
                self.open(ctx, &path, target, answer);
            }
            Request::Loaded(loaded) => self.loaded(ctx, loaded),
            Request::OpenPanel => {
                let requests = self.requests.clone();
                crate::open::pick_pdf(
                    None,
                    None,
                    Arc::new(move |path| {
                        requests.send(Request::Open {
                            path: path.to_path_buf(),
                            target: None,
                            reply: None,
                        });
                    }),
                );
            }
            Request::Quit => {
                self.shut_down();
                paint_root(ctx);
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
            }
            Request::SettingsSaved(settings) => {
                for (_, window) in self.windows.iter() {
                    lock(&window.app).set_settings(settings);
                }
                // A window not in focus paints only when asked.
                for (id, _) in self.windows.iter() {
                    ctx.request_repaint_of(id);
                }
            }
            Request::Closed(id) => {
                if let Some(window) = self.windows.remove(id) {
                    lock(&window.app).save_now();
                    window.save_frame();
                    paint_root(ctx);
                }
            }
        }
    }

    /// Carries out the menu's `command`.
    pub fn menu(&mut self, ctx: &egui::Context, command: MenuCommand) {
        let action = match command {
            MenuCommand::Open => {
                self.requests.send(Request::OpenPanel);
                return;
            }
            // A window, if only a minimized one, AppKit brings back itself.
            MenuCommand::Reopen => {
                if self.windows.is_empty() {
                    self.requests.send(Request::OpenPanel);
                }
                return;
            }
            MenuCommand::Settings => noido_core::Action::OpenSettings,
            MenuCommand::About => noido_core::Action::ShowDocumentInfo,
        };
        let Some((id, window)) = self.windows.iter().find(|(id, _)| focused(ctx, *id)) else {
            return;
        };
        // In the window's own frame, where its keys are carried out too.
        lock(&window.app).queue(action);
        ctx.request_repaint_of(id);
    }

    /// Opens `path` in a new window, or brings forward the window that has
    /// it, and says how it went to `answer`. A new document loads on a
    /// worker: its window comes, and `answer` is told, once it has loaded;
    /// an open of a file still loading waits for that load.
    pub fn open(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        target: Option<Target>,
        answer: Option<Answerer>,
    ) {
        self.start_load(ctx, path, target, answer, None);
    }

    /// Opens `path` as [`Shell::open`] does, but loads it here and now:
    /// for the file a launch names, whose failure fails the launch.
    pub fn open_now(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        target: Option<Target>,
    ) -> Result<Answer, String> {
        self.start_panel.opened();
        let canonical = Windows::<Window>::canonical(path);
        self.requested = Some(canonical.clone());
        if let Some(id) = self.windows.find(&canonical) {
            self.forward(ctx, id, target);
            return Ok(Answer::Forward { pid: None });
        }
        let app = (self.open_doc)(path, target, Windows::<Window>::id_for(&canonical))?;
        self.insert(ctx, canonical, app, Opens::InFront);
        Ok(Answer::Opened { pid: None })
    }

    /// Starts loading `path` on a worker, unless it is open or loading
    /// already. `panel_before` is for a document restored at the start.
    fn start_load(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        target: Option<Target>,
        answer: Option<Answerer>,
        panel_before: Option<bool>,
    ) {
        // Counts from when the load starts: a big file doesn't let the
        // Open panel come up while it loads.
        self.start_panel.opened();
        let canonical = Windows::<Window>::canonical(path);
        if panel_before.is_none() {
            self.requested = Some(canonical.clone());
        }
        if let Some(id) = self.windows.find(&canonical) {
            self.forward(ctx, id, target);
            if let Some(answer) = answer {
                answer(Answer::Forward { pid: None });
            }
            return;
        }
        if let Some((_, loading)) = self.loading.iter_mut().find(|(p, _)| *p == canonical) {
            loading.waiting.push((target, answer));
            return;
        }
        self.loading.push((
            canonical.clone(),
            Loading {
                waiting: vec![(target.clone(), answer)],
                panel_before,
            },
        ));
        let open_doc = self.open_doc.clone();
        let requests = self.requests.clone();
        let path = path.to_path_buf();
        (self.spawn)(Box::new(move || {
            let id = Windows::<Window>::id_for(&canonical);
            // A load that panics is still answered, and can be tried again.
            let app = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                open_doc(&path, target, id)
            }))
            .unwrap_or_else(|_| Err(format!("{}: cannot open the file", path.display())));
            requests.send(Request::Loaded(Box::new(Loaded { canonical, app })));
        }));
    }

    /// Puts up the window of a finished load, or reports why it failed,
    /// to everyone who asked for it.
    fn loaded(&mut self, ctx: &egui::Context, loaded: Box<Loaded>) {
        let Some(at) = self
            .loading
            .iter()
            .position(|(p, _)| *p == loaded.canonical)
        else {
            return;
        };
        let (canonical, loading) = self.loading.remove(at);
        let mut waiting = loading.waiting.into_iter();
        match loaded.app {
            Ok(app) => {
                let opens = self.opens(&canonical, loading.panel_before.is_some());
                let id = self.insert(ctx, canonical, app, opens);
                // The first built the app, going to its target.
                if let Some((_, Some(answer))) = waiting.next() {
                    answer(Answer::Opened { pid: None });
                }
                for (target, answer) in waiting {
                    self.forward(ctx, id, target);
                    if let Some(answer) = answer {
                        answer(Answer::Forward { pid: None });
                    }
                }
            }
            Err(e) => {
                let mut unanswered = false;
                for (_, answer) in waiting {
                    match answer {
                        Some(answer) => answer(Answer::Failed(e.clone())),
                        None => unanswered = true,
                    }
                }
                if unanswered {
                    eprintln!("noido: {e}");
                }
                if let Some(before) = loading.panel_before
                    && self.windows.is_empty()
                    && self.loading.is_empty()
                    && let Some(wait) = self.start_panel.restore(before, Instant::now())
                {
                    self.wake_after(wait);
                }
            }
        }
    }

    /// Brings the window `id` forward, going to `target`.
    fn forward(&self, ctx: &egui::Context, id: ViewportId, target: Option<Target>) {
        if let Some((_, window)) = self.windows.iter().find(|(i, _)| *i == id) {
            window.activation.request(target);
        }
        // An occluded or minimized window gets no frames, so it cannot
        // come forward by itself: the shell brings it.
        ctx.send_viewport_cmd_to(id, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
    }

    /// How the window of `canonical`, `restored` or not, opens: behind
    /// the document asked for last while that is open or loading.
    fn opens(&self, canonical: &Path, restored: bool) -> Opens {
        let Some(requested) = self
            .requested
            .as_ref()
            .filter(|r| restored && *r != canonical)
        else {
            return Opens::InFront;
        };
        match self.windows.find(requested) {
            Some(id) => Opens::Behind(Some(id)),
            None if self.loading.iter().any(|(p, _)| p == requested) => Opens::Behind(None),
            None => Opens::InFront,
        }
    }

    /// Puts `app` in a new window, opening as `opens` says, and has the
    /// root show it.
    fn insert(
        &mut self,
        ctx: &egui::Context,
        canonical: PathBuf,
        app: NoidoApp,
        opens: Opens,
    ) -> ViewportId {
        let kept = app.persistence();
        let opens_at = kept
            .as_ref()
            .and_then(|(store, slot)| store.load_frame(slot));
        let activation = Activation::default();
        let title = format!("{} — noido", app.name());
        let app = app
            .with_requests(self.requests.clone())
            .with_activation(activation.clone());
        let id = self.windows.insert(
            canonical,
            Window {
                app: Arc::new(Mutex::new(app)),
                title,
                activation,
                kept,
                frame: Arc::default(),
                opens_at,
                active: opens == Opens::InFront,
                hand_focus: Arc::new(Mutex::new(match opens {
                    Opens::Behind(window) => window,
                    Opens::InFront => None,
                })),
            },
        );
        paint_root(ctx);
        id
    }

    /// Saves every window and notes which are open, for a quit; a
    /// document still loading counts as open.
    fn shut_down(&mut self) {
        for (_, window) in self.windows.iter() {
            lock(&window.app).save_now();
            window.save_frame();
        }
        let mut open = self.open_paths();
        open.extend(self.loading.iter().map(|(p, _)| p.clone()));
        self.record_open(open);
    }

    /// Keeps the documents open at a quit, to reopen on the next start.
    fn record_open(&mut self, paths: Vec<PathBuf>) {
        if let Some(store) = &self.open_list
            && let Err(e) = store.save_open(&paths)
        {
            eprintln!("noido: cannot save the open documents: {e}");
        }
        self.open_at_quit = Some(paths);
    }

    /// Starts loading `paths`, as documents restored at the start; their
    /// windows come in the order they finish, behind a document asked
    /// for meanwhile (the one a launch names, Finder's). One that fails to open is
    /// reported and skipped. They count as opened for the Open panel
    /// while they load, and after only if one did open.
    pub fn open_all_quietly(&mut self, ctx: &egui::Context, paths: &[PathBuf]) {
        let before = self.start_panel.wanted;
        for path in paths {
            self.start_load(ctx, path, None, None, Some(before));
        }
    }

    /// Takes in what has arrived from outside and from the windows.
    fn receive(&mut self, ctx: &egui::Context) {
        let mut arrived = Vec::new();
        if let Some(incoming) = &mut self.incoming {
            for arg in (incoming.take)() {
                match noido_core::parse_argument(&arg) {
                    Ok(opening) => arrived.push(Request::Open {
                        path: opening.path,
                        target: opening.target,
                        reply: None,
                    }),
                    Err(e) => eprintln!("noido: {e}"),
                }
            }
        }
        let commands = self.menu.as_mut().map(|take| take()).unwrap_or_default();
        for command in commands {
            self.menu(ctx, command);
        }
        if let Some(requests) = &self.app_requests {
            for (request, reply) in requests.take() {
                arrived.push(Request::Open {
                    path: request.path,
                    target: request.target,
                    reply: Some(reply),
                });
            }
        }
        for request in arrived {
            self.handle(ctx, request);
        }
        // Until none is left: a load that finishes at once sends its
        // result while this runs.
        while let Ok(request) = self.received.try_recv() {
            self.handle(ctx, request);
        }
    }

    /// Asks for the Open panel once launching has settled at `now`, if
    /// the app was opened with no document and none has come since; the
    /// first call is when launching finished.
    fn settle_start(&mut self, now: Instant) {
        if self.start_panel.launched(now) {
            self.wake_after(START_QUIET);
        }
        if self.start_panel.due(now) {
            self.requests.send(Request::OpenPanel);
        }
    }

    /// Wakes the root after `wait`: the hidden root runs only when woken.
    fn wake_after(&self, wait: Duration) {
        let wake = self.requests.wake.clone();
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            wake();
        });
    }

    /// Shows every window, as a deferred viewport of the root.
    fn show_windows(&self, ctx: &egui::Context) {
        for (id, window) in self.windows.iter() {
            let app = window.app.clone();
            let requests = self.requests.clone();
            let frame = window.frame.clone();
            let hand_focus = window.hand_focus.clone();
            let mut builder = crate::own_title_bar(window_builder(
                ViewportBuilder::default().with_title(window.title.clone()),
                window.opens_at,
            ));
            if !window.active {
                builder = builder.with_active(false);
            }
            ctx.show_viewport_deferred(id, builder, move |ui, _class| {
                // On the first frame the window is up, in front. Not to a
                // window gone meanwhile: eframe would make it again.
                if let Some(to) = lock(&hand_focus).take()
                    && ui.ctx().input(|i| i.raw.viewports.contains_key(&to))
                {
                    ui.ctx().send_viewport_cmd_to(to, ViewportCommand::Focus);
                }
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    requests.send(Request::Closed(id));
                }
                if let Some(now) = ui.ctx().input(|i| frame_of(i.viewport())) {
                    *lock(&frame) = Some(now);
                }
                let mut app = lock(&app);
                app.logic_pass(ui.ctx());
                app.show(ui, None);
            });
        }
    }
}

impl eframe::App for Shell {
    /// The root's pass: it runs only when woken, so everything that feeds
    /// the shell wakes the root.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.frames < HIDE_FRAMES {
            self.frames += 1;
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(false));
        }
        if let Some(live) = &mut self.theme
            && live.poll(ctx)
        {
            for (id, _) in self.windows.iter() {
                ctx.request_repaint_of(id);
            }
        }
        self.receive(ctx);
        self.settle_start(Instant::now());
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show_windows(ui.ctx());
    }

    /// Quit from anywhere ends here: Close to the root, AppKit's
    /// `terminate:`.
    fn on_exit(&mut self) {
        self.shut_down();
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// Gives the root a full pass, `ui` included, even while it and every
/// window are hidden: only there are windows shown and dropped. Woken
/// alone, a hidden root runs `logic` only.
fn paint_root(ctx: &egui::Context) {
    ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::RequestPaintWhileHidden);
}

/// Whether the window `id` has the keyboard focus, as of its last frame.
fn focused(ctx: &egui::Context, id: ViewportId) -> bool {
    ctx.input(|i| i.raw.viewports.get(&id).and_then(|info| info.focused)) == Some(true)
}

/// Wakes the root, and so the shell.
fn root_waker(ctx: &egui::Context) -> Arc<dyn Fn() + Send + Sync> {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint_of(ViewportId::ROOT))
}

fn lock<T>(app: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    app.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use eframe::egui::RawInput;
    use noido_state::{DocKey, Frame, Store};

    use super::*;
    use crate::app::tests::{blank_app, settings_of};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures")
            .join(name)
    }

    /// Documents that are blank apps, each saving its state in `store` if
    /// given; a file that doesn't exist fails to open. `loads` counts them.
    fn fake_open_doc(store: Option<PathBuf>, loads: Arc<AtomicUsize>) -> OpenDoc {
        Arc::new(move |path, _target, _id| {
            loads.fetch_add(1, Ordering::SeqCst);
            if !path.exists() {
                return Err(format!("{}: no such file", path.display()));
            }
            let app = blank_app().with_name(path.file_name().unwrap().to_string_lossy().into());
            Ok(match &store {
                Some(dir) => {
                    let slot = Store::at(dir).load(DocKey::new(b"x", path)).0;
                    app.with_persistence(Store::at(dir), slot, None)
                }
                None => app,
            })
        })
    }

    /// A shell of fake documents whose loads finish at once.
    fn shell_with(ctx: &egui::Context, store: Option<PathBuf>) -> Shell {
        Shell::new(ctx, fake_open_doc(store, Arc::default()), mpsc::channel())
            .with_spawn(Box::new(|job| job()))
    }

    /// Loads waiting for the test to run them.
    type Jobs = Arc<Mutex<Vec<Job>>>;

    /// A shell of fake documents whose loads wait until the test runs
    /// them, and how many loads were made.
    fn held_shell(ctx: &egui::Context) -> (Shell, Jobs, Arc<AtomicUsize>) {
        let jobs: Jobs = Arc::default();
        let loads: Arc<AtomicUsize> = Arc::default();
        let held = jobs.clone();
        let shell = Shell::new(ctx, fake_open_doc(None, loads.clone()), mpsc::channel())
            .with_spawn(Box::new(move |job| held.lock().unwrap().push(job)));
        (shell, jobs, loads)
    }

    /// Runs the waiting load `at` (in start order), as its worker would.
    fn finish(jobs: &Jobs, at: usize) {
        let job = jobs.lock().unwrap().remove(at);
        job();
    }

    /// Where to answer, and what was answered there.
    fn recorder() -> (Answerer, Arc<Mutex<Vec<Answer>>>) {
        let answers: Arc<Mutex<Vec<Answer>>> = Arc::default();
        let into = answers.clone();
        (
            Box::new(move |answer| into.lock().unwrap().push(answer)),
            answers,
        )
    }

    /// Carries out `request` and takes in what it led to, as the root's
    /// next pass would.
    fn handle_now(shell: &mut Shell, ctx: &egui::Context, request: Request) {
        shell.handle(ctx, request);
        shell.receive(ctx);
    }

    fn open(path: PathBuf, target: Option<Target>) -> Request {
        Request::Open {
            path,
            target,
            reply: None,
        }
    }

    /// The commands the context holds for the root.
    fn root_commands(ctx: &egui::Context) -> Vec<ViewportCommand> {
        let output = ctx.run_ui(RawInput::default(), |_| {});
        let commands = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.commands.clone())
            .unwrap_or_default();
        output.drop_without_applying_deltas();
        commands
    }

    #[test]
    fn opening_a_new_file_adds_a_window_and_answers_opened() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let path = fixture("text.pdf");
        let (answer, answers) = recorder();
        shell.open(&ctx, &path, None, Some(answer));
        shell.receive(&ctx);
        assert_eq!(*answers.lock().unwrap(), vec![Answer::Opened { pid: None }]);
        assert_eq!(
            shell.open_paths(),
            vec![Windows::<Window>::canonical(&path)]
        );
        let (_, window) = shell.windows.iter().next().unwrap();
        assert_eq!(window.title, "text.pdf — noido");
    }

    #[test]
    fn opening_an_open_file_brings_it_forward() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        // Another spelling of the same file.
        let again = fixture("../fixtures/text.pdf");
        let (answer, answers) = recorder();
        shell.open(&ctx, &again, Some(Target::Page(1)), Some(answer));
        assert_eq!(
            *answers.lock().unwrap(),
            vec![Answer::Forward { pid: None }]
        );
        assert_eq!(shell.open_paths().len(), 1);
        let (id, window) = shell.windows.iter().next().unwrap();
        let request = window.activation.take().unwrap();
        assert_eq!(request.target, Some(Target::Page(1)));
        // A minimized window gets no frames to come forward by itself.
        ctx.set_embed_viewports(false);
        let output = ctx.run_ui(RawInput::default(), |ui| shell.show_windows(ui.ctx()));
        let commands = output.viewport_output[&id].commands.clone();
        output.drop_without_applying_deltas();
        assert!(commands.contains(&ViewportCommand::Minimized(false)));
        assert!(commands.contains(&ViewportCommand::Focus));
    }

    #[test]
    fn a_file_that_fails_answers_failed_and_opens_nothing() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let missing = fixture("missing.pdf");
        let (answer, answers) = recorder();
        shell.open(&ctx, &missing, None, Some(answer));
        shell.receive(&ctx);
        assert!(
            matches!(&answers.lock().unwrap()[..], [Answer::Failed(e)] if e.contains("missing.pdf"))
        );
        handle_now(&mut shell, &ctx, open(missing, None));
        assert!(shell.open_paths().is_empty());
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        assert_eq!(shell.open_paths().len(), 1);
    }

    #[test]
    fn a_document_loads_off_the_root_pass_and_is_answered_once_tried() {
        let ctx = egui::Context::default();
        let _ = ctx.run_logic(&RawInput::default(), |_| {});
        let (mut shell, jobs, loads) = held_shell(&ctx);
        let path = fixture("text.pdf");
        let (answer, answers) = recorder();
        shell.open(&ctx, &path, None, Some(answer));
        // Nothing loaded in the root pass: the load waits for its worker.
        assert_eq!(loads.load(Ordering::SeqCst), 0);
        assert_eq!(jobs.lock().unwrap().len(), 1);
        shell.receive(&ctx);
        assert!(shell.open_paths().is_empty());
        assert!(answers.lock().unwrap().is_empty(), "answered before tried");
        let _ = root_commands(&ctx);
        finish(&jobs, 0);
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        // The worker's result wakes the root, which puts up the window.
        assert!(ctx.has_requested_repaint_for(&ViewportId::ROOT));
        shell.receive(&ctx);
        assert_eq!(
            shell.open_paths(),
            vec![Windows::<Window>::canonical(&path)]
        );
        assert_eq!(*answers.lock().unwrap(), vec![Answer::Opened { pid: None }]);
        assert!(root_commands(&ctx).contains(&ViewportCommand::RequestPaintWhileHidden));
    }

    #[test]
    fn a_second_open_while_loading_waits_and_then_brings_the_window_forward() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, loads) = held_shell(&ctx);
        let path = fixture("text.pdf");
        let (first, first_answers) = recorder();
        let (second, second_answers) = recorder();
        shell.open(&ctx, &path, None, Some(first));
        // Another spelling of the same file, while the first still loads.
        let again = fixture("../fixtures/text.pdf");
        shell.open(&ctx, &again, Some(Target::Page(1)), Some(second));
        assert_eq!(jobs.lock().unwrap().len(), 1, "loaded twice");
        assert!(second_answers.lock().unwrap().is_empty());
        finish(&jobs, 0);
        shell.receive(&ctx);
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(shell.open_paths().len(), 1);
        assert_eq!(
            *first_answers.lock().unwrap(),
            vec![Answer::Opened { pid: None }]
        );
        assert_eq!(
            *second_answers.lock().unwrap(),
            vec![Answer::Forward { pid: None }]
        );
        let (_, window) = shell.windows.iter().next().unwrap();
        assert_eq!(
            window.activation.take().unwrap().target,
            Some(Target::Page(1))
        );
    }

    #[test]
    fn a_failed_load_answers_everyone_waiting_and_opens_nothing() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, _) = held_shell(&ctx);
        let missing = fixture("missing.pdf");
        let (first, first_answers) = recorder();
        let (second, second_answers) = recorder();
        shell.open(&ctx, &missing, None, Some(first));
        shell.open(&ctx, &missing, None, Some(second));
        finish(&jobs, 0);
        shell.receive(&ctx);
        assert!(shell.open_paths().is_empty());
        for answers in [first_answers, second_answers] {
            assert!(
                matches!(&answers.lock().unwrap()[..], [Answer::Failed(e)] if e.contains("missing.pdf"))
            );
        }
        // Tried and failed: a later open loads it again.
        shell.open(&ctx, &missing, None, None);
        assert_eq!(jobs.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_load_that_panics_answers_failed() {
        let ctx = egui::Context::default();
        let open_doc: OpenDoc = Arc::new(|_, _, _| panic!("a broken file"));
        let mut shell =
            Shell::new(&ctx, open_doc, mpsc::channel()).with_spawn(Box::new(|job| job()));
        let path = fixture("text.pdf");
        let (answer, answers) = recorder();
        shell.open(&ctx, &path, None, Some(answer));
        shell.receive(&ctx);
        assert!(
            matches!(&answers.lock().unwrap()[..], [Answer::Failed(e)] if e.contains("text.pdf"))
        );
        assert!(shell.loading.is_empty());
    }

    #[test]
    fn the_file_a_launch_names_opens_at_once() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, loads) = held_shell(&ctx);
        let path = fixture("text.pdf");
        assert_eq!(
            shell.open_now(&ctx, &path, None),
            Ok(Answer::Opened { pid: None })
        );
        assert!(jobs.lock().unwrap().is_empty());
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(shell.open_paths().len(), 1);
        assert!(matches!(
            shell.open_now(&ctx, &fixture("missing.pdf"), None),
            Err(e) if e.contains("missing.pdf")
        ));
        assert_eq!(shell.open_paths().len(), 1);
    }

    #[test]
    fn restored_documents_all_appear_in_the_order_they_finish_loading() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, _) = held_shell(&ctx);
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        shell.open_all_quietly(&ctx, &[a.clone(), b.clone()]);
        assert!(shell.open_paths().is_empty());
        finish(&jobs, 1);
        shell.receive(&ctx);
        finish(&jobs, 0);
        shell.receive(&ctx);
        assert_eq!(
            shell.open_paths(),
            vec![
                Windows::<Window>::canonical(&b),
                Windows::<Window>::canonical(&a)
            ]
        );
    }

    /// Runs a root pass showing the windows: each window's builder and
    /// callback, by id.
    fn shown(
        shell: &Shell,
        ctx: &egui::Context,
    ) -> egui::OrderedViewportIdMap<egui::ViewportOutput> {
        ctx.set_embed_viewports(false);
        let output = ctx.run_ui(RawInput::default(), |ui| shell.show_windows(ui.ctx()));
        let viewports = output.viewport_output.clone();
        output.drop_without_applying_deltas();
        viewports
    }

    /// Runs the first frame of the window `id`, as eframe would once the
    /// window exists: the commands it sends, by viewport.
    fn first_frame(
        shell: &Shell,
        ctx: &egui::Context,
        id: ViewportId,
    ) -> egui::OrderedViewportIdMap<egui::ViewportOutput> {
        let callback = shown(shell, ctx)[&id].viewport_ui_cb.clone().unwrap();
        // A window's frame paints with noido's theme.
        crate::configure(ctx);
        // The windows eframe has, as each pass is told.
        let viewports = shell
            .windows
            .iter()
            .map(|(id, _)| (id, egui::ViewportInfo::default()))
            .collect();
        let output = ctx.run_ui(
            RawInput {
                viewport_id: id,
                viewports,
                ..Default::default()
            },
            |ui| callback(ui),
        );
        let viewports = output.viewport_output.clone();
        output.drop_without_applying_deltas();
        viewports
    }

    fn commands_to(
        viewports: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
        id: ViewportId,
    ) -> Vec<ViewportCommand> {
        viewports
            .get(&id)
            .map(|v| v.commands.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_restored_document_after_the_named_one_leaves_it_in_front() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, _) = held_shell(&ctx);
        let (named, restored) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        shell.open_now(&ctx, &named, None).unwrap();
        shell.open_all_quietly(&ctx, std::slice::from_ref(&restored));
        finish(&jobs, 0);
        shell.receive(&ctx);
        let viewports = shown(&shell, &ctx);
        assert_eq!(viewports[&id_of(&restored)].builder.active, Some(false));
        assert_ne!(viewports[&id_of(&named)].builder.active, Some(false));
        // Once the restored window is up, the named one comes back on top;
        // on later frames it is left alone.
        let first = first_frame(&shell, &ctx, id_of(&restored));
        assert!(commands_to(&first, id_of(&named)).contains(&ViewportCommand::Focus));
        let second = first_frame(&shell, &ctx, id_of(&restored));
        assert!(!commands_to(&second, id_of(&named)).contains(&ViewportCommand::Focus));
    }

    #[test]
    fn focus_is_not_handed_to_a_window_closed_meanwhile() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, _) = held_shell(&ctx);
        let (named, restored) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        shell.open_now(&ctx, &named, None).unwrap();
        shell.open_all_quietly(&ctx, std::slice::from_ref(&restored));
        finish(&jobs, 0);
        shell.receive(&ctx);
        shell.handle(&ctx, Request::Closed(id_of(&named)));
        // A command to a viewport eframe no longer has would bring it back.
        let first = first_frame(&shell, &ctx, id_of(&restored));
        assert!(!first.contains_key(&id_of(&named)));
    }

    #[test]
    fn a_restored_document_before_the_requested_one_stays_behind_it() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, _) = held_shell(&ctx);
        let (restored, requested) = (fixture("two-pages.pdf"), fixture("text.pdf"));
        shell.open_all_quietly(&ctx, std::slice::from_ref(&restored));
        // A Finder open, loading alongside.
        shell.open(&ctx, &requested, None, None);
        finish(&jobs, 0);
        shell.receive(&ctx);
        let viewports = shown(&shell, &ctx);
        assert_eq!(viewports[&id_of(&restored)].builder.active, Some(false));
        // The requested one comes later, in front by itself.
        finish(&jobs, 0);
        shell.receive(&ctx);
        let viewports = shown(&shell, &ctx);
        assert_ne!(viewports[&id_of(&requested)].builder.active, Some(false));
        let first = first_frame(&shell, &ctx, id_of(&restored));
        assert!(!commands_to(&first, id_of(&requested)).contains(&ViewportCommand::Focus));
    }

    #[test]
    fn restored_documents_alone_open_as_usual() {
        let ctx = egui::Context::default();
        let (mut shell, jobs, _) = held_shell(&ctx);
        let path = fixture("text.pdf");
        shell.open_all_quietly(&ctx, std::slice::from_ref(&path));
        finish(&jobs, 0);
        shell.receive(&ctx);
        assert_ne!(
            shown(&shell, &ctx)[&id_of(&path)].builder.active,
            Some(false)
        );
    }

    #[test]
    fn a_finder_open_still_loading_means_no_open_panel() {
        let ctx = egui::Context::default();
        let probe = IncomingProbe::default();
        let (shell, _jobs, _) = held_shell(&ctx);
        let mut shell = shell
            .with_open_panel_at_start()
            .with_incoming(probe.incoming());
        let now = Instant::now();
        shell.settle_start(now);
        probe.queue.lock().unwrap().push(fixture("text.pdf").into());
        shell.receive(&ctx);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err());
        assert!(shell.open_paths().is_empty(), "still loading");
    }

    #[test]
    fn restored_documents_still_loading_mean_no_open_panel() {
        let ctx = egui::Context::default();
        let (shell, jobs, _) = held_shell(&ctx);
        let mut shell = shell.with_open_panel_at_start();
        shell.open_all_quietly(&ctx, &[fixture("text.pdf"), fixture("gone.pdf")]);
        let now = Instant::now();
        shell.settle_start(now);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err());
        // One fails; the other still counts.
        finish(&jobs, 1);
        shell.receive(&ctx);
        shell.settle_start(now + START_QUIET * 2);
        assert!(shell.received.try_recv().is_err());
    }

    #[test]
    fn restored_documents_that_all_fail_bring_back_the_open_panel() {
        let ctx = egui::Context::default();
        let (shell, jobs, _) = held_shell(&ctx);
        let mut shell = shell.with_open_panel_at_start();
        shell.open_all_quietly(&ctx, &[fixture("gone.pdf"), fixture("lost.pdf")]);
        let now = Instant::now();
        shell.settle_start(now);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err(), "while loading");
        finish(&jobs, 0);
        shell.receive(&ctx);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err(), "one still loading");
        finish(&jobs, 0);
        shell.receive(&ctx);
        shell.settle_start(now + START_QUIET);
        assert!(matches!(shell.received.try_recv(), Ok(Request::OpenPanel)));
    }

    #[test]
    fn quitting_while_a_document_loads_keeps_it_in_the_open_list() {
        let ctx = egui::Context::default();
        let (mut shell, _jobs, _) = held_shell(&ctx);
        let path = fixture("text.pdf");
        shell.open(&ctx, &path, None, None);
        shell.handle(&ctx, Request::Quit);
        assert_eq!(
            shell.open_at_quit,
            Some(vec![Windows::<Window>::canonical(&path)])
        );
    }

    #[test]
    fn closing_the_last_window_keeps_the_shell() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        let id = Windows::<Window>::id_for(&Windows::<Window>::canonical(&path));
        shell.handle(&ctx, Request::Closed(id));
        assert!(shell.open_paths().is_empty());
        assert!(!root_commands(&ctx).contains(&ViewportCommand::Close));
        // A second close of the same window is nothing.
        shell.handle(&ctx, Request::Closed(id));
        handle_now(&mut shell, &ctx, open(fixture("two-pages.pdf"), None));
        assert_eq!(shell.open_paths().len(), 1);
    }

    #[test]
    fn closing_a_window_saves_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, Some(dir.path().to_path_buf()));
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        let id = Windows::<Window>::id_for(&Windows::<Window>::canonical(&path));
        shell.handle(&ctx, Request::Closed(id));
        assert!(
            Store::at(dir.path())
                .load(DocKey::new(b"x", &path))
                .1
                .is_some()
        );
    }

    #[test]
    fn quit_after_the_last_window_records_nothing() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        let id = Windows::<Window>::id_for(&Windows::<Window>::canonical(&path));
        shell.handle(&ctx, Request::Closed(id));
        shell.handle(&ctx, Request::Quit);
        assert_eq!(shell.open_at_quit, Some(vec![]));
    }

    #[test]
    fn quit_saves_every_window_and_closes_the_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, Some(dir.path().to_path_buf()));
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        handle_now(&mut shell, &ctx, open(a.clone(), None));
        handle_now(&mut shell, &ctx, open(b.clone(), None));
        shell.handle(&ctx, Request::Quit);
        let store = Store::at(dir.path());
        for path in [&a, &b] {
            assert!(store.load(DocKey::new(b"x", path)).1.is_some(), "{path:?}");
        }
        assert_eq!(shell.open_at_quit, Some(shell.open_paths()));
        assert_eq!(shell.open_paths().len(), 2);
        assert!(root_commands(&ctx).contains(&ViewportCommand::Close));
    }

    #[test]
    fn saved_settings_reach_every_window() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        handle_now(&mut shell, &ctx, open(fixture("two-pages.pdf"), None));
        let mut settings = Settings::default();
        settings.new_documents.gapless = !settings.new_documents.gapless;
        shell.handle(&ctx, Request::SettingsSaved(settings));
        assert_eq!(shell.windows.iter().count(), 2);
        for (_, window) in shell.windows.iter() {
            assert_eq!(settings_of(&lock(&window.app)), settings);
        }
    }

    #[test]
    fn saved_settings_repaint_every_window() {
        let ctx = egui::Context::default();
        let _ = ctx.run_logic(&RawInput::default(), |_| {});
        let mut shell = shell_with(&ctx, None);
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        handle_now(&mut shell, &ctx, open(fixture("two-pages.pdf"), None));
        // Let the repaints the opening asked for pass.
        let _ = ctx.run_logic(&RawInput::default(), |_| {});
        let ids: Vec<_> = shell.windows.iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 2);
        for id in &ids {
            assert!(!ctx.has_requested_repaint_for(id), "{id:?} before");
        }
        shell.handle(&ctx, Request::SettingsSaved(Settings::default()));
        for id in &ids {
            assert!(ctx.has_requested_repaint_for(id), "{id:?} after");
        }
    }

    #[test]
    fn a_window_asks_through_the_shell_and_wakes_the_root() {
        let ctx = egui::Context::default();
        let _ = ctx.run_logic(&RawInput::default(), |_| {});
        let mut shell = shell_with(&ctx, None);
        shell.requests().send(open(fixture("text.pdf"), None));
        assert!(ctx.has_requested_repaint_for(&ViewportId::ROOT));
        shell.receive(&ctx);
        assert_eq!(shell.open_paths().len(), 1);
    }

    #[test]
    fn a_hidden_root_paints_when_a_window_opens() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        assert!(root_commands(&ctx).contains(&ViewportCommand::RequestPaintWhileHidden));
    }

    #[test]
    fn a_hidden_root_paints_when_a_window_closes() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        let _ = root_commands(&ctx);
        let id = Windows::<Window>::id_for(&Windows::<Window>::canonical(&path));
        shell.handle(&ctx, Request::Closed(id));
        assert!(root_commands(&ctx).contains(&ViewportCommand::RequestPaintWhileHidden));
    }

    #[test]
    fn a_hidden_root_paints_to_quit() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        shell.handle(&ctx, Request::Quit);
        let commands = root_commands(&ctx);
        assert!(commands.contains(&ViewportCommand::RequestPaintWhileHidden));
        assert!(commands.contains(&ViewportCommand::Close));
    }

    /// What the incoming fixture shares with the shell.
    #[derive(Default)]
    struct IncomingProbe {
        queue: Arc<Mutex<Vec<std::ffi::OsString>>>,
    }

    impl IncomingProbe {
        fn incoming(&self) -> Incoming {
            let queue = self.queue.clone();
            Incoming {
                take: Box::new(move || std::mem::take(&mut *queue.lock().unwrap())),
            }
        }
    }

    #[test]
    fn incoming_documents_open_in_order() {
        let ctx = egui::Context::default();
        let probe = IncomingProbe::default();
        let mut shell = shell_with(&ctx, None).with_incoming(probe.incoming());
        shell.receive(&ctx);
        assert!(shell.open_paths().is_empty());
        let (a, b) = (fixture("two-pages.pdf"), fixture("text.pdf"));
        let url = format!("file://{}", a.display());
        probe
            .queue
            .lock()
            .unwrap()
            .extend([url.into(), b.clone().into()]);
        shell.receive(&ctx);
        shell.receive(&ctx);
        assert_eq!(
            shell.open_paths(),
            vec![
                Windows::<Window>::canonical(&a),
                Windows::<Window>::canonical(&b)
            ]
        );
    }

    #[test]
    fn no_document_by_the_deadline_shows_the_open_panel_once() {
        let now = Instant::now();
        let mut panel = StartPanel::wanted();
        assert!(!panel.due(now), "not before launching has finished");
        assert!(panel.launched(now));
        assert!(!panel.launched(now), "only the first launch counts");
        assert!(!panel.due(now + START_QUIET / 2));
        assert!(panel.due(now + START_QUIET));
        assert!(!panel.due(now + START_QUIET * 2));
    }

    #[test]
    fn a_document_before_the_deadline_shows_no_panel() {
        let now = Instant::now();
        let mut panel = StartPanel::wanted();
        panel.launched(now);
        panel.opened();
        assert!(!panel.due(now + START_QUIET));
        // Opened during launching: nothing to wake for.
        let mut early = StartPanel::wanted();
        early.opened();
        assert!(!early.launched(now));
        assert!(!early.due(now + START_QUIET));
    }

    #[test]
    fn a_panel_wanted_again_after_restoring_comes_when_launching_settles() {
        let now = Instant::now();
        let mut panel = StartPanel::wanted();
        // Restored documents started loading before launching finished.
        panel.opened();
        assert!(!panel.launched(now));
        // None opened, before launching has settled: wake then.
        assert_eq!(panel.restore(true, now), Some(START_QUIET));
        assert!(!panel.due(now));
        assert!(panel.due(now + START_QUIET));
        // After it has settled: due at once, nothing to wake for.
        let mut late = StartPanel::wanted();
        late.opened();
        late.launched(now);
        assert_eq!(late.restore(true, now + START_QUIET * 2), None);
        assert!(late.due(now + START_QUIET * 2));
    }

    #[test]
    fn a_file_on_the_command_line_shows_no_panel() {
        let now = Instant::now();
        let mut panel = StartPanel::default();
        assert!(!panel.launched(now));
        assert!(!panel.due(now + START_QUIET));
    }

    #[test]
    fn opened_with_nothing_the_shell_asks_for_the_open_panel_once_settled() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None).with_open_panel_at_start();
        let now = Instant::now();
        shell.settle_start(now);
        assert!(shell.received.try_recv().is_err());
        shell.settle_start(now + START_QUIET);
        assert!(matches!(shell.received.try_recv(), Ok(Request::OpenPanel)));
        shell.settle_start(now + START_QUIET * 2);
        assert!(shell.received.try_recv().is_err());
    }

    #[test]
    fn a_finder_open_while_launching_means_no_open_panel() {
        let ctx = egui::Context::default();
        let probe = IncomingProbe::default();
        let mut shell = shell_with(&ctx, None)
            .with_open_panel_at_start()
            .with_incoming(probe.incoming());
        let now = Instant::now();
        shell.settle_start(now);
        probe.queue.lock().unwrap().push(fixture("text.pdf").into());
        shell.receive(&ctx);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err());
        assert_eq!(shell.open_paths().len(), 1);
    }

    /// Runs a root pass in which `focused` is the focused window.
    fn focus(ctx: &egui::Context, focused: Option<ViewportId>) {
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(ViewportId::ROOT, egui::ViewportInfo::default());
        if let Some(id) = focused {
            viewports.insert(
                id,
                egui::ViewportInfo {
                    focused: Some(true),
                    ..Default::default()
                },
            );
        }
        let output = ctx.run_ui(
            RawInput {
                viewports,
                ..Default::default()
            },
            |_| {},
        );
        output.drop_without_applying_deltas();
    }

    fn id_of(path: &Path) -> ViewportId {
        Windows::<Window>::id_for(&Windows::<Window>::canonical(path))
    }

    /// The actions each window was asked to carry out, in open order.
    fn queued(shell: &Shell) -> Vec<Vec<noido_core::Action>> {
        shell
            .windows
            .iter()
            .map(|(_, window)| lock(&window.app).queued().to_vec())
            .collect()
    }

    #[test]
    fn open_from_the_menu_asks_for_the_open_panel() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        shell.menu(&ctx, MenuCommand::Open);
        assert!(matches!(shell.received.try_recv(), Ok(Request::OpenPanel)));
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        shell.menu(&ctx, MenuCommand::Open);
        assert!(matches!(shell.received.try_recv(), Ok(Request::OpenPanel)));
    }

    #[test]
    fn a_reopen_shows_the_open_panel_only_with_no_window() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        shell.menu(&ctx, MenuCommand::Reopen);
        assert!(matches!(shell.received.try_recv(), Ok(Request::OpenPanel)));
        // A minimized window counts: AppKit brings it back itself.
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        shell.menu(&ctx, MenuCommand::Reopen);
        assert!(shell.received.try_recv().is_err());
    }

    #[test]
    fn settings_and_about_go_to_the_focused_window() {
        use noido_core::Action;
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        handle_now(&mut shell, &ctx, open(a, None));
        handle_now(&mut shell, &ctx, open(b.clone(), None));
        focus(&ctx, Some(id_of(&b)));
        shell.menu(&ctx, MenuCommand::Settings);
        shell.menu(&ctx, MenuCommand::About);
        assert_eq!(
            queued(&shell),
            vec![vec![], vec![Action::OpenSettings, Action::ShowDocumentInfo]]
        );
        assert!(ctx.has_requested_repaint_for(&id_of(&b)));
    }

    #[test]
    fn settings_and_about_with_no_focused_window_do_nothing() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        shell.menu(&ctx, MenuCommand::Settings);
        shell.menu(&ctx, MenuCommand::About);
        handle_now(&mut shell, &ctx, open(fixture("text.pdf"), None));
        focus(&ctx, None);
        shell.menu(&ctx, MenuCommand::Settings);
        shell.menu(&ctx, MenuCommand::About);
        assert_eq!(queued(&shell), vec![Vec::<noido_core::Action>::new()]);
        assert!(shell.received.try_recv().is_err());
    }

    #[test]
    fn menu_commands_are_taken_in_with_the_rest() {
        let ctx = egui::Context::default();
        let queue: Arc<Mutex<Vec<MenuCommand>>> = Arc::default();
        let mut shell = shell_with(&ctx, None).with_menu({
            let queue = queue.clone();
            Box::new(move || std::mem::take(&mut *queue.lock().unwrap()))
        });
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        focus(&ctx, Some(id_of(&path)));
        queue.lock().unwrap().push(MenuCommand::Settings);
        shell.receive(&ctx);
        assert_eq!(queued(&shell), vec![vec![noido_core::Action::OpenSettings]]);
        assert!(queue.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_follows_the_setting() {
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        assert_eq!(
            to_open(vec![a.clone(), b.clone()], true, None),
            vec![a.clone(), b]
        );
        assert_eq!(to_open(vec![a.clone()], false, None), Vec::<PathBuf>::new());
        assert_eq!(to_open(vec![a.clone()], false, Some(&a)), vec![a]);
    }

    #[test]
    fn the_named_file_is_added_once() {
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        // The same file by another spelling counts as the same.
        let again = a.parent().unwrap().join("../fixtures").join("text.pdf");
        assert_eq!(
            to_open(vec![a.clone()], true, Some(&again)),
            vec![a.clone()]
        );
        assert_eq!(to_open(vec![a.clone()], true, Some(&b)), vec![a, b]);
    }

    #[test]
    fn a_restored_file_that_is_gone_is_skipped() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        shell.open_all_quietly(&ctx, &[a.clone(), fixture("gone.pdf"), b.clone()]);
        shell.receive(&ctx);
        assert_eq!(
            shell.open_paths(),
            vec![
                Windows::<Window>::canonical(&a),
                Windows::<Window>::canonical(&b)
            ]
        );
    }

    #[test]
    fn restored_documents_mean_no_open_panel() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None).with_open_panel_at_start();
        shell.open_all_quietly(&ctx, &[fixture("text.pdf")]);
        shell.receive(&ctx);
        let now = Instant::now();
        shell.settle_start(now);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err());
    }

    #[test]
    fn nothing_restored_keeps_the_open_panel() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None).with_open_panel_at_start();
        shell.open_all_quietly(&ctx, &[fixture("gone.pdf")]);
        shell.receive(&ctx);
        let now = Instant::now();
        shell.settle_start(now);
        shell.settle_start(now + START_QUIET);
        assert!(matches!(shell.received.try_recv(), Ok(Request::OpenPanel)));
    }

    #[test]
    fn on_exit_records_the_open_documents_in_the_store() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None).with_open_list(Store::at(dir.path()));
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        handle_now(&mut shell, &ctx, open(a.clone(), None));
        handle_now(&mut shell, &ctx, open(b.clone(), None));
        eframe::App::on_exit(&mut shell);
        assert_eq!(
            Store::at(dir.path()).load_open(),
            vec![
                Windows::<Window>::canonical(&a),
                Windows::<Window>::canonical(&b)
            ]
        );
        let id = id_of(&a);
        shell.handle(&ctx, Request::Closed(id));
        shell.handle(&ctx, Request::Closed(id_of(&b)));
        eframe::App::on_exit(&mut shell);
        assert!(Store::at(dir.path()).load_open().is_empty());
    }

    #[test]
    fn a_shell_started_with_a_file_asks_for_no_open_panel() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let now = Instant::now();
        shell.settle_start(now);
        shell.settle_start(now + START_QUIET);
        assert!(shell.received.try_recv().is_err());
    }

    /// Sets the frame the window `id` was last seen with.
    fn seen_at(shell: &Shell, id: ViewportId, frame: Frame) {
        let (_, window) = shell.windows.iter().find(|(i, _)| *i == id).unwrap();
        *lock(&window.frame) = Some(frame);
    }

    fn some_frame() -> Frame {
        Frame {
            x: 40.0,
            y: 60.0,
            width: 700.0,
            height: 800.0,
        }
    }

    #[test]
    fn closing_a_window_keeps_its_frame() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, Some(dir.path().to_path_buf()));
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        let id = id_of(&path);
        seen_at(&shell, id, some_frame());
        shell.handle(&ctx, Request::Closed(id));
        let store = Store::at(dir.path());
        let slot = store.load(DocKey::new(b"x", &path)).0;
        assert_eq!(store.load_frame(&slot), Some(some_frame()));
    }

    #[test]
    fn quitting_keeps_the_frames() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, Some(dir.path().to_path_buf()));
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        seen_at(&shell, id_of(&path), some_frame());
        shell.handle(&ctx, Request::Quit);
        let store = Store::at(dir.path());
        let slot = store.load(DocKey::new(b"x", &path)).0;
        assert_eq!(store.load_frame(&slot), Some(some_frame()));
    }

    #[test]
    fn a_window_never_seen_leaves_no_frame() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, Some(dir.path().to_path_buf()));
        let path = fixture("text.pdf");
        handle_now(&mut shell, &ctx, open(path.clone(), None));
        shell.handle(&ctx, Request::Closed(id_of(&path)));
        let store = Store::at(dir.path());
        let slot = store.load(DocKey::new(b"x", &path)).0;
        assert_eq!(store.load_frame(&slot), None);
    }

    #[test]
    fn a_window_opens_where_it_was() {
        let builder = window_builder(ViewportBuilder::default(), Some(some_frame()));
        assert_eq!(builder.position, Some(egui::pos2(40.0, 60.0)));
        assert_eq!(builder.inner_size, Some(egui::vec2(700.0, 800.0)));
    }

    #[test]
    fn a_window_with_only_a_size_is_placed_by_the_system() {
        let builder = window_builder(ViewportBuilder::default(), Some(Frame::sized(700.0, 800.0)));
        assert_eq!(builder.position, None);
        assert_eq!(builder.inner_size, Some(egui::vec2(700.0, 800.0)));
        let builder = window_builder(ViewportBuilder::default(), None);
        assert_eq!(builder.position, None);
        assert_eq!(builder.inner_size, Some(egui::vec2(900.0, 1100.0)));
    }

    #[test]
    fn a_window_is_never_opened_smaller_than_the_minimum() {
        let frame = Frame {
            width: 10.0,
            height: 10.0,
            ..some_frame()
        };
        let builder = window_builder(ViewportBuilder::default(), Some(frame));
        let min = egui::Vec2::from(crate::MIN_WINDOW_SIZE);
        assert_eq!(builder.inner_size, Some(min));
    }

    #[test]
    fn the_frame_is_the_outer_position_and_the_inner_size() {
        let mut info = egui::ViewportInfo::default();
        assert_eq!(frame_of(&info), None);
        info.outer_rect = Some(egui::Rect::from_min_size(
            egui::pos2(40.0, 60.0),
            egui::vec2(700.0, 828.0),
        ));
        info.inner_rect = Some(egui::Rect::from_min_size(
            egui::pos2(40.0, 88.0),
            egui::vec2(700.0, 800.0),
        ));
        assert_eq!(frame_of(&info), Some(some_frame()));
        info.minimized = Some(true);
        assert_eq!(frame_of(&info), None);
        info.minimized = Some(false);
        info.fullscreen = Some(true);
        assert_eq!(frame_of(&info), None);
    }
}
