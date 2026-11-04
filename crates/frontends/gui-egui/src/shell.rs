//! What a document window asks of the shell around it, and the shell: on
//! macOS, the one process that holds every document window, each a
//! deferred viewport of a hidden root.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

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
    // Sent by the native menu and the Dock (Task 9).
    #[allow(dead_code)]
    OpenPanel,
    /// Close every window and quit.
    Quit,
    /// The settings were saved in a window; the others take them.
    SettingsSaved(Settings),
    /// The window was asked to close (its close button, Mod+W).
    Closed(ViewportId),
}

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
    /// Launches handing their file over.
    app_requests: Option<AppRequests>,
    theme: Option<crate::theme::Live>,
    /// Root frames run so far, up to [`HIDE_FRAMES`].
    frames: u32,
    /// The documents open at the last quit; Task 10 stores them.
    open_at_quit: Option<Vec<PathBuf>>,
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
            app_requests: None,
            theme: None,
            frames: 0,
            open_at_quit: None,
        }
    }

    /// Opens what Finder sends.
    pub fn with_incoming(mut self, incoming: Incoming) -> Self {
        self.incoming = Some(incoming);
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

    /// Opens `path` in a new window, or brings forward the window that has
    /// it; what to answer a launch that asked.
    pub fn open(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        target: Option<Target>,
    ) -> Result<Answer, String> {
        let canonical = Windows::<Window>::canonical(path);
        if let Some(id) = self.windows.find(&canonical) {
            if let Some((_, window)) = self.windows.iter().find(|(i, _)| *i == id) {
                window.activation.request(target);
            }
            // An occluded window gets no frames until it is shown.
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
            if let Some(start) = incoming.start.take() {
                start(ctx);
            }
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
        let (_, window) = shell.windows.iter().next().unwrap();
        let request = window.activation.take().unwrap();
        assert_eq!(request.target, Some(Target::Page(1)));
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
        started: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl IncomingProbe {
        fn incoming(&self) -> Incoming {
            let (queue, started) = (self.queue.clone(), self.started.clone());
            Incoming {
                start: Some(Box::new(move |_| {
                    started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                })),
                take: Box::new(move || std::mem::take(&mut *queue.lock().unwrap())),
            }
        }
    }

    #[test]
    fn incoming_documents_open_in_order_and_start_once() {
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
        assert_eq!(probe.started.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            shell.open_paths(),
            vec![
                Windows::<Window>::canonical(&a),
                Windows::<Window>::canonical(&b)
            ]
        );
    }
}
