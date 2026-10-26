//! The links on the pages, as the links thread reads them: asked for once
//! per page when it shows, kept for the whole document (they are small).

use noido_engine::Link;
use noido_render::{LinkReply, Linker};

enum PageLinks {
    Unasked,
    Asked,
    Known(Vec<Link>),
}

pub struct Links {
    linker: Option<Linker>,
    pages: Vec<PageLinks>,
}

impl Links {
    pub fn new(page_count: usize) -> Links {
        Links {
            linker: None,
            pages: unasked(page_count),
        }
    }

    /// Reads links with `linker` from now on, forgetting those known: a
    /// new version of the document has its own.
    pub fn set_linker(&mut self, linker: Linker, page_count: usize) {
        self.linker = Some(linker);
        self.pages = unasked(page_count);
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

    /// Takes in what the links thread sent.
    pub fn receive(&mut self) {
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
                LinkReply::Flash { .. } => {}
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
