//! `noido <file.pdf>`: open a PDF in the egui frontend; also
//! `noido 'noido:///path/file.pdf#page=12'` and `noido file.pdf --page 12`.

mod app;
mod app_bundle;
mod desktop;
mod document;
mod frame;
mod icon;
mod image;
mod info;
mod instance;
mod interrupt;
mod links;
mod load;
mod open;
mod search;
mod select;
mod settings;
mod shell;
mod sidebar;
mod texts;
mod theme;
mod titlebar;
mod watch;
mod windows;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
#[cfg(target_os = "macos")]
use std::time::Duration;

use clap::Parser;
use eframe::egui;
use noido_core::{Opening, Target, parse_argument};

/// The smallest window in which the title bar's controls all fit.
const MIN_WINDOW_SIZE: [f32; 2] = [480.0, 300.0];

/// UI points scrolled per mouse wheel step (egui's default is 40).
const WHEEL_SCROLL_SPEED: f32 = 125.0;

/// How long a terminal launch waits for the app it started to listen.
#[cfg(target_os = "macos")]
const START_WAIT: Duration = Duration::from_secs(5);

/// How often it checks.
#[cfg(target_os = "macos")]
const START_POLL: Duration = Duration::from_millis(50);

#[derive(Parser)]
#[command(
    name = "noido",
    version = env!("NOIDO_VERSION"),
    about = "PDF reader and note taker"
)]
struct Args {
    /// PDF file, or a file: or noido: link, to open
    #[cfg_attr(
        target_os = "macos",
        arg(required_unless_present_any = ["install", "uninstall", "app_bundle", "foreground"])
    )]
    #[cfg_attr(
        not(target_os = "macos"),
        arg(required_unless_present_any = ["install", "uninstall", "app_bundle"])
    )]
    path: Option<OsString>,
    /// Page to open at, from 1
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..),
          conflicts_with_all = ["install", "uninstall", "app_bundle"])]
    page: Option<u32>,
    /// Add noido to the desktop's apps for PDFs
    #[arg(long, conflicts_with_all = ["path", "uninstall", "app_bundle"])]
    install: bool,
    /// Remove what --install added
    #[arg(long, conflicts_with_all = ["path", "app_bundle"])]
    uninstall: bool,
    /// Build an ad-hoc signed noido.app in DIR (macOS)
    #[arg(long, value_name = "DIR", conflicts_with = "path")]
    app_bundle: Option<PathBuf>,
    /// Run in this terminal, which Ctrl-C quits, instead of in the
    /// background (macOS; elsewhere noido always does)
    #[arg(long, conflicts_with_all = ["install", "uninstall", "app_bundle"])]
    foreground: bool,
}

/// Drops the `-psn_…` argument older Finders add to a launch.
fn without_psn(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    args.into_iter()
        .filter(|a| !a.as_encoded_bytes().starts_with(b"-psn_"))
        .collect()
}

/// A launch from Finder: no argument but the program name, from inside a
/// bundle. The documents come as an Apple event, not as arguments.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_finder_launch(args: &[OsString], exe: &Path) -> bool {
    args.len() == 1 && app_bundle::bundle_of(exe).is_some()
}

fn main() -> ExitCode {
    let args = without_psn(std::env::args_os());
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe()
        && is_finder_launch(&args, &exe)
    {
        return exit_code(run_from_finder(&exe));
    }
    let args = Args::parse_from(args);
    let result = match &args.path {
        _ if args.app_bundle.is_some() => build_app_bundle(args.app_bundle.as_deref().unwrap()),
        #[cfg(target_os = "macos")]
        _ if args.install => app_bundle::install(),
        #[cfg(target_os = "macos")]
        _ if args.uninstall => app_bundle::uninstall(),
        #[cfg(not(target_os = "macos"))]
        _ if args.install => desktop::install(),
        #[cfg(not(target_os = "macos"))]
        _ if args.uninstall => desktop::uninstall(),
        #[cfg(target_os = "macos")]
        Some(_) => opening(&args).and_then(|opening| {
            hint_if_stale();
            run(Some(opening), args.foreground)
        }),
        #[cfg(target_os = "macos")]
        None if args.foreground => run(None, true),
        #[cfg(not(target_os = "macos"))]
        Some(_) => opening(&args).and_then(run),
        None => unreachable!("clap requires a file without --install or --uninstall"),
    };
    exit_code(result)
}

