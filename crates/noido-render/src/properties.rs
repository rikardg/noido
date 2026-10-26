//! The properties thread: opens its own copy of the document, reads what
//! it says about itself and its fonts, sends that and stops. Started when
//! asked, so documents never inspected cost nothing, and renders never
//! wait for the walk over every page.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use noido_engine::{PdfBytes, PdfEngine, Properties};

/// Starts reading the document's properties; returns at once. One reply
/// arrives, then `waker` is called. A document that doesn't open has none;
/// without a thread, nothing arrives.
pub fn read_properties(
    engine: Arc<dyn PdfEngine>,
    bytes: PdfBytes,
    waker: Box<dyn Fn() + Send>,
) -> Receiver<Properties> {
    let (reply, replies) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("noido-properties".into())
        .spawn(move || {
            let properties = engine
                .open(bytes)
                .map(|document| document.properties())
                .unwrap_or_default();
            if reply.send(properties).is_ok() {
                waker();
            }
        });
    replies
}
