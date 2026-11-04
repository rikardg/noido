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

use clap::Parser;
use eframe::egui;
use noido_core::{Opening, Target, parse_argument};

use crate::instance::Claim;

/// The smallest window in which the title bar's controls all fit.
const MIN_WINDOW_SIZE: [f32; 2] = [480.0, 300.0];

/// UI points scrolled per mouse wheel step (egui's default is 40).
const WHEEL_SCROLL_SPEED: f32 = 125.0;

#[derive(Parser)]
#[command(
    name = "noido",
    version = env!("NOIDO_VERSION"),
    about = "PDF reader and note taker"
)]
struct Args {
    /// PDF file, or a file: or noido: link, to open
    #[arg(required_unless_present_any = ["install", "uninstall", "app_bundle"])]
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
        && let Some(bundle) = app_bundle::bundle_of(&exe)
    {
        noido_macos::run_launcher(&bundle);
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
        Some(_) => opening(&args).and_then(|opening| {
            #[cfg(target_os = "macos")]
            hint_if_stale();
            run(opening)
        }),
        None => unreachable!("clap requires a file without --install or --uninstall"),
    };
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

fn run(opening: Opening) -> Result<(), String> {
    let path = opening.path.as_path();
    let target = opening.target;
    // Before reading the file: a launch on an open file only hands over.
    let listener = match instance::claim(path, target.as_ref()) {
        Claim::Running { pid } => {
            // The sender brings the window forward: a window can't bring
            // itself to the front on macOS.
            #[cfg(target_os = "macos")]
            if let Some(pid) = pid {
                noido_macos::activate_pid(pid);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = pid;
            eprintln!("{}", handed_over(path, target.as_ref()));
            return Ok(());
        }
        Claim::First(listener) => Some(listener),
        Claim::Unavailable => None,
    };
    let shared = document::Shared::load();

    // Created up front so the render thread can wake the UI when a page is ready.
    let ctx = egui::Context::default();
    let theme = theme::Theme::from_system();
    theme.install(&ctx);
    let theme_live = theme::Live::start(&ctx);
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
    #[cfg(target_os = "macos")]
    let app = app.with_incoming(app::Incoming {
        start: Some(Box::new(|ctx| {
            let ctx = ctx.clone();
            noido_macos::install_open_urls(move || ctx.request_repaint());
        })),
        take: Box::new(noido_macos::take_opened),
    });
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
fn handed_over(path: &Path, target: Option<&Target>) -> String {
    let at = match target {
        Some(Target::Page(page)) => format!(" at page {}", page + 1),
        Some(Target::Named(name)) => format!(" at {name:?}"),
        None => String::new(),
    };
    format!(
        "noido: {} is already open; asked its window to come forward{at}",
        path.display()
    )
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

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
}
