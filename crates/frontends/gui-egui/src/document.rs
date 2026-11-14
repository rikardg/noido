//! One document window: its worker threads and the app built on them.

use std::path::Path;
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

    /// The settings as last saved, for a document opening now: a window
    /// may have saved them since the start. The ones read at the start if
    /// the file can't be read now.
    pub fn current_settings(&self) -> Settings {
        match self.settings_file.as_ref().map(SettingsFile::load) {
            Some(Ok(settings)) => settings,
            _ => self.settings,
        }
    }
}

/// A document's app, with what the shell needs to put it in a window.
pub struct Opened {
    pub app: NoidoApp,
    /// The file name, for the window title; on macOS the shell takes it
    /// from the app.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub name: String,
    /// Where eframe keeps the window's place, elsewhere than on macOS;
    /// there the shell keeps it in the document's state.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
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
    let thumbnailer = Thumbnailer::start(
        Arc::new(HayroEngine),
        bytes.clone(),
        noido_render::Transforms::default(),
        waker(ctx, viewport),
    );
    let sidebar_renderer = Renderer::start_background(
        Arc::new(HayroEngine),
        bytes.clone(),
        noido_render::Transforms::default(),
        waker(ctx, viewport),
    );
    let searcher = TextSearcher::start(
        Arc::new(HayroEngine),
        bytes.clone(),
        noido_render::Transforms::default(),
        waker(ctx, viewport),
    );
    let linker = Linker::start(
        Arc::new(HayroEngine),
        bytes.clone(),
        noido_render::Transforms::default(),
        waker(ctx, viewport),
    );
    let source = info::Source {
        engine: Arc::new(HayroEngine),
        bytes: bytes.clone(),
        // As it was given: the info page makes it absolute itself, so that
        // what it shows is decided in one place (and `..` folded there).
        path: path.to_owned(),
    };
    let (renderer, info) = Renderer::start(
        Arc::new(HayroEngine),
        bytes,
        noido_render::Transforms::default(),
        waker(ctx, viewport),
    )
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
        .with_link_path(canonical)
        .with_thumbnails(thumbnailer)
        .with_sidebar_renderer(sidebar_renderer)
        .with_searcher(searcher)
        .with_linker(linker)
        .with_source(source);
    let app = match watcher {
        Some(watcher) => app.with_watcher(watcher),
        None => app,
    };
    let app = app.with_settings(shared.settings_file.clone(), shared.current_settings());
    let app = match session {
        Some((store, slot, restored)) => app.with_persistence(store, slot, restored),
        None => app,
    };
    Ok(Opened {
        app: app.opening_at(target),
        name,
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

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures")
            .join(name)
    }

    #[test]
    fn a_new_document_takes_the_settings_saved_since_the_start() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = SettingsFile::at(dir.path());
        let shared = Shared {
            settings_file: Some(file.clone()),
            settings: Settings::default(),
            store: None,
        };
        // Saved in a window after the shell loaded its settings.
        let mut saved = Settings {
            paper: noido_core::Paper::Sepia,
            ..Settings::default()
        };
        saved.new_documents.gapless = !saved.new_documents.gapless;
        file.save(&saved).unwrap();
        let ctx = egui::Context::default();
        let opened = open(&fixture("text.pdf"), None, &ctx, ViewportId::ROOT, &shared).unwrap();
        assert_eq!(crate::app::tests::settings_of(&opened.app), saved);
    }
}
