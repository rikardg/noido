//! The pages' text, as the text thread hands it over: asked for once per
//! page when it shows, kept while the reader can reach it.

use std::ops::Range;
use std::sync::Arc;

use noido_core::PageTextState;
use noido_text::PageText;

/// How many bytes of page text are kept. A page of prose is about 150 kB, so
/// this is some two hundred pages, or fewer with their folded forms and a
/// selection across them: more reach than a reader has, and a bound on the
/// document rather than a working set.
pub const TEXT_BUDGET: usize = 32 << 20;

enum Slot {
    Unasked,
    Asked,
    Known(Arc<PageText>),
}

pub struct Texts {
    pages: Vec<Slot>,
    /// When each page was last asked for, counting asks.
    touched: Vec<u64>,
    asks: u64,
    budget: usize,
}

impl Texts {
    pub fn new(page_count: usize) -> Texts {
        Texts {
            pages: (0..page_count).map(|_| Slot::Unasked).collect(),
            touched: vec![0; page_count],
            asks: 0,
            budget: TEXT_BUDGET,
        }
    }

    /// Keeps at most `bytes` of page text, for a test that wants a bound
    /// small enough to see.
    #[cfg(test)]
    pub fn with_budget(mut self, bytes: usize) -> Texts {
        self.budget = bytes;
        self
    }

    /// Forgets every page's text: a new version has its own, and the bound
    /// stays as it was.
    pub fn reset(&mut self, page_count: usize) {
        let budget = self.budget;
        *self = Texts::new(page_count);
        self.budget = budget;
    }

    /// Drops the pages asked for longest ago until what is kept fits the
    /// budget, except the pages in `in_view` — drawn this frame — and those
    /// `pinned` names, which something needs whole: the pages of a copy
    /// being made, of a highlight under edit, of a selection's ends. Returns
    /// the pages it let go, for the thread to forget too.
    ///
    /// A page is counted by what it holds now, so the folded forms a search
    /// builds over it are seen. A copy can need more pages than the budget
    /// holds, and then the budget gives way: it is a bound on what is kept
    /// for later, not on what is being used now.
    pub fn evict(&mut self, in_view: Range<usize>, pinned: impl Fn(usize) -> bool) -> Vec<usize> {
        let mut total: usize = self
            .pages
            .iter()
            .filter_map(|slot| match slot {
                Slot::Known(text) => Some(text.memory_bytes()),
                _ => None,
            })
            .sum();
        let mut dropped = Vec::new();
        while total > self.budget {
            // The least recently asked for page that may go; the one asked
            // for last is the one on screen.
            let oldest = (0..self.pages.len())
                .filter(|&page| !in_view.contains(&page) && !pinned(page))
                .filter(|&page| matches!(self.pages[page], Slot::Known(_)))
                .min_by_key(|&page| self.touched[page]);
            let Some(page) = oldest else { break };
            if let Slot::Known(text) = &self.pages[page] {
                total -= text.memory_bytes();
            }
            self.pages[page] = Slot::Unasked;
            dropped.push(page);
        }
        dropped
    }

    /// Asks for `page`'s text with `request` unless asked already, and notes
    /// that the reader can reach it: what was wanted least recently goes
    /// when the budget is over.
    pub fn want(&mut self, page: usize, request: impl FnOnce(usize)) {
        let Some(slot) = self.pages.get(page) else {
            return;
        };
        let unasked = matches!(slot, Slot::Unasked);
        self.touch(page);
        if unasked && let Some(slot) = self.pages.get_mut(page) {
            request(page);
            *slot = Slot::Asked;
        }
    }

    /// Notes that `page` was just asked for, made or answered.
    fn touch(&mut self, page: usize) {
        self.asks += 1;
        if let Some(when) = self.touched.get_mut(page) {
            *when = self.asks;
        }
    }

    /// Forgets `page`'s text, so it is asked for again: the page turned.
    pub fn forget(&mut self, page: usize) {
        if let Some(slot) = self.pages.get_mut(page) {
            *slot = Slot::Unasked;
        }
    }

