//! Search state: the text thread, the hits of the current query, and when
//! to search and where to jump.

use noido_core::Hits;
use noido_render::TextSearcher;

/// What to do with the first hit of a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum First {
    /// Go there: the reader just typed the query.
    Jump,
    /// Make it current without moving: the query was re-run on a reload.
    Select,
    /// Nothing: done, or the reader moved first.
    Done,
}

pub struct Search {
    searcher: Option<TextSearcher>,
    /// Whether extraction was asked for, so a reload asks the new thread too.
    extracting: bool,
    pub hits: Hits,
}

impl Search {
    pub fn new() -> Search {
        Search {
            searcher: None,
            extracting: false,
            hits: Hits::new(),
        }
    }

    /// Searches with `searcher` from now on, as after a reload.
    pub fn set_searcher(&mut self, searcher: TextSearcher) {
        if self.extracting {
            searcher.extract();
        }
        self.searcher = Some(searcher);
    }

    /// Starts extracting text, once.
    pub fn extract(&mut self) {
        if self.extracting {
            return;
        }
        self.extracting = true;
        if let Some(searcher) = &self.searcher {
            searcher.extract();
        }
    }
}