/// How noido exits after `result`, which it reports.
fn exit_code(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("noido: {message}");
            ExitCode::FAILURE
        }
    }
}

/// From a terminal, says when the installed noido.app is another version.
#[cfg(target_os = "macos")]
fn hint_if_stale() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if app_bundle::bundle_of(&exe).is_some() {
        return;
    }
    let Some(home) = std::env::home_dir() else {
        return;
    };
    let installed = app_bundle::read_identity(&home.join("Applications/noido.app"));
    if let Some(hint) = app_bundle::stale_hint(installed.as_ref(), env!("NOIDO_VERSION")) {
        eprintln!("noido: {hint}");
    }
}

#[cfg(target_os = "macos")]
fn build_app_bundle(dir: &Path) -> Result<(), String> {
    app_bundle::app_bundle(dir)
}

#[cfg(not(target_os = "macos"))]
fn build_app_bundle(_dir: &Path) -> Result<(), String> {
    Err("--app-bundle is for macOS only".into())
}

/// What the command line asks to open: `--page` wins over a link's target.
fn opening(args: &Args) -> Result<Opening, String> {
    let arg = args.path.as_deref().unwrap_or_default();
    let mut opening = parse_argument(arg)?;
    if let Some(page) = args.page {
        opening.target = Some(Target::Page(page as usize - 1));
    }
    Ok(opening)
}

/// Sets up `ctx` as every noido window wants it: the system theme, keys.
fn configure(ctx: &egui::Context) {
    let theme = theme::Theme::from_system();
    theme.install(ctx);
    // Mod +/-/0 zoom the document, not egui's interface.
    ctx.options_mut(|o| {
        o.zoom_with_keyboard = false;
        // Mod+Q quits through noido's keymap, like every other shortcut.
        o.quit_shortcuts.clear();
        // Mouse wheel steps scroll further than egui's default. egui derives
        // Mod+wheel zoom from the same scaled delta, so slow zoom down by the
        // same factor to keep it unchanged.
        let factor = WHEEL_SCROLL_SPEED / o.input_options.line_scroll_speed;
        o.input_options.line_scroll_speed = WHEEL_SCROLL_SPEED;
        o.input_options.scroll_zoom_speed /= factor;
    });
}

/// On macOS: hands the file to this executable's running app, or starts
/// that app in the background and hands it over; with `foreground`, this
/// launch becomes the app, a hidden root with a window per document.
#[cfg(target_os = "macos")]
fn run(opening: Option<Opening>, foreground: bool) -> Result<(), String> {
    use instance::{AppClaim, OpenRequest};

    let exe = std::env::current_exe().map_err(|e| format!("cannot find noido itself: {e}"))?;
    let request = opening.map(|opening| OpenRequest {
        // The running app has another working directory.
        path: std::path::absolute(&opening.path).unwrap_or(opening.path),
        target: opening.target,
    });
    let listener = match instance::claim_app(&exe, request.as_ref()) {
        AppClaim::Running(answer) => {
            let Some(request) = request else {
                eprintln!("noido: noido is running already");
                return Ok(());
            };
            return handed_to_app(&request, answer);
        }
        AppClaim::First(listener) => Some(listener),
        // No socket to hand over through: this launch is the app.
        AppClaim::Unavailable => None,
    };
    match (listener, request) {
        (Some(listener), Some(request)) if !foreground => {
            // The app started next binds the address itself.
            drop(listener);
            let answer = start_app(&exe, &request)?;
            handed_to_app(&request, answer)
        }
        (listener, request) => run_app(listener, request, false),
    }
}

/// Started by Finder: noido.app opened by itself, or for documents, which
/// come as Apple events once the app runs. This launch becomes the app,
/// unless this executable's app runs already (Launch Services normally
/// hands everything to that one instead of starting another).
#[cfg(target_os = "macos")]
fn run_from_finder(exe: &Path) -> Result<(), String> {
    use instance::AppClaim;

    let listener = match instance::claim_app(exe, None) {
        AppClaim::Running(_) => return Ok(()),
        AppClaim::First(listener) => Some(listener),
        AppClaim::Unavailable => None,
    };
    run_app(listener, None, true)
}

