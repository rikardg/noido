//! The render thread. PDF engines are not thread-safe, so one thread opens
//! the document, owns it, and renders pages on request.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use noido_engine::{Document, EngineError, PdfEngine, PixelRect, Rgba8Image, Size};

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
}

impl Renderer {
    /// Starts the render thread and opens the document on it. Blocks until the
    /// document is open; `waker` is called after each finished render.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: Arc<[u8]>,
        waker: Box<dyn Fn() + Send>,
    ) -> Result<(Renderer, DocumentInfo), EngineError> {
        let (jobs, job_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        let (opened_tx, opened_rx) = mpsc::sync_channel(1);

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
                run(document.as_ref(), &job_rx, &result_tx, &*waker);
            })
            .map_err(|e| EngineError::Open(format!("cannot start render thread: {e}")))?;

        let info = opened_rx
            .recv()
            .map_err(|_| EngineError::Open("render thread stopped while opening".into()))??;
        Ok((Renderer { jobs, results }, info))
    }

    pub fn request(&self, req: RenderRequest) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.jobs.send(req);
    }

    pub fn try_recv(&self) -> Option<RenderedPage> {
        self.results.try_recv().ok()
    }
}

fn open(
    engine: &dyn PdfEngine,
    bytes: Arc<[u8]>,
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
    waker: &dyn Fn(),
) {
    while let Ok(first) = jobs.recv() {
        // Keep only the latest request per page, in order of first arrival.
        let mut batch = vec![first];
        for req in jobs.try_iter() {
            match batch.iter_mut().find(|r| r.page == req.page) {
                Some(existing) => existing.scale = req.scale,
                None => batch.push(req),
            }
        }
        for RenderRequest { page, scale } in batch {
            let result = document.page_size(page).and_then(|size| {
                document.render_tile(page, scale, PixelRect::full_page(size, scale))
            });
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
}
