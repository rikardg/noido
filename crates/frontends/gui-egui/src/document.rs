//! One document window: its worker threads and the app built on them.

use std::path::Path;
use std::sync::Arc;

use eframe::egui;
use engine_comic::ComicEngine;
use engine_hayro::HayroEngine;
use noido_core::Target;
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine};
use noido_render::{Linker, Renderer, TextSearcher, Thumbnailer};
use noido_settings::{Settings, SettingsFile};
use noido_state::{Slot, Store};
use noido_typeset::TypesetEngine;

use crate::app::NoidoApp;
use crate::info;
use crate::load::load_document;
use crate::watch::{self, Watcher};

/// Documents whose state is kept; older ones are forgotten.
const REMEMBERED_DOCUMENTS: usize = 1000;

/// The engine for a file: hayro for a PDF, the typesetter for an EPUB, the
/// comic engine for another ZIP, the typesetter for text. Other bytes that hold a PDF header go to hayro,
/// whose error names them. One per window, its `Arc` cloned to every
/// thread, so that a text file is laid out once.
#[derive(Default)]
pub(crate) struct AnyEngine {
    typeset: TypesetEngine,
}

impl AnyEngine {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

impl PdfEngine for AnyEngine {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        if bytes.starts_with(b"%PDF-") {
            HayroEngine.open(bytes)
        } else if bytes.starts_with(b"PK\x03\x04") {
            if noido_epub::is_epub(&bytes) {
                self.typeset.open(bytes)
            } else {
                ComicEngine.open(bytes)
            }
        } else if bytes[..bytes.len().min(1024)]
            .windows(5)
            .any(|window| window == b"%PDF-")
        {
            // A PDF may carry junk before its header; let hayro name it,
            // unless it is text that merely mentions a header.
            match HayroEngine.open(bytes.clone()) {
                Err(_) if std::str::from_utf8(&bytes).is_ok() => self.typeset.open(bytes),
                opened => opened,
            }
        } else {
            self.typeset.open(bytes)
        }
    }
}

