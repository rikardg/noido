//! Highlights on the page: coloured character ranges, and the undo and
//! redo history of changes to them.

use std::cmp::Reverse;
use std::ops::{Range, RangeInclusive};

use noido_text::PageText;

use crate::selection::range_on_page;
use crate::{Caret, DocPoint};

/// How many changes [`Highlights::undo`] can step back through.
pub const UNDO_LIMIT: usize = 100;

/// The colours a highlight can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightColour {
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl HighlightColour {
    /// Every colour, in the order the bar offers them.
    pub const ALL: [HighlightColour; 5] = [
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Pink,
        Self::Purple,
    ];
}

/// A coloured run of characters, from `start` (included) to `end`
/// (excluded) in document order; `start < end` always holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub start: Caret,
    pub end: Caret,
    pub colour: HighlightColour,
}

impl Highlight {
    /// The highlighted characters on `page`, given its char count.
    pub fn on_page(&self, page: usize, char_count: usize) -> Option<Range<usize>> {
        range_on_page(self.start, self.end, page, char_count)
    }

    /// The pages from start to end.
    pub fn pages(&self) -> RangeInclusive<usize> {
        self.start.page..=self.end.page
    }
}

/// A document's highlights, with the snapshots its changes can be undone
/// and redone through: each change keeps the list as it was before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Highlights {
    items: Vec<Highlight>,
    undo: Vec<Vec<Highlight>>,
    redo: Vec<Vec<Highlight>>,
}

impl Highlights {
    /// The highlights in `items`, less any whose end is not after its
    /// start; no history to undo.
    pub fn from_items(items: Vec<Highlight>) -> Self {
        Self {
            items: items.into_iter().filter(|h| h.start < h.end).collect(),
            ..Self::default()
        }
    }

    pub fn items(&self) -> &[Highlight] {
        &self.items
    }

    pub fn get(&self, index: usize) -> Option<Highlight> {
        self.items.get(index).copied()
    }

    /// Adds `highlight` as one change, keeping its index; `None`, and no
    /// change, for an empty or backwards range.
    pub fn add(&mut self, highlight: Highlight) -> Option<usize> {
        if highlight.start >= highlight.end {
            return None;
        }
        let index = self.items.len();
        self.record();
        self.items.push(highlight);
        Some(index)
    }

    /// Gives highlight `index` the colour `colour`; `false`, and no
    /// change, when it is missing or already that colour.
    pub fn set_colour(&mut self, index: usize, colour: HighlightColour) -> bool {
        if self.get(index).is_none_or(|h| h.colour == colour) {
            return false;
        }
        self.record();
        self.items[index].colour = colour;
        true
    }

    /// Resizes highlight `index` to the range between `a` and `b`, in
    /// document order; `false`, and no change, when it is missing,
    /// already that range, or the range holds nothing.
    pub fn set_range(&mut self, index: usize, a: Caret, b: Caret) -> bool {
        let (start, end) = (a.min(b), a.max(b));
        let Some(current) = self.get(index) else {
            return false;
        };
        if start >= end || (current.start, current.end) == (start, end) {
            return false;
        }
        self.record();
        self.items[index].start = start;
        self.items[index].end = end;
        true
    }

    /// Removes highlight `index`, returning it; `None` when there is no
    /// such highlight.
    pub fn remove(&mut self, index: usize) -> Option<Highlight> {
        if index >= self.items.len() {
            return None;
        }
        self.record();
        Some(self.items.remove(index))
    }