/// Starts this executable's app detached from the terminal and hands it
/// `request` once it listens; `None` if it never answered.
#[cfg(target_os = "macos")]
fn start_app(
    exe: &Path,
    request: &instance::OpenRequest,
) -> Result<Option<instance::Answer>, String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut child = None;
    if let Some(app) = app_bundle::bundle_of(exe) {
        noido_macos::launch_in_bundle(&app, &["--foreground".into()], |_| {});
    } else {
        let started = Command::new(exe)
            .arg("--foreground")
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Out of the terminal's process group: its Ctrl-C stays here.
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot start noido: {e}"))?;
        child = Some(started);
    }
    await_app(
        || instance::send(exe, request),
        || child.as_mut().and_then(|c| c.try_wait().ok().flatten()),
        START_WAIT,
        START_POLL,
    )
}

/// Hands a request to the app a launch started, with `send`, once it
/// listens; `exited` says whether the started process has ended. `None`
/// if nothing answered within `wait`. A process that quit well found
/// another app running, which is asked once more.
#[cfg(target_os = "macos")]
fn await_app(
    mut send: impl FnMut() -> Option<instance::Answer>,
    mut exited: impl FnMut() -> Option<std::process::ExitStatus>,
    wait: Duration,
    poll: Duration,
) -> Result<Option<instance::Answer>, String> {
    let start = std::time::Instant::now();
    loop {
        if let Some(answer) = send() {
            return Ok(Some(answer));
        }
        if let Some(status) = exited() {
            // Quitting well, it found another app, started by a launch at
            // the same moment: that one listens.
            if status.success()
                && let Some(answer) = send()
            {
                return Ok(Some(answer));
            }
            return Err(format!("the noido it started quit at once ({status})"));
        }
        if start.elapsed() >= wait {
            return Ok(None);
        }
        std::thread::sleep(poll);
    }
}

/// Runs the app in this process: a hidden root with a window per
/// document, opening `request` first. With `open_panel`, opened with no
/// document, it shows the Open panel unless documents arrive as it starts.
#[cfg(target_os = "macos")]
fn run_app(
    listener: Option<instance::AppListener>,
    request: Option<instance::OpenRequest>,
    open_panel: bool,
) -> Result<(), String> {
    use std::sync::{Arc, Mutex, OnceLock};

    let root = egui::ViewportId::ROOT;
    let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("cannot open window: {e}"))?;
    // Between building the event loop and running it, or the documents
    // Finder started the app with are lost. They wait in the queue until
    // the shell's first frame; later ones wake it.
    let woken: Arc<OnceLock<egui::Context>> = Arc::default();
    {
        let woken = woken.clone();
        noido_macos::install_open_urls(move || {
            if let Some(ctx) = woken.get() {
                ctx.request_repaint_of(root);
            }
        });
    }
    // Why the shell could not start: eframe keeps its own error.
    let failed: Arc<Mutex<Option<String>>> = Arc::default();
    let creator_failed = failed.clone();
    let create = move |ctx: &egui::Context| -> Result<shell::Shell, String> {
        let _ = woken.set(ctx.clone());
        let shared = document::Shared::load();
        configure(ctx);
        let theme_live = theme::Live::start(ctx);
        let open_doc: shell::OpenDoc = {
            let ctx = ctx.clone();
            Box::new(move |path, target, viewport| {
                document::open(path, target, &ctx, viewport, &shared).map(|opened| opened.app)
            })
        };
        let mut shell = shell::Shell::new(ctx, open_doc, std::sync::mpsc::channel())
            .with_theme(theme_live)
            .with_incoming(app::Incoming {
                take: Box::new(noido_macos::take_opened),
            });
        if open_panel {
            shell = shell.with_open_panel_at_start();
        }
        if let Some(listener) = listener {
            shell = shell.with_app_requests(listener.serve(document::shared_waker(ctx, root)));
        }
        interrupt::quit_on_interrupt(shell.requests());
        if let Some(request) = request {
            shell.open(ctx, &request.path, request.target)?;
        }
        Ok(shell)
    };
    let options = eframe::NativeOptions {
        viewport: root_viewport(),
        persist_window: false,
        ..Default::default()
    };
    let mut app = eframe::create_native(
        "noido",
        options,
        Box::new(move |cc| match create(&cc.egui_ctx) {
            Ok(shell) => Ok(Box::new(shell)),
            Err(message) => {
                *creator_failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.clone());
                Err(message.into())
            }
        }),
        &event_loop,
    );
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("cannot open window: {e}"))?;
    match failed.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