    pub fn insert(&mut self, page: usize, text: Arc<PageText>) {
        if let Some(slot) = self.pages.get_mut(page) {
            *slot = Slot::Known(text);
            self.touch(page);
        }
    }

    /// The bytes of page text held now.
    #[cfg(test)]
    pub fn retained_bytes(&self) -> usize {
        self.pages
            .iter()
            .filter_map(|slot| match slot {
                Slot::Known(text) => Some(text.memory_bytes()),
                _ => None,
            })
            .sum()
    }

    pub fn get(&self, page: usize) -> PageTextState<'_> {
        match self.pages.get(page) {
            Some(Slot::Known(text)) => PageTextState::Known(text),
            _ => PageTextState::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noido_text::testing::page;

    fn text(lines: &[&str]) -> Arc<PageText> {
        Arc::new(PageText::new(page(lines)))
    }

    /// A text of `Texts` that was asked for and arrived.
    fn shown(texts: &mut Texts, page: usize) {
        texts.want(page, |_| {});
        texts.insert(page, text(&["a page's worth of words"]));
    }

    fn known(texts: &Texts, page: usize) -> bool {
        matches!(texts.get(page), PageTextState::Known(_))
    }

    #[test]
    fn a_page_out_of_reach_is_let_go() {
        let one = text(&["a page's worth of words"]).memory_bytes();
        let mut texts = Texts::new(8).with_budget(2 * one);
        for page in 0..3 {
            shown(&mut texts, page);
        }
        let dropped = texts.evict(3..4, |_| false);
        assert_eq!(dropped, [0], "dropped {dropped:?}");
        assert!(!known(&texts, 0));
        assert!(known(&texts, 2));
    }

    #[test]
    fn the_pages_on_screen_stay() {
        let mut texts = Texts::new(4).with_budget(1);
        for page in 0..3 {
            shown(&mut texts, page);
        }
        let dropped = texts.evict(1..2, |_| false);
        assert_eq!(dropped, [0, 2], "dropped {dropped:?}");
        assert!(known(&texts, 1), "the page at the view went too");
    }

    #[test]
    fn the_page_asked_for_longest_ago_goes_first() {
        let one = text(&["a page's worth of words"]).memory_bytes();
        let mut texts = Texts::new(8).with_budget(2 * one);
        for page in 0..3 {
            shown(&mut texts, page);
        }
        // Looked at again, so page 0 is more recent than page 1.
        texts.want(0, |_| {});
        let dropped = texts.evict(3..4, |_| false);
        assert_eq!(dropped, [1], "dropped {dropped:?}");
        assert!(known(&texts, 0));
    }

    #[test]
    fn a_pinned_page_stays_over_the_budget() {
        let mut texts = Texts::new(4).with_budget(1);
        shown(&mut texts, 0);
        shown(&mut texts, 1);
        // A copy needs page 0 whole; only page 1 may go.
        let dropped = texts.evict(3..4, |page| page == 0);
        assert_eq!(dropped, [1], "dropped {dropped:?}");
        assert!(known(&texts, 0), "a page a copy needs went");
    }

    #[test]
    fn everything_pinned_lets_nothing_go() {
        let mut texts = Texts::new(4).with_budget(1);
        shown(&mut texts, 0);
        shown(&mut texts, 1);
        assert_eq!(texts.evict(3..4, |_| true), []);
        assert!(known(&texts, 0) && known(&texts, 1));
    }

    #[test]
    fn a_page_that_left_is_asked_for_again() {
        let mut texts = Texts::new(4).with_budget(1);
        shown(&mut texts, 0);
        shown(&mut texts, 1);
        assert_eq!(texts.evict(3..4, |_| false).len(), 2);
        let mut asked = Vec::new();
        texts.want(0, |page| asked.push(page));
        assert_eq!(asked, [0], "a page that was let go was not asked for");
    }
}
