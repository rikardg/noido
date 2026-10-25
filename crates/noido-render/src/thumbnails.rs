//! The thumbnail thread: renders low-res pages around the reader's position,
//! on its own copy of the document so it never waits for full renders.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use noido_core::{Direction, THUMBNAIL_BUDGET, thumbnail_keep, thumbnail_order, thumbnail_scale};
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image};

pub struct Thumbnail {
    pub page: usize,
    /// The scale it was rendered at.
    pub scale: f32,
    pub result: Result<Rgba8Image, EngineError>,
}

/// Handle to the thumbnail thread. Dropping it stops the thread after its
/// current page.
pub struct Thumbnailer {
    focus: Sender<(usize, Direction)>,
    results: Receiver<Thumbnail>,
}

impl Thumbnailer {
    /// Starts the thread, which opens the document itself; returns at once.
    /// Nothing is rendered until [`Thumbnailer::set_focus`]. If the document
    /// fails to open, the thread stops and no thumbnails arrive.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        waker: Box<dyn Fn() + Send>,
    ) -> Thumbnailer {
        let (focus, focus_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        // Without a thread there are no thumbnails; pages fall back to white.
        let _ = thread::Builder::new()
            .name("noido-thumbnails".into())
            .spawn(move || {
                if let Ok(document) = engine.open(bytes) {
                    run(document.as_ref(), &focus_rx, &result_tx, &*waker);
                }
            });
        Thumbnailer { focus, results }
    }

    /// The page the reader is at and the direction they last moved in.
    pub fn set_focus(&self, page: usize, direction: Direction) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.focus.send((page, direction));
    }

    pub fn try_recv(&self) -> Option<Thumbnail> {
        self.results.try_recv().ok()
    }
}

fn run(
    document: &dyn Document,
    focus_rx: &Receiver<(usize, Direction)>,
    results: &Sender<Thumbnail>,
    waker: &dyn Fn(),
) {
    let count = document.page_count();
    let mut delivered = vec![false; count];
    let Ok(mut focus) = focus_rx.recv() else {
        return;
    };
    loop {
        let (page, direction) = focus;
        forget_outside(&mut delivered, page);

        let mut newer = None;
        for page in thumbnail_order(count, page, direction, THUMBNAIL_BUDGET) {
            // The frontend drops thumbnails for every focus it sends, so each
            // one, not just the newest, must make the thread forget them too.
            for focus in focus_rx.try_iter() {
                forget_outside(&mut delivered, focus.0);
                newer = Some(focus);
            }
            if newer.is_some() {
                break;
            }
            if delivered[page] {
                continue;
            }
            let (scale, result) = match document.page_size(page) {
                Ok(size) => {
                    let scale = thumbnail_scale(size);
                    let tile = PixelRect::full_page(size, scale);
                    (scale, document.render_tile(page, scale, tile))
                }
                Err(e) => (1.0, Err(e)),
            };
            if results
                .send(Thumbnail {
                    page,
                    scale,
                    result,
                })
                .is_err()
            {
                return;
            }
            delivered[page] = true;
            waker();
        }

        focus = match newer {
            Some(newer) => newer,
            None => match focus_rx.recv() {
                Ok(newer) => newer,
                Err(_) => return,
            },
        };
    }
}

/// Marks pages outside the thumbnail window around `focus` as not delivered,
/// matching the frontend dropping their thumbnails.
fn forget_outside(delivered: &mut [bool], focus: usize) {
    let keep = thumbnail_keep(delivered.len(), focus, THUMBNAIL_BUDGET);
    for (page, done) in delivered.iter_mut().enumerate() {
        if !keep.contains(&page) {
            *done = false;
        }
    }
}