/// What a launch that handed `request` to the running app says, from its
/// answer; the running app is brought to the front.
#[cfg(target_os = "macos")]
fn handed_to_app(
    request: &instance::OpenRequest,
    answer: Option<instance::Answer>,
) -> Result<(), String> {
    use instance::Answer;
    let Some(answer) = answer else {
        return Err("the running noido did not answer".into());
    };
    if let Answer::Opened { pid: Some(pid) } | Answer::Forward { pid: Some(pid) } = answer {
        noido_macos::activate_pid(pid);
    }
    let message = answered(&request.path, request.target.as_ref(), &answer);
    match answer {
        Answer::Opened { .. } | Answer::Forward { .. } => {
            eprintln!("{message}");
            Ok(())
        }
        Answer::Failed(_) => Err(message),
    }
}

/// What a launch on macOS says about the running app's `answer` for
/// `path`; for a failure, the app's own message.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn answered(path: &Path, target: Option<&Target>, answer: &instance::Answer) -> String {
    use instance::Answer;
    let at = at(target);
    let path = path.display();
    match answer {
        Answer::Opened { .. } => format!("noido: opened {path}{at} in the running noido"),
        Answer::Forward { .. } => {
            format!("noido: {path} is already open; brought its window forward{at}")
        }
        Answer::Failed(message) => message.clone(),
    }
}

/// Elsewhere: one process per document, handing over to the one that has
/// the file open.
#[cfg(not(target_os = "macos"))]
fn run(opening: Opening) -> Result<(), String> {
    use instance::Claim;

    let path = opening.path.as_path();
    let target = opening.target;
    // Before reading the file: a launch on an open file only hands over.
    let listener = match instance::claim(path, target.as_ref()) {
        Claim::Running { .. } => {
            eprintln!("{}", handed_over(path, target.as_ref()));
            return Ok(());
        }
        Claim::First(listener) => Some(listener),
        Claim::Unavailable => None,
    };
    let shared = document::Shared::load();

    // Created up front so the render thread can wake the UI when a page is ready.
    let ctx = egui::Context::default();
    configure(&ctx);
    let theme_live = theme::Live::start(&ctx);
    interrupt::close_on_interrupt(ctx.clone());
    let root = egui::ViewportId::ROOT;
    let opened = document::open(path, target, &ctx, root, &shared)?;
    let activation = listener.map(|listener| listener.serve(document::shared_waker(&ctx, root)));
    let name = &opened.name;
    let options = eframe::NativeOptions {
        viewport: own_title_bar(
            egui::ViewportBuilder::default()
                .with_title(format!("{name} — noido"))
                // Ties the window to noido.desktop (`noido --install`).
                .with_app_id("noido")
                // GNOME on Wayland takes the desktop file's icon instead.
                .with_icon(icon::window_icon())
                // Used when no window was saved yet.
                .with_inner_size([900.0, 1100.0])
                .with_min_inner_size(MIN_WINDOW_SIZE),
        ),
        persist_window: true,
        persistence_path: shared
            .store
            .as_ref()
            .zip(opened.slot.as_ref())
            .map(|(store, slot)| store.window_file(slot)),
        ..Default::default()
    };
    let app = opened.app.with_theme(theme_live);
    let app = match activation {
        Some(activation) => app.with_activation(activation),
        None => app,
    };
    eframe::run_native_ext(
        "noido",
        options,
        Some(ctx),
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .map_err(|e| format!("cannot open window: {e}"))
}

/// What a launch on an already open file prints. The running noido answered,
/// but whether its window gets focus is up to the window manager.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn handed_over(path: &Path, target: Option<&Target>) -> String {
    let at = at(target);
    format!(
        "noido: {} is already open; asked its window to come forward{at}",
        path.display()
    )
}

/// Where a launch asked to go, as the end of a message.
fn at(target: Option<&Target>) -> String {
    match target {
        Some(Target::Page(page)) => format!(" at page {}", page + 1),
        Some(Target::Named(name)) => format!(" at {name:?}"),
        None => String::new(),
    }
}

/// noido draws its own title bar: on macOS under the system's transparent
/// one, which keeps its window buttons; elsewhere without the system frame.
fn own_title_bar(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") {
        builder
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_titlebar_buttons_shown(true)
    } else {
        builder.with_decorations(false)
    }
}

