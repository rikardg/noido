//! The render thread. PDF engines are not thread-safe, so one thread opens
//! the document, owns it, and renders pages on request. A second thread
//! renders thumbnails on its own copy, a third extracts and searches text,
//! a fourth reads links, and a short-lived one reads the document's
//! properties.

mod links;
mod properties;
mod text;
mod thumbnails;
mod transformed;

pub use links::{FLASH_BAR_HEIGHT, LinkReply, Linker};
pub use properties::read_properties;
pub use text::{MAX_HITS, SearchUpdate, TextSearcher};
pub use thumbnails::{Thumbnail, Thumbnailer};
pub use transformed::{Transformed, Transforms, open_transformed};

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use noido_core::{Keep, PageTransform};
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, Rgba8Image, Size};

/// Runs `f`, reporting a panic as an [`EngineError`] rather than letting it
/// end the thread that asked: one page the engine cannot survive must not
/// cost the reader every page after it.
///
/// The panic hook has already printed what panicked and where; the error
/// carries the message, so the page's failure names it too.
pub(crate) fn caught<T>(f: impl FnOnce() -> Result<T, EngineError>) -> Result<T, EngineError> {
    // The engine is not `UnwindSafe`, and does not need to be: nothing reads
    // state a half-finished render left behind. The parse is only read, and
    // each render builds its own cache.
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => Err(EngineError::Render(format!(
            "the engine panicked: {}",
            panic_message(&*payload)
        ))),
    }
}

/// [`caught`] for a call that answers with an `Option`: a panic is `None`.
pub(crate) fn caught_none<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    caught(|| Ok(f())).unwrap_or_else(|e| {
        eprintln!("noido: {e}");
        None
    })
}

/// What a panic was raised with, where it can be read.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

