//! The pages' text, as the text thread hands it over: asked for once per
//! page when it shows, kept for the document.

use std::sync::Arc;

use noido_core::PageTextState;
use noido_text::PageText;

enum Slot {
    Unasked,
    Asked,
    Known(Arc<PageText>),
}

pub struct Texts {
    pages: Vec<Slot>,
}

impl Texts {
    pub fn new(page_count: usize) -> Texts {
        Texts {
            pages: (0..page_count).map(|_| Slot::Unasked).collect(),
        }
    }

    /// Forgets every page's text: a new version has its own.
    pub fn reset(&mut self, page_count: usize) {
        *self = Texts::new(page_count);
    }

    /// Asks for `page`'s text with `request` unless asked already.
    pub fn want(&mut self, page: usize, request: impl FnOnce(usize)) {
        if let Some(slot) = self.pages.get_mut(page)
            && matches!(slot, Slot::Unasked)
        {
            request(page);
            *slot = Slot::Asked;
        }
    }

    pub fn insert(&mut self, page: usize, text: Arc<PageText>) {
        if let Some(slot) = self.pages.get_mut(page) {
            *slot = Slot::Known(text);
        }
    }

    pub fn get(&self, page: usize) -> PageTextState<'_> {
        match self.pages.get(page) {
            Some(Slot::Known(text)) => PageTextState::Known(text),
            _ => PageTextState::Unknown,
        }
    }
}
