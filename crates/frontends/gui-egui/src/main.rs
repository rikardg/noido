//! `noido <file.pdf>`: open a PDF in the egui frontend.

mod app;
mod frame;
mod image;
mod instance;
mod interrupt;
mod load;
mod search;
mod sidebar;
mod titlebar;
mod watch;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use eframe::egui;
use engine_hayro::HayroEngine;
use noido_render::{Renderer, TextSearcher, Thumbnailer};
use noido_state::{DocKey, Store};

use crate::app::NoidoApp;
use crate::instance::Claim;
use crate::load::load_document;
use crate::watch::Watcher;

/// Documents whose state is kept; older ones are forgotten.
const REMEMBERED_DOCUMENTS: usize = 1000;

/// The smallest window in which the title bar's controls all fit.
const MIN_WINDOW_SIZE: [f32; 2] = [480.0, 300.0];

/// UI points scrolled per mouse wheel step (egui's default is 40).
const WHEEL_SCROLL_SPEED: f32 = 125.0;

#[derive(Parser)]
#[command(name = "noido", version, about = "PDF reader and note taker")]
struct Args {
    /// PDF file to open
    path: PathBuf,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args.path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("noido: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &Path) -> Result<(), String> {
    // Before reading the file: a launch on an open file only hands over.
    let listener = match instance::claim(path) {
        Claim::Running => {
            eprintln!("{}", handed_over(path));
            return Ok(());
        }
        Claim::First(listener) => Some(listener),
        Claim::Unavailable => None,
    };
    let bytes = load_document(path)?;
    let key = DocKey::new(&bytes, path);
    // Read before the window file is asked for: the entry names it.
    let session = Store::open_default().map(|store| {
        store.prune(REMEMBERED_DOCUMENTS);
        let (slot, restored) = store.load(key.clone());
        (store, slot, restored)
    });

    // Created up front so the render thread can wake the UI when a page is ready.
    let ctx = egui::Context::default();
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
    let waker = |ctx: &egui::Context| {
        let ctx = ctx.clone();
        Box::new(move || ctx.request_repaint())
    };
    // Shares the document bytes with the renderer; opens it in the background.
    let thumbnailer = Thumbnailer::start(Arc::new(HayroEngine), bytes.clone(), waker(&ctx));
    let sidebar_renderer =
        Renderer::start_background(Arc::new(HayroEngine), bytes.clone(), waker(&ctx));
    let searcher = TextSearcher::start(Arc::new(HayroEngine), bytes.clone(), waker(&ctx));
    let (renderer, info) = Renderer::start(Arc::new(HayroEngine), bytes, waker(&ctx))
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let wake: watch::Waker = {
        let ctx = ctx.clone();
        Arc::new(move || ctx.request_repaint())
    };
    let activation = listener.map(|listener| listener.serve(wake.clone()));
    let watcher = Watcher::start(path, key, Arc::new(HayroEngine), wake)
        .inspect_err(|e| eprintln!("noido: cannot watch {}: {e}", path.display()))
        .ok();

    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let options = eframe::NativeOptions {
        viewport: own_title_bar(
            egui::ViewportBuilder::default()
                .with_title(format!("{name} — noido"))
                // Used when no window was saved yet.
                .with_inner_size([900.0, 1100.0])
                .with_min_inner_size(MIN_WINDOW_SIZE),
        ),
        persist_window: true,
        persistence_path: session
            .as_ref()
            .map(|(store, slot, _)| store.window_file(slot)),
        ..Default::default()
    };
    let app = NoidoApp::new(renderer, info)
        .with_name(name.clone())
        .with_thumbnails(thumbnailer)
        .with_sidebar_renderer(sidebar_renderer)
        .with_searcher(searcher);
    let app = match activation {
        Some(activation) => app.with_activation(activation),
        None => app,
    };
    let app = match watcher {
        Some(watcher) => app.with_watcher(watcher),
        None => app,
    };
    let app = match session {
        Some((store, slot, restored)) => app.with_persistence(store, slot, restored),
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
fn handed_over(path: &Path) -> String {
    format!(
        "noido: {} is already open; asked its window to come forward",
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
    use super::*;

    #[test]
    fn handover_message_names_the_file() {
        assert_eq!(
            handed_over(Path::new("papers/a.pdf")),
            "noido: papers/a.pdf is already open; asked its window to come forward"
        );
    }
}
