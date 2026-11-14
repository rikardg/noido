//! The thumbnail thread: renders low-res pages around the reader's position,
//! on its own copy of the document so it never waits for full renders.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use noido_core::PageTransform;
use noido_core::{Direction, THUMBNAIL_BUDGET, thumbnail_scale, window_keep, window_order};
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, Rgba8Image};

use crate::{Transformed, Transforms, open_transformed};

pub struct Thumbnail {
    pub page: usize,
    /// The scale it was rendered at.
    pub scale: f32,
    /// The page's transform when it was rendered.
    pub transform: PageTransform,
    pub result: Result<Rgba8Image, EngineError>,
}

/// Handle to the thumbnail thread. Dropping it stops the thread after its
/// current page.
pub struct Thumbnailer {
    focus: Sender<Message>,
    results: Receiver<Thumbnail>,
}

impl Thumbnailer {
    /// Starts the thread, which opens the document itself; returns at once.
    /// Nothing is rendered until [`Thumbnailer::set_focus`]. If the document
    /// fails to open, the thread stops and no thumbnails arrive.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        transforms: Transforms,
        waker: Box<dyn Fn() + Send>,
    ) -> Thumbnailer {
        let (focus, focus_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        // Without a thread there are no thumbnails; pages fall back to white.
        let _ = thread::Builder::new()
            .name("noido-thumbnails".into())
            .spawn(move || {
                if let Ok(document) = open_transformed(engine.as_ref(), bytes, &transforms) {
                    run(&document, &focus_rx, &result_tx, &*waker);
                }
            });
        Thumbnailer { focus, results }
    }

    /// The page the reader is at and the direction they last moved in.
    pub fn set_focus(&self, page: usize, direction: Direction) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.focus.send(Message::Focus(page, direction));
    }

    /// Renders `page` again, if it is near the focus: after it was turned.
    pub fn refresh(&self, page: usize) {
        let _ = self.focus.send(Message::Refresh(page));
    }

    pub fn try_recv(&self) -> Option<Thumbnail> {
        self.results.try_recv().ok()
    }
}

/// What the thumbnail thread is told.
enum Message {
    /// The page the reader is at and the direction they last moved in.
    Focus(usize, Direction),
    /// Render this page again.
    Refresh(usize),
}

fn run(
    document: &Transformed,
    messages: &Receiver<Message>,
    results: &Sender<Thumbnail>,
    waker: &dyn Fn(),
) {
    let count = document.page_count();
    let mut delivered = vec![false; count];
    // Waits for the first focus; refreshes before it change nothing.
    let mut focus = loop {
        match messages.recv() {
            Ok(Message::Focus(page, direction)) => break (page, direction),
            Ok(Message::Refresh(_)) => {}
            Err(_) => return,
        }
    };
    loop {
        let (page, direction) = focus;
        forget_outside(&mut delivered, page);

        let mut newer = None;
        for page in window_order(count, page, direction, THUMBNAIL_BUDGET) {
            // The frontend drops thumbnails for every focus it sends, so each
            // one, not just the newest, must make the thread forget them too.
            // A refresh starts the window over, so the page is drawn again.
            for message in messages.try_iter() {
                match message {
                    Message::Focus(page, direction) => {
                        forget_outside(&mut delivered, page);
                        newer = Some((page, direction));
                    }
                    Message::Refresh(page) => {
                        if let Some(done) = delivered.get_mut(page) {
                            *done = false;
                        }
                        newer = newer.or(Some(focus));
                    }
                }
            }
            if newer.is_some() {
                break;
            }
            if delivered[page] {
                continue;
            }
            // Read before the render, so a turn under way stamps it old.
            let transform = document.transform(page);
            let (scale, result) = crate::caught(|| {
                let size = document.page_size(page)?;
                let scale = thumbnail_scale(size);
                Ok((scale, crate::render_whole(document, page, size, scale)))
            })
            .unwrap_or_else(|e| (1.0, Err(e)));
            if results
                .send(Thumbnail {
                    page,
                    scale,
                    transform,
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
            None => match messages.recv() {
                Ok(Message::Focus(page, direction)) => (page, direction),
                Ok(Message::Refresh(page)) => {
                    if let Some(done) = delivered.get_mut(page) {
                        *done = false;
                    }
                    focus
                }
                Err(_) => return,
            },
        };
    }
}

/// Marks pages outside the thumbnail window around `focus` as not delivered,
/// matching the frontend dropping their thumbnails.
fn forget_outside(delivered: &mut [bool], focus: usize) {
    let keep = window_keep(delivered.len(), focus, THUMBNAIL_BUDGET);
    for (page, done) in delivered.iter_mut().enumerate() {
        if !keep.contains(&page) {
            *done = false;
        }
    }
}
