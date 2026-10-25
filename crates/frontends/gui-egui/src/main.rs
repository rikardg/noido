//! `noido <file.pdf>`: open a PDF in the egui frontend.

mod app;
mod load;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use eframe::egui;
use engine_hayro::HayroEngine;
use noido_render::Renderer;

use crate::app::NoidoApp;
use crate::load::load_document;

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
    let bytes = load_document(path)?;

    // Created up front so the render thread can wake the UI when a page is ready.
    let ctx = egui::Context::default();
    // Mod +/-/0 zoom the document, not egui's interface.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    let waker_ctx = ctx.clone();
    let waker = Box::new(move || waker_ctx.request_repaint());
    let (renderer, info) = Renderer::start(Arc::new(HayroEngine), bytes, waker)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("{name} — noido"))
            .with_inner_size([900.0, 1100.0]),
        ..Default::default()
    };
    eframe::run_native_ext(
        "noido",
        options,
        Some(ctx),
        Box::new(|_cc| Ok(Box::new(NoidoApp::new(renderer, info)))),
    )
    .map_err(|e| format!("cannot open window: {e}"))
}