/// `page`'s glyphs, leaving out any drawn wholly outside the page.
fn glyphs_on_page(
    document: &dyn Document,
    page: usize,
) -> Result<Vec<noido_engine::Glyph>, EngineError> {
    caught(|| {
        let glyphs = document.text(page)?;
        Ok(match document.page_size(page) {
            Ok(size) => noido_text::on_page(glyphs, size.width, size.height),
            Err(_) => glyphs,
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderRequest {
    pub page: usize,
    pub scale: f32,
}

pub struct RenderedPage {
    pub page: usize,
    pub scale: f32,
    /// The page's transform when it was rendered.
    pub transform: PageTransform,
    pub result: Result<Rgba8Image, EngineError>,
}

#[derive(Clone, Debug)]
pub struct DocumentInfo {
    /// Each page as shown: turned and scaled by `transforms`.
    pub page_sizes: Vec<Size>,
    /// Each page as the engine has it: unturned and unscaled.
    pub engine_sizes: Vec<Size>,
    /// The transforms the document was opened with, fitted to its pages
    /// when scale to fit is on. The shared transforms are the app's to
    /// write; opening does not touch them.
    pub transforms: noido_core::PageTransforms,
}

/// Handle to the render thread. Dropping it stops the thread after its current job.
pub struct Renderer {
    jobs: Sender<RenderRequest>,
    results: Receiver<RenderedPage>,
    keep: Arc<Mutex<Keep>>,
    priority: Arc<Mutex<Vec<usize>>>,
}

impl Renderer {
    /// Starts the render thread and opens the document on it. Blocks until the
    /// document is open; `waker` is called after each finished render.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        transforms: Transforms,
        waker: Box<dyn Fn() + Send>,
    ) -> Result<(Renderer, DocumentInfo), EngineError> {
        let (opened_tx, opened_rx) = mpsc::sync_channel(1);
        let renderer = Self::spawn(engine, bytes, transforms, waker, Some(opened_tx))
            .map_err(|e| EngineError::Open(format!("cannot start render thread: {e}")))?;
        let info = opened_rx
            .recv()
            .map_err(|_| EngineError::Open("render thread stopped while opening".into()))??;
        Ok((renderer, info))
    }

    /// Starts the render thread, which opens the document itself; returns at
    /// once. Requests wait until the document is open. If it fails to open,
    /// the thread stops and nothing arrives.
    pub fn start_background(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        transforms: Transforms,
        waker: Box<dyn Fn() + Send>,
    ) -> Renderer {
        match Self::spawn(engine, bytes, transforms, waker, None) {
            Ok(renderer) => renderer,
            // Without a thread nothing is rendered, as when opening fails.
            Err(_) => {
                let (jobs, _) = mpsc::channel();
                let (_, results) = mpsc::channel();
                Renderer {
                    jobs,
                    results,
                    keep: Arc::new(Mutex::new(Keep::from(0..usize::MAX))),
                    priority: Arc::new(Mutex::new(Vec::new())),
                }
            }
        }
    }

    /// Spawns the thread; it reports the result of opening to `opened`.
    fn spawn(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        transforms: Transforms,
        waker: Box<dyn Fn() + Send>,
        opened: Option<mpsc::SyncSender<Result<DocumentInfo, EngineError>>>,
    ) -> std::io::Result<Renderer> {
        let (jobs, job_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        let keep = Arc::new(Mutex::new(Keep::from(0..usize::MAX)));
        let thread_keep = keep.clone();
        let priority = Arc::new(Mutex::new(Vec::new()));
        let thread_priority = priority.clone();

        thread::Builder::new()
            .name("noido-render".into())
            .spawn(move || {
                let document = match open(engine.as_ref(), bytes, &transforms) {
                    Ok((document, info)) => {
                        if let Some(opened) = opened {
                            let _ = opened.send(Ok(info));
                        }
                        document
                    }
                    Err(e) => {
                        if let Some(opened) = opened {
                            let _ = opened.send(Err(e));
                        }
                        return;
                    }
                };
                run(
                    &document,
                    &job_rx,
                    &result_tx,
                    &thread_keep,
                    &thread_priority,
                    &*waker,
                );
            })?;
        Ok(Renderer {
            jobs,
            results,
            keep,
            priority,
        })
    }

    pub fn request(&self, req: RenderRequest) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.jobs.send(req);
    }

    /// Pages outside `keep` are dropped from the queue without rendering and
    /// without a result. Set it before requesting pages for the same range,
    /// and forget pending requests outside it.
    pub fn set_keep(&self, keep: impl Into<Keep>) {
        *self.keep.lock().unwrap() = keep.into();
    }

    /// The pages kept, as last set.
    pub fn keep(&self) -> Keep {
        self.keep.lock().unwrap().clone()
    }

    /// The pages the reader needs most, most urgent first. Queued requests
    /// for these pages are rendered in this order, before any others, which
    /// follow in the order they were requested.
    pub fn set_priority(&self, pages: Vec<usize>) {
        *self.priority.lock().unwrap() = pages;
    }

    pub fn try_recv(&self) -> Option<RenderedPage> {
        self.results.try_recv().ok()
    }
}

fn open(
    engine: &dyn PdfEngine,
    bytes: PdfBytes,
    transforms: &Transforms,
) -> Result<(Transformed, DocumentInfo), EngineError> {
    let document = open_transformed(engine, bytes, transforms)?;
    let engine_sizes = document.engine_sizes()?;
    let mut fitted = transforms.snapshot();
    fitted.refit(&engine_sizes);
    let page_sizes = engine_sizes
        .iter()
        .enumerate()
        .map(|(page, &size)| fitted.get(page).size(size))
        .collect();
    Ok((
        document,
        DocumentInfo {
            page_sizes,
            engine_sizes,
            transforms: fitted,
        },
    ))
}

/// `page`, of `size`, rendered at about `scale`: stretched to the nearest
/// whole pixels, so no edge pixel is partly background.
pub(crate) fn render_whole(
    document: &dyn Document,
    page: usize,
    size: Size,
    scale: f32,
) -> Result<Rgba8Image, EngineError> {
    let pixels = |points: f32| ((points * scale).round() as u32).max(1);
    document.render_page(page, pixels(size.width), pixels(size.height))
}

fn run(
    document: &Transformed,
    jobs: &Receiver<RenderRequest>,
    results: &Sender<RenderedPage>,
    keep: &Mutex<Keep>,
    priority: &Mutex<Vec<usize>>,
    waker: &dyn Fn(),
) {
    // The latest request per page, in order of first arrival.
    let mut queue: Vec<RenderRequest> = Vec::new();
    loop {
        if queue.is_empty() {
            match jobs.recv() {
                Ok(req) => queue.push(req),
                Err(_) => return,
            }
        }
        for req in jobs.try_iter() {
            match queue.iter_mut().find(|r| r.page == req.page) {
                Some(existing) => existing.scale = req.scale,
                None => queue.push(req),
            }
        }
        // Chosen again after every page, so a reader who has moved on is
        // served next instead of waiting for pages they scrolled past.
        let next = {
            let priority = priority.lock().unwrap();
            let rank = |page| priority.iter().position(|&p| p == page);
            (0..queue.len())
                .min_by_key(|&i| rank(queue[i].page).unwrap_or(usize::MAX))
                .expect("queue is not empty")
        };
        let RenderRequest { page, scale } = queue.remove(next);
        if !keep.lock().unwrap().contains(page) {
            continue;
        }
        // Read before the render, so a turn under way stamps it old.
        let transform = document.transform(page);
        let result = caught(|| {
            document
                .page_size(page)
                .and_then(|size| render_whole(document, page, size, scale))
        });
        if results
            .send(RenderedPage {
                page,
                scale,
                transform,
                result,
            })
            .is_err()
        {
            return;
        }
        waker();
    }
}