    /// Steps back one change, keeping it for [`redo`](Self::redo);
    /// `false` with nothing left to undo.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.items, previous));
        true
    }

    /// Steps forward one undone change; `false` with nothing left to
    /// redo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.items, next));
        true
    }

    /// The index of the highlight covering the point `p` on its page: of
    /// those covering it, the one spanning fewest pages and then fewest
    /// characters, the newest of equals.
    pub fn at_point(&self, p: DocPoint, text: &PageText) -> Option<usize> {
        let covers = |h: &Highlight| {
            let Some(range) = h.on_page(p.page, text.char_count()) else {
                return false;
            };
            let inside = |r: &noido_text::Rect| {
                r.x <= p.x && p.x <= r.x + r.width && r.y <= p.y && p.y <= r.y + r.height
            };
            text.rects(range).iter().any(inside)
        };
        let span = |h: &Highlight| {
            (
                h.end.page - h.start.page,
                h.end.char.saturating_sub(h.start.char),
            )
        };
        self.items
            .iter()
            .enumerate()
            .filter(|(_, h)| covers(h))
            .min_by_key(|&(index, h)| (span(h), Reverse(index)))
            .map(|(index, _)| index)
    }

    /// Keeps the list as it is for undo, within [`UNDO_LIMIT`] steps, and
    /// drops what could have been redone.
    fn record(&mut self) {
        let items = self.items.clone();
        if self.undo.len() >= UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(items);
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noido_text::testing::page;

    fn at(page: usize, char: usize) -> Caret {
        Caret { page, char }
    }

    fn point(x: f32, y: f32) -> DocPoint {
        DocPoint { page: 0, x, y }
    }

    fn highlight(start: (usize, usize), end: (usize, usize), colour: HighlightColour) -> Highlight {
        Highlight {
            start: at(start.0, start.1),
            end: at(end.0, end.1),
            colour,
        }
    }

    #[test]
    fn add_and_on_page_across_pages() {
        let spanning = highlight((0, 4), (2, 3), HighlightColour::Yellow);
        assert_eq!(spanning.on_page(0, 11), Some(4..11));
        assert_eq!(spanning.on_page(1, 5), Some(0..5));
        assert_eq!(spanning.on_page(2, 5), Some(0..3));
        assert_eq!(spanning.on_page(3, 5), None);
        assert_eq!(spanning.pages(), 0..=2);

        let empty = highlight((0, 4), (0, 4), HighlightColour::Yellow);
        let backwards = highlight((2, 3), (0, 4), HighlightColour::Yellow);
        let mut highlights = Highlights::from_items(vec![spanning, empty, backwards]);
        assert_eq!(highlights.items(), &[spanning]);
        assert_eq!(highlights.add(empty), None);
        assert_eq!(highlights.add(backwards), None);
        assert_eq!(highlights.items(), &[spanning]);
    }

    #[test]
    fn set_colour_set_range_and_remove() {
        let yellow = HighlightColour::Yellow;
        let mut highlights = Highlights::from_items(vec![highlight((0, 2), (0, 5), yellow)]);

        assert!(highlights.set_colour(0, HighlightColour::Green));
        assert_eq!(highlights.get(0).unwrap().colour, HighlightColour::Green);
        assert!(!highlights.set_colour(0, HighlightColour::Green));

        assert!(highlights.set_range(0, at(0, 9), at(0, 2)));
        let green = highlight((0, 2), (0, 9), HighlightColour::Green);
        assert_eq!(highlights.get(0), Some(green));

        assert!(!highlights.set_range(0, at(0, 4), at(0, 4)));
        assert_eq!(highlights.get(0), Some(green));

        assert_eq!(highlights.remove(0), Some(green));
        assert!(highlights.items().is_empty());
        assert_eq!(highlights.remove(0), None);
    }

    #[test]
    fn undo_and_redo_lifecycle() {
        let mut highlights = Highlights::default();
        assert!(!highlights.undo());
        assert!(!highlights.redo());

        let added = highlight((0, 0), (0, 5), HighlightColour::Yellow);
        let blue = highlight((0, 0), (0, 5), HighlightColour::Blue);
        let resized = highlight((0, 1), (0, 3), HighlightColour::Blue);
        assert_eq!(highlights.add(added), Some(0));
        assert!(highlights.set_colour(0, HighlightColour::Blue));
        assert!(highlights.set_range(0, at(0, 1), at(0, 3)));
        assert_eq!(highlights.remove(0), Some(resized));
        assert!(highlights.items().is_empty());

        // Back through the remove, the resize, the recolour and the add.
        assert!(highlights.undo());
        assert_eq!(highlights.items(), &[resized]);
        assert!(highlights.undo());
        assert_eq!(highlights.items(), &[blue]);
        assert!(highlights.undo());
        assert_eq!(highlights.items(), &[added]);
        assert!(highlights.undo());
        assert!(highlights.items().is_empty());
        assert!(!highlights.undo());

        // And forward through them again.
        assert!(highlights.redo());
        assert_eq!(highlights.items(), &[added]);
        assert!(highlights.redo());
        assert_eq!(highlights.items(), &[blue]);
        assert!(highlights.redo());
        assert_eq!(highlights.items(), &[resized]);
        assert!(highlights.redo());
        assert!(highlights.items().is_empty());
        assert!(!highlights.redo());

        // A new change after an undo drops what could have been redone.
        assert!(highlights.undo());
        let pink = highlight((1, 0), (1, 2), HighlightColour::Pink);
        assert_eq!(highlights.add(pink), Some(1));
        assert!(!highlights.redo());
        assert_eq!(highlights.items(), &[resized, pink]);

        // Changes that change nothing record no step.
        let mut highlights = Highlights::from_items(vec![blue]);
        assert!(!highlights.set_colour(0, HighlightColour::Blue));
        assert!(!highlights.set_range(0, at(0, 0), at(0, 5)));
        assert!(!highlights.undo());
    }

    #[test]
    fn undo_limit_caps_history() {
        let mut highlights = Highlights::default();
        for char in 0..UNDO_LIMIT + 10 {
            let step = highlight((0, char), (0, char + 1), HighlightColour::Yellow);
            assert_eq!(highlights.add(step), Some(char));
        }
        let mut steps = 0;
        while highlights.undo() {
            steps += 1;
        }
        assert_eq!(steps, UNDO_LIMIT);
        assert_eq!(highlights.items().len(), 10);
    }

    #[test]
    fn at_point_picks_shorter_then_newer() {
        let text = PageText::new(page(&["the cat sat", "on the mat"]));
        let mut highlights = Highlights::default();
        let line = highlight((0, 0), (0, 11), HighlightColour::Yellow);
        let word = highlight((0, 4), (0, 7), HighlightColour::Green);
        assert_eq!(highlights.add(line), Some(0));
        assert_eq!(highlights.add(word), Some(1));

        assert_eq!(highlights.at_point(point(95.0, 95.0), &text), Some(1));
        assert_eq!(highlights.at_point(point(75.0, 95.0), &text), Some(0));
        assert_eq!(highlights.at_point(point(300.0, 400.0), &text), None);

        // Two equal ranges: the newer wins.
        let twins = Highlights::from_items(vec![line, word, word]);
        assert_eq!(twins.at_point(point(95.0, 95.0), &text), Some(2));
    }
}
