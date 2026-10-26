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

pub struct Links {
    linker: Option<Linker>,
    pages: Vec<PageLinks>,
    /// The target whose flash was asked for last.
    pending: Option<Destination>,
    /// The flash showing: its page, area and start time.
    flash: Option<(usize, Rect, f64)>,
}

impl Links {
    pub fn new(page_count: usize) -> Links {
        Links {
            linker: None,
            pages: unasked(page_count),
            pending: None,
            flash: None,
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

    /// Takes in what the links thread sent; a flash starts at `now`.
    pub fn receive(&mut self, now: f64) {
        let Some(linker) = &self.linker else {
            return;
        };
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
            }
        }
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