/// What every document window shares: the settings and the store.
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
    let key = crate::load::doc_key(&bytes, path);
    // Read before the window file is asked for: the entry names it.
    let session = shared.store.as_ref().map(|store| {
        let (slot, restored) = store.load(key.clone());
        (store.clone(), slot, restored)
    });

    // The reader's turns, from the start: the threads hand out pages turned.
    let transforms = noido_render::Transforms::new(
        session
            .as_ref()
            .and_then(|(_, _, restored)| restored.as_ref())
            .map(|state| state.transforms.clone())
            .unwrap_or_default(),
    );

    // One engine for every thread: a text file is laid out once.
    let engine = Arc::new(AnyEngine::new());
    // Shares the document bytes with the renderer; opens it in the background.
    let thumbnailer = Thumbnailer::start(
        engine.clone(),
        bytes.clone(),
        transforms.clone(),
        waker(ctx, viewport),
    );
    let sidebar_renderer = Renderer::start_background(
        engine.clone(),
        bytes.clone(),
        transforms.clone(),
        waker(ctx, viewport),
    );
    let searcher = TextSearcher::start(
        engine.clone(),
        bytes.clone(),
        transforms.clone(),
        waker(ctx, viewport),
    );
    let linker = Linker::start(
        engine.clone(),
        bytes.clone(),
        transforms.clone(),
        waker(ctx, viewport),
    );
    let source = info::Source {
        engine: engine.clone(),
        bytes: bytes.clone(),
        // As it was given: the info page makes it absolute itself, so that
        // what it shows is decided in one place (and `..` folded there).
        path: path.to_owned(),
    };
    let (renderer, info) = Renderer::start(
        engine.clone(),
        bytes,
        transforms.clone(),
        waker(ctx, viewport),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;

    let watcher = Watcher::start(
        path,
        key,
        engine.clone(),
        transforms.clone(),
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
        .with_transforms(transforms)
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
    fn unknown_bytes_are_not_called_a_pdf() {
        let error = match AnyEngine::new().open(PdfBytes::from(vec![0xff, 0xfe, 0x00])) {
            Err(error) => error,
            Ok(_) => panic!("opened junk"),
        };
        assert_eq!(error.to_string(), "not a readable document");
    }

    #[test]
    fn a_cbz_open_error_names_the_cbz() {
        let bytes = PdfBytes::from(std::fs::read(fixture("comic-empty.cbz")).unwrap());
        let error = match AnyEngine::new().open(bytes) {
            Err(error) => error,
            Ok(_) => panic!("opened an archive without images"),
        };
        let message = error.to_string();
        assert!(message.contains("CBZ"), "{message}");
        assert!(!message.contains("PDF"), "{message}");
    }

    #[test]
    fn an_epub_opens_through_the_typesetter() {
        let bytes = PdfBytes::from(std::fs::read(fixture("book.epub")).unwrap());
        let document = AnyEngine::new().open(bytes).unwrap();
        assert_eq!(document.properties().format, noido_engine::Format::Epub);
    }

    #[test]
    fn a_cbz_still_opens_as_a_comic() {
        let bytes = PdfBytes::from(std::fs::read(fixture("comic.cbz")).unwrap());
        let document = AnyEngine::new().open(bytes).unwrap();
        assert_eq!(document.properties().format, noido_engine::Format::Comic);
    }

    #[test]
    fn utf8_text_opens_as_typeset_pages() {
        let document = AnyEngine::new()
            .open(PdfBytes::from(b"# Hi\n\nThere.\n".to_vec()))
            .unwrap();
        assert_eq!(document.page_count(), 1);
        assert_eq!(document.properties().format, noido_engine::Format::Markdown);
    }

    #[test]
    fn text_that_mentions_a_pdf_header_still_opens_as_text() {
        let document = AnyEngine::new()
            .open(PdfBytes::from(
                b"Notes on %PDF-1.7 files\n\nMore text.\n".to_vec(),
            ))
            .unwrap();
        assert_eq!(document.properties().format, noido_engine::Format::Markdown);
    }

    #[test]
    fn a_pdf_with_junk_before_its_header_opens_as_a_pdf() {
        let mut bytes = b"junk\r\n".to_vec();
        bytes.extend(std::fs::read(fixture("two-pages.pdf")).unwrap());
        let document = AnyEngine::new().open(PdfBytes::from(bytes)).unwrap();
        assert_eq!(document.properties().format, noido_engine::Format::Pdf);
        assert_eq!(document.page_count(), 2);
    }

    #[test]
    fn a_cbz_is_opened_by_the_comic_engine() {
        let bytes = PdfBytes::from(std::fs::read(fixture("comic.cbz")).unwrap());
        let document = AnyEngine::new().open(bytes).unwrap();
        assert_eq!(document.page_count(), 4);
    }

    #[test]
    fn a_comic_opens_in_a_window() {
        let ctx = egui::Context::default();
        let shared = Shared {
            settings_file: None,
            settings: Settings::default(),
            store: None,
        };
        let opened = open(&fixture("comic.cbz"), None, &ctx, ViewportId::ROOT, &shared).unwrap();
        assert_eq!(opened.name, "comic.cbz");
    }

    #[test]
    fn a_window_gives_every_thread_the_same_engine() {
        let ctx = egui::Context::default();
        let shared = Shared {
            settings_file: None,
            settings: Settings::default(),
            store: None,
        };
        let opened = open(&fixture("sample.md"), None, &ctx, ViewportId::ROOT, &shared).unwrap();
        let engine = opened.app.source_engine().unwrap();
        // This one, and at least the render and text threads' clones.
        assert!(
            Arc::strong_count(&engine) > 2,
            "{}",
            Arc::strong_count(&engine)
        );
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
