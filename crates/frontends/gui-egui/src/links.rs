//! The links on the pages, as the links thread reads them: asked for once
//! per page when it shows, kept for the whole document (they are small).

use noido_engine::{Destination, Link, Rect};
use noido_render::{LinkReply, Linker};

enum PageLinks {
    Unasked,
    Asked,
    Known(Vec<Link>),
}

/// How long a target flashes, in seconds.
pub const FLASH_SECONDS: f64 = 1.0;

/// The answer to [`Links::resolve`].
#[derive(Debug, PartialEq)]
pub enum Named {
    Found(Destination),
    Unknown(String),
}

pub struct Links {
    linker: Option<Linker>,
    pages: Vec<PageLinks>,
    /// The target whose flash was asked for last.
    pending: Option<Destination>,
    /// The flash showing: its page, area and start time.
    flash: Option<(usize, Rect, f64)>,
    /// The named place whose answer is wanted.
    named: Option<String>,
}

impl Links {
    pub fn new(page_count: usize) -> Links {
        Links {
            linker: None,
            pages: unasked(page_count),
            pending: None,
            flash: None,
            named: None,
        }
    }

    /// Reads links with `linker` from now on, forgetting those known: a
    /// new version of the document has its own.
    pub fn set_linker(&mut self, linker: Linker, page_count: usize) {
        *self = Links::new(page_count);
        self.linker = Some(linker);
    }

    /// Asks for `page`'s links unless they were asked for already.
    pub fn want(&mut self, page: usize) {
        let (Some(linker), Some(state)) = (&self.linker, self.pages.get_mut(page)) else {
            return;
        };
        if matches!(state, PageLinks::Unasked) {
            linker.request_links(page);
            *state = PageLinks::Asked;
        }
    }

    /// Flashes `target` once the links thread says where, replacing any
    /// flash showing or asked for.
    pub fn start_flash(&mut self, target: Destination) {
        self.flash = None;
        if let Some(linker) = &self.linker {
            linker.request_flash(target);
            self.pending = Some(target);
        }
    }

    /// The flash showing at `now`: its page, area and opacity (1 fading to 0).
    pub fn flash(&self, now: f64) -> Option<(usize, Rect, f32)> {
        let (page, area, start) = self.flash?;
        let elapsed = now - start;
        (elapsed < FLASH_SECONDS).then(|| (page, area, (1.0 - elapsed / FLASH_SECONDS) as f32))
    }

    /// Takes in what the links thread sent; a flash starts at `now`. Gives
    /// the answer for the name asked for, once it comes.
    pub fn receive(&mut self, now: f64) -> Option<Named> {
        let Some(linker) = &self.linker else {
            return None;
        };
        let mut answer = None;
        while let Some(reply) = linker.try_recv() {
            match reply {
                LinkReply::Links { page, links } => {
                    if let Some(state) = self.pages.get_mut(page) {
                        *state = PageLinks::Known(links);
                    }
                }
                LinkReply::Flash { target, area } => {
                    if self.pending == Some(target) {
                        self.pending = None;
                        self.flash = area.map(|area| (target.page, area, now));
                    }
                }
                LinkReply::Named { name, target } => {
                    if self.named.as_ref() == Some(&name) {
                        self.named = None;
                        answer = Some(match target {
                            Some(target) => Named::Found(target),
                            None => Named::Unknown(name),
                        });
                    }
                }
            }
        }
        answer
    }

    /// Asks where the place named `name` is; [`Links::receive`] gives the
    /// answer. Replaces any name asked for before.
    pub fn resolve(&mut self, name: String) {
        if let Some(linker) = &self.linker {
            linker.request_named(name.clone());
            self.named = Some(name);
        }
    }

    /// Forgets the name asked for: its answer is no longer wanted.
    pub fn cancel_named(&mut self) {
        self.named = None;
    }

    /// The topmost link at page point (`x`, `y`) on `page`: the last one,
    /// in the document's order, with an area holding the point.
    pub fn at(&self, page: usize, x: f32, y: f32) -> Option<&Link> {
        let Some(PageLinks::Known(links)) = self.pages.get(page) else {
            return None;
        };
        links.iter().rev().find(|link| {
            link.areas
                .iter()
                .any(|a| (a.x..=a.x + a.width).contains(&x) && (a.y..=a.y + a.height).contains(&y))
        })
    }
}

fn unasked(page_count: usize) -> Vec<PageLinks> {
    (0..page_count).map(|_| PageLinks::Unasked).collect()
}

/// Whether a web link's `uri` may be opened: web and mail addresses only.
pub fn opens(uri: &str) -> bool {
    uri.split_once(':').is_some_and(|(scheme, _)| {
        ["http", "https", "mailto"]
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};

    use super::*;

    /// Two pages; the place named "a" is on page 1, "b" on page 0.
    #[derive(Clone)]
    struct Places;

    impl PdfEngine for Places {
        fn open(&self, _: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(Places))
        }
    }

    impl Document for Places {
        fn page_count(&self) -> usize {
            2
        }
        fn page_size(&self, _: usize) -> Result<Size, EngineError> {
            Ok(Size {
                width: 612.0,
                height: 792.0,
            })
        }
        fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
            Err(EngineError::Render("not drawn".into()))
        }
        fn named_destination(&self, name: &str) -> Option<Destination> {
            let page = match name {
                "a" => 1,
                "b" => 0,
                _ => return None,
            };
            Some(Destination {
                page,
                x: None,
                y: None,
            })
        }
    }

    fn links() -> Links {
        let mut links = Links::new(2);
        let linker = Linker::start(Arc::new(Places), Arc::new(Vec::new()), Box::new(|| {}));
        links.set_linker(linker, 2);
        links
    }

    /// Takes in replies until one gives an answer, or a second has passed.
    fn answer(links: &mut Links) -> Option<Named> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            if let Some(named) = links.receive(0.0) {
                return Some(named);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    fn found(page: usize) -> Option<Named> {
        Some(Named::Found(Destination {
            page,
            x: None,
            y: None,
        }))
    }

    #[test]
    fn a_named_place_is_found() {
        let mut links = links();
        links.resolve("a".into());
        assert_eq!(answer(&mut links), found(1));
    }

    #[test]
    fn an_unknown_name_says_so() {
        let mut links = links();
        links.resolve("nope".into());
        assert_eq!(answer(&mut links), Some(Named::Unknown("nope".into())));
    }

    #[test]
    fn only_the_last_name_is_answered() {
        let mut links = links();
        links.resolve("a".into());
        links.resolve("b".into());
        assert_eq!(answer(&mut links), found(0));
        assert_eq!(answer(&mut links), None);
    }

    #[test]
    fn a_cancelled_name_is_not_answered() {
        let mut links = links();
        links.resolve("a".into());
        links.cancel_named();
        assert_eq!(answer(&mut links), None);
    }
}
