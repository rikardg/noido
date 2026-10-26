//! The links thread: answers a page's links and where to flash a link's
//! target, on its own copy of the document so links never wait for
//! renders or searches.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use noido_engine::{Destination, Document, Link, PdfBytes, PdfEngine, Rect};
use noido_text::PageText;

/// The height, in points, of the bar flashed where a target has no line.
pub const FLASH_BAR_HEIGHT: f32 = 2.0;

#[derive(Debug)]
pub enum LinkReply {
    /// The links on `page`; none if they couldn't be read.
    Links { page: usize, links: Vec<Link> },
    /// Where to flash `target`: its line, else a bar at its height, else
    /// nothing.
    Flash {
        target: Destination,
        area: Option<Rect>,
    },
}

enum Command {
    Links(usize),
    Flash(Destination),
}

/// Handle to the links thread. Dropping it stops the thread.
pub struct Linker {
    commands: Sender<Command>,
    replies: Receiver<LinkReply>,
}

impl Linker {
    /// Starts the thread, which opens the document itself; returns at once.
    /// If the document fails to open, the thread stops and nothing arrives.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        waker: Box<dyn Fn() + Send>,
    ) -> Linker {
        let (commands, command_rx) = mpsc::channel();
        let (reply_tx, replies) = mpsc::channel();
        // Without a thread there are no links; pages still show.
        let _ = thread::Builder::new()
            .name("noido-links".into())
            .spawn(move || {
                if let Ok(document) = engine.open(bytes) {
                    run(document.as_ref(), &command_rx, &reply_tx, &*waker);
                }
            });
        Linker { commands, replies }
    }

    /// Asks for the links on `page`.
    pub fn request_links(&self, page: usize) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.commands.send(Command::Links(page));
    }

    /// Asks where to flash `target`.
    pub fn request_flash(&self, target: Destination) {
        let _ = self.commands.send(Command::Flash(target));
    }

    pub fn try_recv(&self) -> Option<LinkReply> {
        self.replies.try_recv().ok()
    }
}

fn run(
    document: &dyn Document,
    commands: &Receiver<Command>,
    replies: &Sender<LinkReply>,
    waker: &dyn Fn(),
) {
    for command in commands {
        let reply = match command {
            Command::Links(page) => LinkReply::Links {
                page,
                links: document.links(page).unwrap_or_default(),
            },
            Command::Flash(target) => LinkReply::Flash {
                target,
                area: flash_area(document, target),
            },
        };
        if replies.send(reply).is_err() {
            return;
        }
        waker();
    }
}

/// The line at `target`; without one, a bar across the text (or most of
/// the page) at its height; without a height either, nothing.
fn flash_area(document: &dyn Document, target: Destination) -> Option<Rect> {
    let text = PageText::new(document.text(target.page).unwrap_or_default());
    if let Some(line) = text.line_near(target.x, target.y) {
        return Some(line);
    }
    let y = target.y?;
    let (left, right) = match text.text_span() {
        Some(span) => span,
        None => {
            let width = document.page_size(target.page).ok()?.width;
            (width * 0.1, width * 0.9)
        }
    };
    Some(Rect {
        x: left,
        y: y - FLASH_BAR_HEIGHT / 2.0,
        width: right - left,
        height: FLASH_BAR_HEIGHT,
    })
}