/// The root viewport, which holds the app and is never shown; see
/// `Shell`.
#[cfg(target_os = "macos")]
fn root_viewport() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("noido")
        // eframe gives the Dock the root's icon, or its own if there is none.
        .with_icon(icon::window_icon())
        .with_visible(false)
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn the_hidden_root_carries_the_app_icon() {
        let root = root_viewport();
        assert_eq!(root.visible, Some(false));
        assert_eq!(root.icon.as_deref(), Some(&icon::window_icon()));
    }

    #[test]
    fn version_is_the_package_version_and_the_commit() {
        let (version, commit) = env!("NOIDO_VERSION")
            .split_once('+')
            .expect("the version carries its commit after a +");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        let known = commit == "unknown";
        assert!(
            known || (commit.len() == 8 && commit.chars().all(|c| c.is_ascii_hexdigit())),
            "{commit} is neither a shortened commit nor unknown"
        );
    }

    #[test]
    fn the_parser_shows_the_stamped_version() {
        assert_eq!(
            Args::command().get_version(),
            Some(env!("NOIDO_VERSION")),
            "--version shows what the build script stamped"
        );
    }

    #[test]
    fn app_bundle_takes_a_dir_and_no_file() {
        let a = Args::try_parse_from(["noido", "--app-bundle", "out"]).unwrap();
        assert_eq!(a.app_bundle, Some(PathBuf::from("out")));
        assert!(Args::try_parse_from(["noido", "--app-bundle", "out", "a.pdf"]).is_err());
        assert!(Args::try_parse_from(["noido", "--app-bundle", "out", "--install"]).is_err());
    }

    #[test]
    fn install_and_uninstall_take_no_file() {
        let install = Args::try_parse_from(["noido", "--install"]).unwrap();
        assert!(install.install && install.path.is_none());
        let uninstall = Args::try_parse_from(["noido", "--uninstall"]).unwrap();
        assert!(uninstall.uninstall && uninstall.path.is_none());
    }

    #[test]
    fn a_file_is_required_otherwise() {
        assert!(Args::try_parse_from(["noido"]).is_err());
        let open = Args::try_parse_from(["noido", "a.pdf"]).unwrap();
        assert_eq!(open.path, Some(OsString::from("a.pdf")));
    }

    #[test]
    fn install_flags_conflict_with_a_file_and_each_other() {
        assert!(Args::try_parse_from(["noido", "--install", "a.pdf"]).is_err());
        assert!(Args::try_parse_from(["noido", "--uninstall", "a.pdf"]).is_err());
        assert!(Args::try_parse_from(["noido", "--install", "--uninstall"]).is_err());
    }

    #[test]
    fn handover_message_names_the_file() {
        assert_eq!(
            handed_over(Path::new("papers/a.pdf"), None),
            "noido: papers/a.pdf is already open; asked its window to come forward"
        );
    }

    #[test]
    fn handover_message_names_the_target() {
        assert_eq!(
            handed_over(Path::new("a.pdf"), Some(&Target::Page(50))),
            "noido: a.pdf is already open; asked its window to come forward at page 51"
        );
        assert_eq!(
            handed_over(Path::new("a.pdf"), Some(&Target::Named("x".into()))),
            "noido: a.pdf is already open; asked its window to come forward at \"x\""
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn foreground_takes_a_file_or_none_on_macos() {
        let none = Args::try_parse_from(["noido", "--foreground"]).unwrap();
        assert!(none.foreground && none.path.is_none());
        let file = Args::try_parse_from(["noido", "--foreground", "a.pdf"]).unwrap();
        assert!(file.foreground);
        assert_eq!(file.path, Some(OsString::from("a.pdf")));
        assert!(!Args::try_parse_from(["noido", "a.pdf"]).unwrap().foreground);
        assert!(Args::try_parse_from(["noido", "--foreground", "--install"]).is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn foreground_is_accepted_on_linux() {
        let file = Args::try_parse_from(["noido", "--foreground", "a.pdf"]).unwrap();
        assert!(file.foreground);
        assert_eq!(file.path, Some(OsString::from("a.pdf")));
    }

    #[test]
    fn answer_messages_name_the_file_and_target() {
        use instance::Answer;
        let path = Path::new("papers/a.pdf");
        let page = Some(&Target::Page(50));
        assert_eq!(
            answered(path, page, &Answer::Opened { pid: None }),
            "noido: opened papers/a.pdf at page 51 in the running noido"
        );
        assert_eq!(
            answered(path, None, &Answer::Opened { pid: Some(7) }),
            "noido: opened papers/a.pdf in the running noido"
        );
        assert_eq!(
            answered(path, page, &Answer::Forward { pid: Some(7) }),
            "noido: papers/a.pdf is already open; brought its window forward at page 51"
        );
        assert_eq!(
            answered(
                path,
                None,
                &Answer::Failed("cannot read papers/a.pdf".into())
            ),
            "cannot read papers/a.pdf"
        );
    }

    fn opening_of(args: &[&str]) -> Opening {
        opening(&Args::try_parse_from(args).unwrap()).unwrap()
    }

    #[test]
    fn page_flag() {
        assert_eq!(
            opening_of(&["noido", "a.pdf", "--page", "3"]),
            Opening {
                path: PathBuf::from("a.pdf"),
                target: Some(Target::Page(2)),
            }
        );
    }

    #[test]
    fn a_link_is_read() {
        assert_eq!(
            opening_of(&["noido", "noido:///b/a%20b.pdf#page=9"]),
            Opening {
                path: PathBuf::from("/b/a b.pdf"),
                target: Some(Target::Page(8)),
            }
        );
    }

    #[test]
    fn page_flag_wins_over_the_link() {
        assert_eq!(
            opening_of(&["noido", "noido:///b/a.pdf#page=9", "--page", "3"]).target,
            Some(Target::Page(2))
        );
    }

    #[test]
    fn page_zero_is_rejected() {
        assert!(Args::try_parse_from(["noido", "a.pdf", "--page", "0"]).is_err());
    }

    #[test]
    fn page_conflicts_with_install() {
        assert!(Args::try_parse_from(["noido", "--install", "--page", "3"]).is_err());
        assert!(Args::try_parse_from(["noido", "--uninstall", "--page", "3"]).is_err());
    }

    #[test]
    fn psn_arguments_are_dropped() {
        let args = without_psn(["noido", "-psn_0_12345", "a.pdf"].map(OsString::from));
        assert_eq!(args, ["noido", "a.pdf"].map(OsString::from));
    }

    #[test]
    fn finder_launch_is_no_argument_inside_a_bundle() {
        let bundled = Path::new("/A/noido.app/Contents/MacOS/noido");
        let bare = Path::new("/opt/homebrew/bin/noido");
        let none = [OsString::from("noido")];
        let one = ["noido", "a.pdf"].map(OsString::from);
        assert!(is_finder_launch(&none, bundled));
        assert!(!is_finder_launch(&none, bare));
        assert!(!is_finder_launch(&one, bundled));
    }

    #[test]
    fn file_urls_from_finder_are_read_exactly() {
        assert_eq!(
            opening_of(&["noido", "file:///b/a%20b%20%C3%A9%23.pdf"]).path,
            PathBuf::from("/b/a b é#.pdf")
        );
    }

    /// The started app's exit `code`, as `try_wait` gives it.
    #[cfg(target_os = "macos")]
    fn exit(code: i32) -> std::process::ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_started_app_that_lost_a_race_hands_over_to_the_winner() {
        use instance::Answer;
        // The started app found another one, started by a launch at the
        // same moment, and quit well; that one listens by now.
        let mut sends = 0;
        let answer = await_app(
            || {
                sends += 1;
                (sends > 1).then_some(Answer::Opened { pid: Some(7) })
            },
            || Some(exit(0)),
            Duration::from_secs(5),
            Duration::ZERO,
        );
        assert_eq!(answer, Ok(Some(Answer::Opened { pid: Some(7) })));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_started_app_that_quit_with_nobody_listening_is_a_failure() {
        for code in [0, 1] {
            let answer = await_app(
                || None,
                || Some(exit(code)),
                Duration::from_secs(5),
                Duration::ZERO,
            );
            assert!(
                matches!(&answer, Err(e) if e.contains("quit at once")),
                "{code}: {answer:?}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_started_app_that_fails_is_not_asked_again() {
        let mut sends = 0;
        let answer = await_app(
            || {
                sends += 1;
                None
            },
            || Some(exit(1)),
            Duration::from_secs(5),
            Duration::ZERO,
        );
        assert!(answer.is_err());
        assert_eq!(sends, 1);
    }
}
