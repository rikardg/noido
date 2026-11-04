//! One document window: its worker threads and the app built on them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use engine_hayro::HayroEngine;
use noido_core::Target;
use noido_render::{Linker, Renderer, TextSearcher, Thumbnailer};
use noido_settings::{Settings, SettingsFile};
use noido_state::{DocKey, Slot, Store};

use crate::app::NoidoApp;
use crate::info;
use crate::load::load_document;
use crate::watch::{self, Watcher};

/// Documents whose state is kept; older ones are forgotten.
const REMEMBERED_DOCUMENTS: usize = 1000;

/// What every document window shares: the settings and the state store.
pub struct Shared {
    pub settings_file: Option<SettingsFile>,
    pub settings: Settings,
    pub store: Option<Store>,
}

impl Shared {
    pub fn load() -> Self {
        let store = Store::open_default();
        if let Some(store) = &store {
            store.prune(REMEMBERED_DOCUMENTS);
        }
        let settings_file = SettingsFile::open_default();
        let settings = settings_file
            .as_ref()
            .map_or_else(Settings::default, |file| {
                file.load().unwrap_or_else(|e| {
                    let path = file.path().display();
                    eprintln!("noido: cannot read settings ({path}): {e}; using the defaults");
                    Settings::default()
                })
            });
        Self {
            settings_file,
            settings,
            store,
        }
    }
}

/// A document's app, with what the shell needs to put it in a window.
pub struct Opened {
    pub app: NoidoApp,
    /// The file name, for the window title.
    pub name: String,
    /// For the shell to tell open documents apart; unused until it does.
    #[allow(dead_code)]
    pub canonical: PathBuf,
    /// Where the document's window state is kept.
    pub slot: Option<Slot>,
}

/// Wakes `viewport` and no other, for a worker thread to call.
pub fn waker(ctx: &egui::Context, viewport: egui::ViewportId) -> Box<dyn Fn() + Send> {
    let ctx = ctx.clone();
    Box::new(move || ctx.request_repaint_of(viewport))
}

/// The same waker, shareable between threads.
pub fn shared_waker(ctx: &egui::Context, viewport: egui::ViewportId) -> watch::Waker {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint_of(viewport))
}

/// Loads `path`, starts its threads and watcher, and builds its app.
pub fn open(
    path: &Path,
    target: Option<Target>,
    ctx: &egui::Context,
    viewport: egui::ViewportId,
    shared: &Shared,
) -> Result<Opened, String> {
    let bytes = load_document(path)?;
    let key = DocKey::new(&bytes, path);
    // Read before the window file is asked for: the entry names it.
    let session = shared.store.as_ref().map(|store| {
        let (slot, restored) = store.load(key.clone());
        (store.clone(), slot, restored)
    });

    // Shares the document bytes with the renderer; opens it in the background.
    let thumbnailer =
        Thumbnailer::start(Arc::new(HayroEngine), bytes.clone(), waker(ctx, viewport));
    let sidebar_renderer =
        Renderer::start_background(Arc::new(HayroEngine), bytes.clone(), waker(ctx, viewport));
    let searcher = TextSearcher::start(Arc::new(HayroEngine), bytes.clone(), waker(ctx, viewport));
    let linker = Linker::start(Arc::new(HayroEngine), bytes.clone(), waker(ctx, viewport));
    let source = info::Source {
        engine: Arc::new(HayroEngine),
        bytes: bytes.clone(),
        // The info page shows the folder: a relative one says little.
        path: std::path::absolute(path).unwrap_or_else(|_| path.to_owned()),
    };
    let (renderer, info) = Renderer::start(Arc::new(HayroEngine), bytes, waker(ctx, viewport))
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let watcher = Watcher::start(
        path,
        key,
        Arc::new(HayroEngine),
        shared_waker(ctx, viewport),
    )
    .inspect_err(|e| eprintln!("noido: cannot watch {}: {e}", path.display()))
    .ok();

    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let slot = session.as_ref().map(|(_, slot, _)| slot.clone());
    let app = NoidoApp::new(renderer, info)
        .with_name(name.clone())
        .with_link_path(canonical.clone())
        .with_thumbnails(thumbnailer)
        .with_sidebar_renderer(sidebar_renderer)
        .with_searcher(searcher)
        .with_linker(linker)
        .with_source(source);
    let app = match watcher {
        Some(watcher) => app.with_watcher(watcher),
        None => app,
    };
    let app = app.with_settings(shared.settings_file.clone(), shared.settings);
    let app = match session {
        Some((store, slot, restored)) => app.with_persistence(store, slot, restored),
        None => app,
    };
    Ok(Opened {
        app: app.opening_at(target),
        name,
        canonical,
        slot,
    })
}

#[cfg(test)]
mod tests {
    use eframe::egui::{self, ViewportId};

    use super::*;

    #[test]
    fn a_waker_repaints_its_own_viewport() {
        let ctx = egui::Context::default();
        let id = ViewportId::from_hash_of("doc");
        let _ = ctx.run_logic(&egui::RawInput::default(), |_| {});
        waker(&ctx, id)();
        assert!(ctx.has_requested_repaint_for(&id));
    }
}
