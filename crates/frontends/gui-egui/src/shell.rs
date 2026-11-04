//! What a document window asks of the shell around it, and the shell: on
//! macOS, the one process that holds every document window, each a
//! deferred viewport of a hidden root.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportBuilder, ViewportCommand, ViewportId};
use noido_core::Target;
use noido_settings::Settings;

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
pub type OpenDoc = Box<dyn FnMut(&Path, Option<Target>, ViewportId) -> Result<NoidoApp, String>>;

/// How many of its first frames the shell hides the root in: eframe shows
/// the root after its first frame, and hiding it every frame would keep
/// waking it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const HIDE_FRAMES: u32 = 3;

/// How long after launching the shell still waits for documents before
/// it shows the Open panel by itself: Finder's opens can come just after
/// launching has finished.
const START_QUIET: Duration = Duration::from_millis(300);

/// Whether an app opened by itself, with no document, shows the Open
/// panel: once launching has settled, if nothing was asked to open by
/// then (Finder's opens, a launch handing over, documents restored).
#[derive(Debug, Default)]
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
        if !self.wanted || self.settled_at.is_some() {
            return false;
        }
        self.settled_at = Some(now + START_QUIET);
        true
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
}

/// The app on macOS: every document window, and what feeds them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Shell {
    open_doc: OpenDoc,
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
    /// The documents open at the last quit; Task 10 stores them.
    open_at_quit: Option<Vec<PathBuf>>,
    /// Whether to show the Open panel once launching has settled.
    start_panel: StartPanel,
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
            requests: Requests::new(tx, root_waker(ctx)),
            received,
            windows: Windows::new(),
            incoming: None,
            menu: None,
            app_requests: None,
            theme: None,
            frames: 0,
            open_at_quit: None,
            start_panel: StartPanel::default(),
        }
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
            } => match (self.open(ctx, &path, target), reply) {
                (Ok(answer), Some(reply)) => reply.send(answer),
                (Err(e), Some(reply)) => reply.send(Answer::Failed(e)),
                (Ok(_), None) => {}
                (Err(e), None) => eprintln!("noido: {e}"),
            },
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
    /// it; what to answer a launch that asked.
    pub fn open(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        target: Option<Target>,
    ) -> Result<Answer, String> {
        self.start_panel.opened();
        let canonical = Windows::<Window>::canonical(path);
        if let Some(id) = self.windows.find(&canonical) {
            if let Some((_, window)) = self.windows.iter().find(|(i, _)| *i == id) {
                window.activation.request(target);
            }
            // An occluded or minimized window gets no frames, so it cannot
            // come forward by itself: the shell brings it.
            ctx.send_viewport_cmd_to(id, ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
            return Ok(Answer::Forward { pid: None });
        }
        let id = Windows::<Window>::id_for(&canonical);
        let app = (self.open_doc)(path, target, id)?;
        let activation = Activation::default();
        let title = format!("{} — noido", app.name());
        let app = app
            .with_requests(self.requests.clone())
            .with_activation(activation.clone());
        self.windows.insert(
            canonical,
            Window {
                app: Arc::new(Mutex::new(app)),
                title,
                activation,
            },
        );
        paint_root(ctx);
        Ok(Answer::Opened { pid: None })
    }

    /// Saves every window and notes which are open, for a quit.
    fn shut_down(&mut self) {
        for (_, window) in self.windows.iter() {
            lock(&window.app).save_now();
        }
        self.record_open(self.open_paths());
    }

    /// Keeps the documents open at a quit. Task 10 stores them, to reopen
    /// on the next start.
    fn record_open(&mut self, paths: Vec<PathBuf>) {
        self.open_at_quit = Some(paths);
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
        arrived.extend(self.received.try_iter());
        for request in arrived {
            self.handle(ctx, request);
        }
    }

    /// Asks for the Open panel once launching has settled at `now`, if
    /// the app was opened with no document and none has come since; the
    /// first call is when launching finished.
    fn settle_start(&mut self, now: Instant) {
        if self.start_panel.launched(now) {
            // The hidden root runs only when woken.
            let wake = self.requests.wake.clone();
            std::thread::spawn(move || {
                std::thread::sleep(START_QUIET);
                wake();
            });
        }
        if self.start_panel.due(now) {
            self.requests.send(Request::OpenPanel);
        }
    }

    /// Shows every window, as a deferred viewport of the root.
    fn show_windows(&self, ctx: &egui::Context) {
        for (id, window) in self.windows.iter() {
            let app = window.app.clone();
            let requests = self.requests.clone();
            let builder = crate::own_title_bar(
                ViewportBuilder::default()
                    .with_title(window.title.clone())
                    // Used until the window's own place is kept.
                    .with_inner_size([900.0, 1100.0])
                    .with_min_inner_size(crate::MIN_WINDOW_SIZE),
            );
            ctx.show_viewport_deferred(id, builder, move |ui, _class| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    requests.send(Request::Closed(id));
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

fn lock(app: &Mutex<NoidoApp>) -> std::sync::MutexGuard<'_, NoidoApp> {
    app.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use eframe::egui::RawInput;
    use noido_state::{DocKey, Store};

    use super::*;
    use crate::app::tests::{blank_app, settings_of};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures")
            .join(name)
    }

    /// A shell whose documents are blank apps, each saving its state in
    /// `store` if given; a file that doesn't exist fails to open.
    fn shell_with(ctx: &egui::Context, store: Option<std::path::PathBuf>) -> Shell {
        let open_doc: OpenDoc = Box::new(move |path, _target, _id| {
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
        });
        Shell::new(ctx, open_doc, mpsc::channel())
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
        assert!(matches!(
            shell.open(&ctx, &path, None),
            Ok(Answer::Opened { .. })
        ));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
        // Another spelling of the same file.
        let again = fixture("../fixtures/text.pdf");
        assert!(matches!(
            shell.open(&ctx, &again, Some(Target::Page(1))),
            Ok(Answer::Forward { .. })
        ));
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
        assert!(matches!(shell.open(&ctx, &missing, None), Err(e) if e.contains("missing.pdf")));
        shell.handle(&ctx, open(missing, None));
        assert!(shell.open_paths().is_empty());
        shell.handle(&ctx, open(fixture("text.pdf"), None));
        assert_eq!(shell.open_paths().len(), 1);
    }

    #[test]
    fn closing_the_last_window_keeps_the_shell() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let path = fixture("text.pdf");
        shell.handle(&ctx, open(path.clone(), None));
        let id = Windows::<Window>::id_for(&Windows::<Window>::canonical(&path));
        shell.handle(&ctx, Request::Closed(id));
        assert!(shell.open_paths().is_empty());
        assert!(!root_commands(&ctx).contains(&ViewportCommand::Close));
        // A second close of the same window is nothing.
        shell.handle(&ctx, Request::Closed(id));
        shell.handle(&ctx, open(fixture("two-pages.pdf"), None));
        assert_eq!(shell.open_paths().len(), 1);
    }

    #[test]
    fn closing_a_window_saves_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, Some(dir.path().to_path_buf()));
        let path = fixture("text.pdf");
        shell.handle(&ctx, open(path.clone(), None));
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
        shell.handle(&ctx, open(path.clone(), None));
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
        shell.handle(&ctx, open(a.clone(), None));
        shell.handle(&ctx, open(b.clone(), None));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
        shell.handle(&ctx, open(fixture("two-pages.pdf"), None));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
        shell.handle(&ctx, open(fixture("two-pages.pdf"), None));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
        assert!(root_commands(&ctx).contains(&ViewportCommand::RequestPaintWhileHidden));
    }

    #[test]
    fn a_hidden_root_paints_when_a_window_closes() {
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let path = fixture("text.pdf");
        shell.handle(&ctx, open(path.clone(), None));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
        shell.menu(&ctx, MenuCommand::Reopen);
        assert!(shell.received.try_recv().is_err());
    }

    #[test]
    fn settings_and_about_go_to_the_focused_window() {
        use noido_core::Action;
        let ctx = egui::Context::default();
        let mut shell = shell_with(&ctx, None);
        let (a, b) = (fixture("text.pdf"), fixture("two-pages.pdf"));
        shell.handle(&ctx, open(a, None));
        shell.handle(&ctx, open(b.clone(), None));
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
        shell.handle(&ctx, open(fixture("text.pdf"), None));
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
        shell.handle(&ctx, open(path.clone(), None));
        focus(&ctx, Some(id_of(&path)));
        queue.lock().unwrap().push(MenuCommand::Settings);
        shell.receive(&ctx);
        assert_eq!(queued(&shell), vec![vec![noido_core::Action::OpenSettings]]);
        assert!(queue.lock().unwrap().is_empty());
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
}
