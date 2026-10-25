//! The render thread. PDF engines are not thread-safe, so one thread opens
//! the document, owns it, and renders pages on request. A second thread
//! renders thumbnails on its own copy.

mod thumbnails;

pub use thumbnails::{Thumbnail, Thumbnailer};

use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderRequest {
    pub page: usize,
    pub scale: f32,
}

pub struct RenderedPage {
    pub page: usize,
    pub scale: f32,
    pub result: Result<Rgba8Image, EngineError>,
}

#[derive(Clone, Debug)]
pub struct DocumentInfo {
    pub page_sizes: Vec<Size>,
}

/// Handle to the render thread. Dropping it stops the thread after its current job.
pub struct Renderer {
    jobs: Sender<RenderRequest>,
    results: Receiver<RenderedPage>,
    keep: Arc<Mutex<Range<usize>>>,
    priority: Arc<Mutex<Vec<usize>>>,
}

impl Renderer {
    /// Starts the render thread and opens the document on it. Blocks until the
    /// document is open; `waker` is called after each finished render.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        waker: Box<dyn Fn() + Send>,
    ) -> Result<(Renderer, DocumentInfo), EngineError> {
        let (jobs, job_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        let (opened_tx, opened_rx) = mpsc::sync_channel(1);
        let keep = Arc::new(Mutex::new(0..usize::MAX));
        let thread_keep = keep.clone();
        let priority = Arc::new(Mutex::new(Vec::new()));
        let thread_priority = priority.clone();

        thread::Builder::new()
            .name("noido-render".into())
            .spawn(move || {
                let document = match open(engine.as_ref(), bytes) {
                    Ok((document, info)) => {
                        let _ = opened_tx.send(Ok(info));
                        document
                    }
                    Err(e) => {
                        let _ = opened_tx.send(Err(e));
                        return;
                    }
                };
                run(
                    document.as_ref(),
                    &job_rx,
                    &result_tx,
                    &thread_keep,
                    &thread_priority,
                    &*waker,
                );
            })
            .map_err(|e| EngineError::Open(format!("cannot start render thread: {e}")))?;

        let info = opened_rx
            .recv()
            .map_err(|_| EngineError::Open("render thread stopped while opening".into()))??;
        Ok((
            Renderer {
                jobs,
                results,
                keep,
                priority,
            },
            info,
        ))
    }

    pub fn request(&self, req: RenderRequest) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.jobs.send(req);
    }

    /// Pages outside `keep` are dropped from the queue without rendering and
    /// without a result. Set it before requesting pages for the same range,
    /// and forget pending requests outside it.
    pub fn set_keep(&self, keep: Range<usize>) {
        *self.keep.lock().unwrap() = keep;
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
) -> Result<(Box<dyn Document>, DocumentInfo), EngineError> {
    let document = engine.open(bytes)?;
    let page_sizes = (0..document.page_count())
        .map(|page| document.page_size(page))
        .collect::<Result<_, _>>()?;
    Ok((document, DocumentInfo { page_sizes }))
}

fn run(
    document: &dyn Document,
    jobs: &Receiver<RenderRequest>,
    results: &Sender<RenderedPage>,
    keep: &Mutex<Range<usize>>,
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
        if !keep.lock().unwrap().contains(&page) {
            continue;
        }
        let result = document
            .page_size(page)
            .and_then(|size| document.render_tile(page, scale, PixelRect::full_page(size, scale)));
        if results
            .send(RenderedPage {
                page,
                scale,
                result,
            })
            .is_err()
        {
            return;
        }
        waker();
    }
}
