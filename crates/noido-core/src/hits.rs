//! Search hits, kept in document order while pages arrive in any order.

use noido_text::TextHit;

use crate::DocPoint;

/// A match and the page it's on.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub page: usize,
    pub text: TextHit,
}

impl Hit {
    /// Where it starts on its page, for comparing with a place in the
    /// document: its page and its first rectangle's top.
    fn position(&self) -> (usize, f32) {
        (self.page, self.text.rects.first().map_or(0.0, |r| r.y))
    }
}

/// The hits of a search, in document order, and which one is current.
#[derive(Debug, Default)]
pub struct Hits {
    hits: Vec<Hit>,
    current: Option<usize>,
}

impl Hits {
    pub fn new() -> Hits {
        Hits::default()
    }

    pub fn clear(&mut self) {
        self.hits.clear();
        self.current = None;
    }

    /// Adds `page`'s hits, in the page's text order. The current hit stays
    /// the same hit, though its index may change.
    pub fn add_page(&mut self, page: usize, hits: Vec<TextHit>) {
        if hits.is_empty() {
            return;
        }
        let at = self.hits.partition_point(|h| h.page < page);
        let added = hits.len();
        self.hits
            .splice(at..at, hits.into_iter().map(|text| Hit { page, text }));
        if let Some(current) = &mut self.current
            && *current >= at
        {
            *current += added;
        }
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Hit> {
        self.hits.get(index)
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn set_current(&mut self, current: Option<usize>) {
        self.current = current.filter(|&i| i < self.hits.len());
    }

    /// The hit after the current one, wrapping; the first without one.
    pub fn next(&self) -> Option<usize> {
        if self.hits.is_empty() {
            return None;
        }
        Some(self.current.map_or(0, |i| (i + 1) % self.hits.len()))
    }

    /// The hit before the current one, wrapping; the last without one.
    pub fn previous(&self) -> Option<usize> {
        let last = self.hits.len().checked_sub(1)?;
        Some(match self.current {
            Some(0) | None => last,
            Some(i) => i - 1,
        })
    }

    /// The first hit starting at or after `at`, else the first.
    pub fn first_at_or_after(&self, at: DocPoint) -> Option<usize> {
        if self.hits.is_empty() {
            return None;
        }
        let at = (at.page, at.y);
        Some(
            self.hits
                .iter()
                .position(|h| h.position() >= at)
                .unwrap_or(0),
        )
    }

    /// The last hit starting before `at`, else the last.
    pub fn last_before(&self, at: DocPoint) -> Option<usize> {
        let last = self.hits.len().checked_sub(1)?;
        let at = (at.page, at.y);
        Some(
            self.hits
                .iter()
                .rposition(|h| h.position() < at)
                .unwrap_or(last),
        )
    }

    /// The index of the first hit on `page`.
    pub fn first_on(&self, page: usize) -> Option<usize> {
        let index = self.hits.partition_point(|h| h.page < page);
        self.hits
            .get(index)
            .filter(|h| h.page == page)
            .map(|_| index)
    }

    /// The hits on `page`.
    pub fn on_page(&self, page: usize) -> &[Hit] {
        let start = self.hits.partition_point(|h| h.page < page);
        let end = self.hits.partition_point(|h| h.page <= page);
        &self.hits[start..end]
    }
}

#[cfg(test)]
mod tests {
    use noido_text::{Context, Rect, TextHit};

    use super::*;
    use crate::DocPoint;

    /// A hit at `start` in the page text whose one rectangle has top `y`.
    fn hit(start: usize, y: f32) -> TextHit {
        TextHit {
            range: start..start + 3,
            rects: vec![Rect {
                x: 72.0,
                y,
                width: 18.0,
                height: 12.0,
            }],
            context: Context {
                text: String::new(),
                bold: 0..0,
            },
        }
    }

    fn at(page: usize, y: f32) -> DocPoint {
        DocPoint { page, x: 0.0, y }
    }

    fn pages(hits: &Hits) -> Vec<usize> {
        (0..hits.len()).map(|i| hits.get(i).unwrap().page).collect()
    }

    /// Hits on pages 1 (two), 3 and 5.
    fn sample() -> Hits {
        let mut hits = Hits::new();
        hits.add_page(1, vec![hit(0, 100.0), hit(50, 300.0)]);
        hits.add_page(3, vec![hit(10, 200.0)]);
        hits.add_page(5, vec![hit(0, 400.0)]);
        hits
    }

    #[test]
    fn pages_arrive_out_of_order_kept_sorted() {
        let mut hits = Hits::new();
        for page in [3, 4, 0, 1] {
            hits.add_page(page, vec![hit(0, 100.0)]);
        }
        assert_eq!(pages(&hits), [0, 1, 3, 4]);
    }

    #[test]
    fn hits_in_a_page_keep_text_order() {
        let mut hits = Hits::new();
        hits.add_page(2, vec![hit(5, 100.0), hit(40, 50.0)]);
        assert_eq!(hits.get(0).unwrap().text.range.start, 5);
        assert_eq!(hits.get(1).unwrap().text.range.start, 40);
    }

    #[test]
    fn insert_before_current_keeps_current() {
        let mut hits = Hits::new();
        hits.add_page(3, vec![hit(0, 100.0)]);
        hits.set_current(Some(0));
        hits.add_page(0, vec![hit(0, 100.0), hit(9, 200.0)]);
        assert_eq!(hits.current(), Some(2));
        assert_eq!(hits.get(2).unwrap().page, 3);
        hits.add_page(4, vec![hit(0, 100.0)]);
        assert_eq!(hits.current(), Some(2));
    }

    #[test]
    fn next_and_previous_wrap() {
        let mut hits = sample();
        hits.set_current(Some(3));
        assert_eq!(hits.next(), Some(0));
        hits.set_current(Some(0));
        assert_eq!(hits.next(), Some(1));
        assert_eq!(hits.previous(), Some(3));
    }

    #[test]
    fn next_without_current_is_first() {
        let hits = sample();
        assert_eq!(hits.next(), Some(0));
        assert_eq!(hits.previous(), Some(3));
    }

    #[test]
    fn first_at_or_after_finds_and_wraps() {
        let hits = sample();
        assert_eq!(hits.first_at_or_after(at(1, 100.0)), Some(0));
        assert_eq!(hits.first_at_or_after(at(1, 150.0)), Some(1));
        assert_eq!(hits.first_at_or_after(at(2, 0.0)), Some(2));
        assert_eq!(hits.first_at_or_after(at(5, 500.0)), Some(0));
    }

    #[test]
    fn last_before_finds_and_wraps() {
        let hits = sample();
        assert_eq!(hits.last_before(at(3, 300.0)), Some(2));
        assert_eq!(hits.last_before(at(3, 200.0)), Some(1));
        assert_eq!(hits.last_before(at(1, 50.0)), Some(3));
    }

    #[test]
    fn on_page_slices() {
        let hits = sample();
        assert_eq!(hits.on_page(1).len(), 2);
        assert_eq!(hits.on_page(3)[0].text.range.start, 10);
        assert!(hits.on_page(2).is_empty());
        assert!(hits.on_page(9).is_empty());
    }

    #[test]
    fn first_on_finds_page_start() {
        let hits = sample();
        assert_eq!(hits.first_on(1), Some(0));
        assert_eq!(hits.first_on(3), Some(2));
        assert_eq!(hits.first_on(2), None);
        assert_eq!(hits.first_on(9), None);
    }

    #[test]
    fn clear_forgets_current() {
        let mut hits = sample();
        hits.set_current(Some(1));
        hits.clear();
        assert!(hits.is_empty());
        assert_eq!(hits.current(), None);
    }

    #[test]
    fn empty_is_safe() {
        let hits = Hits::new();
        assert_eq!(hits.next(), None);
        assert_eq!(hits.previous(), None);
        assert_eq!(hits.first_at_or_after(at(0, 0.0)), None);
        assert_eq!(hits.last_before(at(0, 0.0)), None);
        assert!(hits.get(0).is_none());
    }
}
