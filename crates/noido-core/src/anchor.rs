//! Finding a highlight's text again by its quote, after the document
//! changed. `check` and `Walk` are pure and work on a few pages at a time,
//! so the text thread can drive them; `anchor` drives them over pages
//! already in memory and is the reference they must agree with.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

use noido_text::{
    Joined, PageText, Query, Span, context_full, folded_prefix, folded_suffix, same_text,
};

use crate::{Highlight, Location, Quote};

impl Highlight {
    /// The pages a check reads: from the page before its start to the page
    /// after its end, clamped to the document. `None` when its start is
    /// past the last page.
    pub fn window(&self, page_count: usize) -> Option<RangeInclusive<usize>> {
        if self.start.part >= page_count {
            return None;
        }
        Some(self.start.part.saturating_sub(1)..=(self.end.part + 1).min(page_count - 1))
    }

    fn span(&self) -> Span {
        Span {
            start: (self.start.part, self.start.char),
            end: (self.end.part, self.end.char),
        }
    }
}

/// What a check made of a highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// It is where it was (with a quote filled in, if asked).
    Keep(Highlight),
    /// Its text has to be looked for.
    Search,
}

/// Whether `h` still has its text where it was, read through `window`,
/// which holds the pages of `h.window(..)`. A highlight without a quote
/// has nothing to compare and is kept, taking a quote from the window when
/// `fill_missing`.
pub fn check(h: &Highlight, window: &Joined, fill_missing: bool) -> Check {
    let span = h.span();
    let text = window.text_at(span);
    let mut kept = h.clone();
    match &h.quote {
        None => {
            if fill_missing && text.is_some() {
                let (exact, prefix, suffix) = window.quote(span);
                kept.quote = Some(Quote {
                    exact,
                    prefix,
                    suffix,
                });
            }
        }
        Some(q) => {
            let same = text.is_some_and(|t| same_text(&t, &q.exact));
            let full = context_full(&q.prefix, &q.suffix);
            if !(same && window.context(span, &q.prefix, &q.suffix) == full) {
                return Check::Search;
            }
            kept.orphaned = false;
        }
    }
    Check::Keep(kept)
}

/// A place the quote was found, and how well its neighbours agree.
#[derive(Clone, Copy)]
struct Candidate {
    span: Span,
    context: usize,
    /// One side's neighbours agree in full (or the quote has none), so the
    /// highlight may move here.
    sure: bool,
}

struct Pending {
    index: usize,
    highlight: Highlight,
    /// Into `Walk::queries`; `None` for a quote with nothing to search for.
    query: Option<usize>,
    /// The quote's prefix and suffix, folded once for scoring.
    prefix: Vec<char>,
    suffix: Vec<char>,
    /// The candidates with the best context so far.
    best: Vec<Candidate>,
}

/// The candidates for the highlights that need searching, collected page
/// by page.
pub struct Walk {
    pending: Vec<Pending>,
    /// One per distinct `exact`.
    queries: Vec<Query>,
}

impl Walk {
    /// `pending` holds each highlight with its index in the list.
    pub fn new(pending: Vec<(usize, Highlight)>) -> Walk {
        let mut queries = Vec::new();
        let mut known: HashMap<String, Option<usize>> = HashMap::new();
        let pending = pending
            .into_iter()
            .map(|(index, highlight)| {
                let query = highlight.quote.as_ref().and_then(|q| {
                    *known.entry(q.exact.clone()).or_insert_with(|| {
                        Query::new(&q.exact).map(|query| {
                            queries.push(query);
                            queries.len() - 1
                        })
                    })
                });
                let (prefix, suffix) =
                    highlight.quote.as_ref().map_or_else(Default::default, |q| {
                        (folded_prefix(&q.prefix), folded_suffix(&q.suffix))
                    });
                Pending {
                    index,
                    highlight,
                    query,
                    prefix,
                    suffix,
                    best: Vec::new(),
                }
            })
            .collect();
        Walk { pending, queries }
    }

    /// How many pages the window must hold past the page being walked: the
    /// pages the widest old range runs over, so a quote that crosses pages
    /// and its suffix fit.
    pub fn span(&self) -> usize {
        self.pending
            .iter()
            .map(|p| p.highlight.end.part.saturating_sub(p.highlight.start.part) + 1)
            .max()
            .unwrap_or(1)
    }

    /// The pages [`Walk::page`] expects for `page` in a document of
    /// `page_count` pages: `page - 1` through `page + span()`, clamped.
    /// This is the contract; drivers take their window from here.
    pub fn window(&self, page: usize, page_count: usize) -> RangeInclusive<usize> {
        page.saturating_sub(1)..=(page + self.span()).min(page_count - 1)
    }

    /// Records the matches that start on `page`, with their context. The
    /// window holds `page - 1` through `page + span()`.
    pub fn page(&mut self, page: usize, window: &Joined) {
        let found: Vec<Vec<Span>> = self
            .queries
            .iter()
            .map(|query| {
                window
                    .find(query)
                    .into_iter()
                    .filter(|s| s.start.0 == page)
                    .collect()
            })
            .collect();
        for p in &mut self.pending {
            let Some(query) = p.query else {
                continue;
            };
            for &span in &found[query] {
                let (back, forward) = window.agreement_folded(span, &p.prefix, &p.suffix);
                let context = back + forward;
                let full = |side: &[char], n: usize| !side.is_empty() && n == side.len();
                let sure = (p.prefix.is_empty() && p.suffix.is_empty())
                    || full(&p.prefix, back)
                    || full(&p.suffix, forward);
                if p.best.first().is_some_and(|b| context < b.context) {
                    continue;
                }
                if p.best.first().is_some_and(|b| context > b.context) {
                    p.best.clear();
                }
                p.best.push(Candidate {
                    span,
                    context,
                    sure,
                });
            }
        }
    }

    /// Each pending highlight at its best candidate: the highest context,
    /// ties to the nearest its old start, measured in characters through
    /// `char_counts`. It moves there, un-orphaned, only if the candidate is
    /// sure and no other highlight takes the span: one in `items` that
    /// isn't pending and isn't orphaned, or a pending one whose candidate
    /// there has more context (then is nearer); a twin, on the same old
    /// span with the same quote, moves with it. Otherwise it is orphaned
    /// where it was, keeping its quote. `items` is the whole list, by the
    /// indices the walk was given.
    pub fn finish(self, char_counts: &[usize], items: &[Highlight]) -> Vec<(usize, Highlight)> {
        let mut before = Vec::with_capacity(char_counts.len());
        let mut total = 0;
        for &count in char_counts {
            before.push(total);
            total += count;
        }
        // A position past the document's end counts as its end.
        let global = |(page, char): (usize, usize)| match before.get(page) {
            Some(&b) => b + char,
            None => total,
        };
        let pending: HashSet<usize> = self.pending.iter().map(|p| p.index).collect();
        let held: Vec<Span> = items
            .iter()
            .enumerate()
            .filter(|(i, h)| !pending.contains(i) && !h.orphaned)
            .map(|(_, h)| h.span())
            .collect();
        // Each pending highlight's best candidate, if it is sure and free,
        // with its distance from the old start.
        let chosen: Vec<Option<(Candidate, usize)>> = self
            .pending
            .iter()
            .map(|p| {
                let old = global((p.highlight.start.part, p.highlight.start.char));
                let distance = |c: &Candidate| global(c.span.start).abs_diff(old);
                p.best
                    .iter()
                    .min_by_key(|c| {
                        (
                            Reverse(c.context),
                            Reverse(c.sure),
                            distance(c),
                            c.span.start,
                        )
                    })
                    .filter(|c| {
                        // Inside, as a kept highlight may hold its edge spaces too.
                        let inside = |h: &Span| h.start <= c.span.start && c.span.end <= h.end;
                        c.sure && !held.iter().any(inside)
                    })
                    .map(|c| (*c, distance(c)))
            })
            .collect();
        // Where several want one span, the most context, then the nearest,
        // then the first in the list takes it.
        let mut taker: HashMap<Span, (Reverse<usize>, usize, usize)> = HashMap::new();
        for (p, c) in self.pending.iter().zip(&chosen) {
            if let Some((c, d)) = c {
                let rank = (Reverse(c.context), *d, p.index);
                taker
                    .entry(c.span)
                    .and_modify(|r| *r = (*r).min(rank))
                    .or_insert(rank);
            }
        }
        // Highlights on one span with one quote (two colours, two notes)
        // are twins: where one takes a span, the other moves with it.
        let by_index: HashMap<usize, &Highlight> = self
            .pending
            .iter()
            .map(|p| (p.index, &p.highlight))
            .collect();
        let twins = |a: &Highlight, b: &Highlight| {
            a.start == b.start && a.end == b.end && a.quote == b.quote
        };
        let takes: Vec<bool> = self
            .pending
            .iter()
            .zip(&chosen)
            .map(|(p, c)| {
                c.as_ref().is_some_and(|(c, _)| {
                    taker
                        .get(&c.span)
                        .is_some_and(|r| twins(by_index[&r.2], &p.highlight))
                })
            })
            .collect();
        self.pending
            .into_iter()
            .zip(chosen)
            .zip(takes)
            .map(|((p, c), takes)| {
                let best = c.map(|(c, _)| c).filter(|_| takes);
                let mut h = p.highlight;
                match best {
                    Some(c) => {
                        h.start = Location {
                            part: c.span.start.0,
                            char: c.span.start.1,
                        };
                        h.end = Location {
                            part: c.span.end.0,
                            char: c.span.end.1,
                        };
                        h.orphaned = false;
                    }
                    None => h.orphaned = true,
                }
                (p.index, h)
            })
            .collect()
    }
}

/// The highlights after anchoring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchored {
    pub items: Vec<Highlight>,
    /// Whether any highlight's position or orphaned flag changed.
    pub relocated: bool,
}

/// Gives a highlight found by the walk the quote of its new place, so its
/// prefix and suffix describe the new neighbours and a later check keeps it.
/// `window` holds the pages of `h.window(..)`.
///
/// The match is of the trimmed text; a highlight made with a space at an
/// edge takes it in again where the new place has one. Its quote shows
/// that: copying puts the space into the selection, not the neighbour, so
/// that side's neighbour doesn't end on whitespace.
pub fn requote(h: &mut Highlight, window: &Joined) {
    if let Some(q) = &h.quote {
        let before = q.prefix.chars().last().is_some_and(|c| !c.is_whitespace());
        let after = q.suffix.chars().next().is_some_and(|c| !c.is_whitespace());
        let wide = window.widen(h.span(), before, after);
        h.start = Location {
            part: wide.start.0,
            char: wide.start.1,
        };
        h.end = Location {
            part: wide.end.0,
            char: wide.end.1,
        };
    }
    let (exact, prefix, suffix) = window.quote(h.span());
    h.quote = Some(Quote {
        exact,
        prefix,
        suffix,
    });
}

/// Anchors `highlights` against `pages`, all in memory: checks each where
/// it was, then, with `relocate`, walks the pages for those that need
/// searching; without it they are left as they are.
pub fn anchor(
    highlights: &[Highlight],
    pages: &[&PageText],
    fill_missing: bool,
    relocate: bool,
) -> Anchored {
    let n = pages.len();
    let mut items = highlights.to_vec();
    let mut pending = Vec::new();
    for (i, h) in highlights.iter().enumerate() {
        match h.window(n) {
            Some(w) => {
                let first = *w.start();
                let window = Joined::new(first, &pages[w]);
                match check(h, &window, fill_missing) {
                    Check::Keep(kept) => items[i] = kept,
                    Check::Search if relocate => pending.push((i, h.clone())),
                    Check::Search => {}
                }
            }
            None if relocate && h.quote.is_some() => pending.push((i, h.clone())),
            None => {}
        }
    }
    if !pending.is_empty() {
        let mut walk = Walk::new(pending);
        for page in 0..n {
            let w = walk.window(page, n);
            walk.page(page, &Joined::new(*w.start(), &pages[w]));
        }
        let counts: Vec<usize> = pages.iter().map(|p| p.char_count()).collect();
        for (i, mut h) in walk.finish(&counts, &items) {
            if let (false, Some(w)) = (h.orphaned, h.window(n)) {
                requote(&mut h, &Joined::new(*w.start(), &pages[w]));
            }
            items[i] = h;
        }
    }
    let relocated = items
        .iter()
        .zip(highlights)
        .any(|(a, b)| a.start != b.start || a.end != b.end || a.orphaned != b.orphaned);
    Anchored { items, relocated }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use noido_text::testing::page;
    use noido_text::{Joined, PageText, Span};

    use super::*;
    use crate::{Highlight, HighlightColour, Location, Quote};

    fn text(lines: &[&str]) -> PageText {
        PageText::new(page(lines))
    }

    fn refs(pages: &[PageText]) -> Vec<&PageText> {
        pages.iter().collect()
    }

    fn loc(part: usize, char: usize) -> Location {
        Location { part, char }
    }

    fn plain(start: (usize, usize), end: (usize, usize)) -> Highlight {
        Highlight {
            start: loc(start.0, start.1),
            end: loc(end.0, end.1),
            colour: HighlightColour::Green,
            quote: None,
            orphaned: false,
        }
    }

    /// A highlight with the quote `pages` give its range.
    fn made(pages: &[PageText], start: (usize, usize), end: (usize, usize)) -> Highlight {
        let (exact, prefix, suffix) = Joined::new(0, &refs(pages)).quote(Span { start, end });
        Highlight {
            quote: Some(Quote {
                exact,
                prefix,
                suffix,
            }),
            ..plain(start, end)
        }
    }

    fn quoted(
        start: (usize, usize),
        end: (usize, usize),
        exact: &str,
        prefix: &str,
        suffix: &str,
    ) -> Highlight {
        Highlight {
            quote: Some(Quote {
                exact: exact.into(),
                prefix: prefix.into(),
                suffix: suffix.into(),
            }),
            ..plain(start, end)
        }
    }

    fn span_of(h: &Highlight) -> ((usize, usize), (usize, usize)) {
        ((h.start.part, h.start.char), (h.end.part, h.end.char))
    }

    #[test]
    fn unchanged_text_changes_nothing() {
        let pages = [text(&["the cat sat. the cat ran."]), text(&["other"])];
        let items = vec![made(&pages, (0, 13), (0, 20)), made(&pages, (1, 0), (1, 5))];
        let out = anchor(&items, &refs(&pages), true, true);
        assert_eq!(out.items, items);
        assert!(!out.relocated);
    }

    #[test]
    fn fills_a_missing_quote_only_when_asked() {
        let pages = [text(&["the cat sat."])];
        let h = plain((0, 0), (0, 7));
        let out = anchor(std::slice::from_ref(&h), &refs(&pages), true, true);
        let expected = made(&pages, (0, 0), (0, 7)).quote;
        assert_eq!(out.items[0].quote, expected);
        assert_eq!(out.items[0].quote.as_ref().unwrap().exact, "the cat");
        assert!(!out.relocated);
        let out = anchor(&[h], &refs(&pages), false, true);
        assert_eq!(out.items[0].quote, None);
    }

    #[test]
    fn without_relocate_a_moved_highlight_stays() {
        let old = [text(&["the cat"])];
        let h = made(&old, (0, 0), (0, 7));
        let new = [text(&["so the cat"])];
        let out = anchor(std::slice::from_ref(&h), &refs(&new), true, false);
        assert_eq!(out.items, std::slice::from_ref(&h));
        assert!(!out.relocated);
        let gone = [text(&["nothing here."])];
        let orphan = anchor(std::slice::from_ref(&h), &refs(&gone), true, true).items;
        assert!(orphan[0].orphaned);
        let out = anchor(&orphan, &refs(&new), true, false);
        assert_eq!(out.items, orphan);
        assert!(!out.relocated);
    }

    #[test]
    fn follows_text_moved_on_its_page() {
        let old = [text(&["the cat"])];
        let h = made(&old, (0, 0), (0, 7));
        let new = [text(&["so the cat"])];
        let out = anchor(std::slice::from_ref(&h), &refs(&new), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 3), (0, 10)));
        let quote = out.items[0].quote.as_ref().unwrap();
        assert_eq!(quote.exact, "the cat");
        assert_eq!(quote.prefix, "so ");
        assert_eq!(out.items[0].colour, h.colour);
        assert!(out.relocated);
    }

    #[test]
    fn follows_text_moved_to_another_page() {
        let old = [text(&["zz the cat"]), text(&["other"])];
        let h = made(&old, (0, 3), (0, 10));
        let new = [text(&["other"]), text(&["zz the cat"])];
        let out = anchor(&[h], &refs(&new), false, true);
        assert_eq!(span_of(&out.items[0]), ((1, 3), (1, 10)));
        assert!(out.relocated);
    }

    #[test]
    fn deleted_text_orphans_and_comes_back() {
        let old = [text(&["the cat sat."])];
        let h = made(&old, (0, 0), (0, 7));
        let new = [text(&["nothing here."])];
        let out = anchor(std::slice::from_ref(&h), &refs(&new), false, true);
        assert!(out.items[0].orphaned);
        assert_eq!(span_of(&out.items[0]), ((0, 0), (0, 7)));
        assert!(out.relocated);
        let back = anchor(&out.items, &refs(&old), false, true);
        assert_eq!(back.items, [h]);
        assert!(back.relocated);
    }

    #[test]
    fn repeats_resolve_by_context() {
        let pages = [text(&["the cat sat. the cat ran."])];
        let h = quoted((0, 0), (0, 7), "the cat", "sat. ", " ran");
        let out = anchor(&[h], &refs(&pages), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 13), (0, 20)));
    }

    #[test]
    fn moved_to_next_page_beats_a_repeat_on_the_old_page() {
        let pages = [text(&["the cat slept."]), text(&["sat. the cat ran."])];
        let h = quoted((0, 13), (0, 20), "the cat", "sat. ", " ran");
        let out = anchor(&[h], &refs(&pages), false, true);
        assert_eq!(span_of(&out.items[0]), ((1, 5), (1, 12)));
    }

    #[test]
    fn repeat_at_the_old_offset_does_not_hold_it() {
        let pages = [text(&["q. the cat gone. a. the cat ran."])];
        let h = quoted((0, 3), (0, 10), "the cat", "a. ", " ran");
        let out = anchor(&[h], &refs(&pages), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 20), (0, 27)));
        assert!(out.relocated);
    }

    #[test]
    fn now_straddling_a_page_break() {
        let old = [text(&["the cat sat on mat"])];
        let h = made(&old, (0, 4), (0, 11));
        assert_eq!(h.quote.as_ref().unwrap().exact, "cat sat");
        let new = [text(&["the cat"]), text(&["sat on mat"])];
        let out = anchor(&[h], &refs(&new), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 4), (1, 3)));
    }

    #[test]
    fn across_a_page_break_moves_as_a_whole() {
        let old = [text(&["one two three"]), text(&["four five six"])];
        let h = made(&old, (0, 8), (1, 4));
        assert_eq!(h.quote.as_ref().unwrap().exact, "three four");
        let new = [text(&["zero", "one two three"]), text(&["four five six"])];
        let out = anchor(&[h], &refs(&new), false, true);
        let at = new[0].text().find("three").unwrap();
        assert_eq!(span_of(&out.items[0]), ((0, at), (1, 4)));
    }

    #[test]
    fn long_highlight_over_three_pages_is_found() {
        let old = [
            text(&["head"]),
            text(&["alpha beta gamma"]),
            text(&["delta epsilon zeta"]),
            text(&["eta theta iota"]),
            text(&["tail"]),
        ];
        let gamma = old[1].text().find("gamma").unwrap();
        let h = made(&old, (1, gamma), (3, 3));
        assert_eq!(
            h.quote.as_ref().unwrap().exact,
            "gamma delta epsilon zeta eta"
        );
        assert_eq!(Walk::new(vec![(0, h.clone())]).span(), 3);
        let new = [
            text(&["head"]),
            text(&["extra", "alpha beta gamma"]),
            text(&["delta epsilon zeta"]),
            text(&["eta theta iota"]),
            text(&["tail"]),
        ];
        let out = anchor(&[h], &refs(&new), false, true);
        let at = new[1].text().find("gamma").unwrap();
        assert_eq!(span_of(&out.items[0]), ((1, at), (3, 3)));
        assert!(!out.items[0].orphaned);
    }

    #[test]
    fn past_the_last_page_reanchors_or_orphans() {
        let mut old: Vec<PageText> = (0..5).map(|_| text(&["nothing"])).collect();
        old.push(text(&["the cat"]));
        let h = made(&old, (5, 0), (5, 7));
        assert_eq!(h.window(2), None);
        let new = [
            text(&["nothing"]),
            text(&["nothing nothing nothing nothing the cat"]),
        ];
        let out = anchor(std::slice::from_ref(&h), &refs(&new), false, true);
        assert_eq!(span_of(&out.items[0]), ((1, 32), (1, 39)));
        assert!(!out.items[0].orphaned);
        let gone = [text(&["nothing"]), text(&["nothing"])];
        let out = anchor(&[h], &refs(&gone), false, true);
        assert!(out.items[0].orphaned);
        assert_eq!(span_of(&out.items[0]), ((5, 0), (5, 7)));
    }

    #[test]
    fn equal_context_goes_to_the_nearest_old_start() {
        let pages = [text(&["the cat."]), text(&["x"]), text(&["the cat."])];
        let h = quoted((2, 3), (2, 6), "the cat", "zzz", ".");
        let out = anchor(&[h], &refs(&pages), false, true);
        assert_eq!(span_of(&out.items[0]), ((2, 0), (2, 7)));
    }

    #[test]
    fn past_the_end_counts_as_the_end_of_the_document() {
        let pages = [text(&["the cat."]), text(&["the cat."])];
        let h = quoted((5, 0), (5, 7), "the cat", "zzz", ".");
        let out = anchor(&[h], &refs(&pages), false, true);
        assert_eq!(span_of(&out.items[0]), ((1, 0), (1, 7)));
    }

    #[test]
    fn walk_window_is_what_page_expects() {
        let h = plain((1, 0), (3, 1));
        let walk = Walk::new(vec![(0, h)]);
        assert_eq!(walk.span(), 3);
        assert_eq!(walk.window(0, 10), 0..=3);
        assert_eq!(walk.window(5, 10), 4..=8);
        assert_eq!(walk.window(9, 10), 8..=9);
    }

    #[test]
    fn anchoring_again_is_a_no_op() {
        let old = [text(&["a the cat b"])];
        let h = made(&old, (0, 2), (0, 9));
        let new = [text(&["so a the cat sat"])];
        let first = anchor(std::slice::from_ref(&h), &refs(&new), false, true);
        assert!(first.relocated);
        let quote = first.items[0].quote.as_ref().unwrap();
        assert_eq!(
            (quote.prefix.as_str(), quote.suffix.as_str()),
            ("so a ", " sat")
        );
        let again = anchor(&first.items, &refs(&new), false, true);
        assert_eq!(again.items, first.items);
        assert!(!again.relocated);
        let w = first.items[0].window(1).unwrap();
        let window = Joined::new(*w.start(), &refs(&new)[w]);
        assert_eq!(
            check(&first.items[0], &window, false),
            Check::Keep(first.items[0].clone())
        );
    }

    #[test]
    fn window_is_clamped_to_the_document() {
        let h = plain((2, 0), (3, 1));
        assert_eq!(h.window(10), Some(1..=4));
        assert_eq!(h.window(4), Some(1..=3));
        assert_eq!(plain((0, 0), (0, 1)).window(10), Some(0..=1));
        assert_eq!(h.window(2), None);
    }

    #[test]
    fn one_side_agreeing_in_full_is_enough_to_move() {
        let old = [text(&["xx aa the cat bb yy"])];
        let h = made(&old, (0, 6), (0, 13));
        let prefix_kept = [text(&["more xx aa the cat zz yy"])];
        let out = anchor(std::slice::from_ref(&h), &refs(&prefix_kept), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 11), (0, 18)));
        assert!(!out.items[0].orphaned);
        let suffix_kept = [text(&["more qq the cat bb yy"])];
        let out = anchor(&[h], &refs(&suffix_kept), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 8), (0, 15)));
        assert!(!out.items[0].orphaned);
    }

    #[test]
    fn neighbours_changed_on_both_sides_orphan() {
        let old = [text(&["xx aa the cat bb yy"])];
        let h = made(&old, (0, 6), (0, 13));
        let new = [text(&["xx qq the cat zz yy"])];
        let out = anchor(std::slice::from_ref(&h), &refs(&new), false, true);
        assert!(out.items[0].orphaned);
        assert_eq!(out.items[0].quote, h.quote);
        assert_eq!(span_of(&out.items[0]), span_of(&h));
        assert!(out.relocated);
    }

    #[test]
    fn an_empty_side_is_not_agreement() {
        let old = [text(&["the cat sat"])];
        let h = made(&old, (0, 0), (0, 7));
        assert_eq!(h.quote.as_ref().unwrap().prefix, "");
        let new = [text(&["so the cat ran"])];
        let out = anchor(&[h], &refs(&new), false, true);
        assert!(out.items[0].orphaned);
    }

    #[test]
    fn the_best_match_must_be_sure_not_just_some_match() {
        // The best match agrees on most of both sides; a weaker one agrees
        // in full on one side only. Neither is taken.
        let h = quoted((0, 0), (0, 7), "the cat", "abcd ", " efgh");
        let pages = [text(&["Xbcd the cat efgX. abcd the cat qqqq"])];
        let out = anchor(&[h], &refs(&pages), false, true);
        assert!(out.items[0].orphaned);
    }

    #[test]
    fn a_deleted_repeat_orphans_rather_than_taking_the_other() {
        let first = "Waves broke on the wall. The keeper counted the ships that passed.";
        let second = "It grew late, so he slept. The keeper counted the ships for the last time.";
        let old = [text(&[first, second])];
        let at = old[0].text().find("The keeper").unwrap();
        let h = made(
            &old,
            (0, at),
            (0, at + "The keeper counted the ships".len()),
        );
        let new = [text(&[second])];
        let out = anchor(std::slice::from_ref(&h), &refs(&new), false, true);
        assert!(out.items[0].orphaned);
        assert_eq!(out.items[0].quote, h.quote);
        let back = anchor(&out.items, &refs(&old), false, true);
        assert_eq!(back.items, [h]);
    }

    #[test]
    fn never_onto_a_span_another_highlight_keeps() {
        let pages = [text(&["aa the cat bb"])];
        let kept = made(&pages, (0, 3), (0, 10));
        let q = kept.quote.clone().unwrap();
        // Same quote, from a passage that is gone.
        let gone = Highlight {
            colour: HighlightColour::Blue,
            ..quoted((0, 40), (0, 47), &q.exact, &q.prefix, &q.suffix)
        };
        let out = anchor(&[gone.clone(), kept.clone()], &refs(&pages), false, true);
        assert!(out.items[0].orphaned);
        assert_eq!(span_of(&out.items[0]), span_of(&gone));
        assert_eq!(out.items[1], kept);
    }

    #[test]
    fn two_moving_onto_one_span_the_better_neighbours_take_it() {
        let pages = [text(&["new aa the cat bb"])];
        let weak = quoted((0, 30), (0, 37), "the cat", "aa ", " zz");
        let strong = quoted((0, 50), (0, 57), "the cat", "aa ", " bb");
        let out = anchor(&[weak, strong], &refs(&pages), false, true);
        assert!(out.items[0].orphaned);
        assert!(!out.items[1].orphaned);
        assert_eq!(span_of(&out.items[1]), ((0, 7), (0, 14)));
        // Equal neighbours: the nearer old start takes it.
        let far = quoted((0, 90), (0, 97), "the cat", "aa ", " bb");
        let near = quoted((0, 20), (0, 27), "the cat", "aa ", " bb");
        let out = anchor(&[far, near], &refs(&pages), false, true);
        assert!(out.items[0].orphaned);
        assert_eq!(span_of(&out.items[1]), ((0, 7), (0, 14)));
    }

    #[test]
    fn identical_passages_one_deleted_lose_nothing() {
        // Text alone can't tell which copy went: the deleted one's highlight
        // stays on the copy that slid into its place, and the other is
        // orphaned. Putting the passage back brings both back.
        let p =
            "The keeper counted the ships that passed in the bay at dawn, and wrote them down. ";
        let both = format!("Intro. {p}{p}Outro.");
        let old = [text(&[&both])];
        let at = both.find("counted").unwrap();
        let second = at + p.len();
        let len = "counted the ships".len();
        let a = made(&old, (0, at), (0, at + len));
        let b = Highlight {
            colour: HighlightColour::Blue,
            ..made(&old, (0, second), (0, second + len))
        };
        let one = format!("Intro. {p}Outro.");
        let new = [text(&[&one])];
        let out = anchor(&[a.clone(), b.clone()], &refs(&new), false, true);
        assert_eq!(out.items[0], a);
        assert!(out.items[1].orphaned);
        assert_eq!(out.items[1].quote, b.quote);
        let back = anchor(&out.items, &refs(&old), false, true);
        assert_eq!(back.items, [a, b]);
    }

    #[test]
    fn a_selection_with_spaces_at_its_edges_stays_and_follows() {
        let old = [text(&["there was a quiet argument on tides"])];
        let h = made(&old, (0, 11), (0, 27));
        let out = anchor(std::slice::from_ref(&h), &refs(&old), false, true);
        assert_eq!(out.items, std::slice::from_ref(&h));
        assert!(!out.relocated);
        let new = [text(&["new words. there was a quiet argument on tides"])];
        let out = anchor(&[h], &refs(&new), false, true);
        assert!(!out.items[0].orphaned);
        assert_eq!(out.items[0].quote.as_ref().unwrap().exact, "quiet argument");
        // It keeps the spaces it was made with, and stays put after that.
        assert_eq!(span_of(&out.items[0]), ((0, 22), (0, 38)));
        let again = anchor(&out.items, &refs(&new), false, true);
        assert_eq!(again.items, out.items);
        assert!(!again.relocated);
    }

    #[test]
    fn edge_spaces_are_kept_on_each_side_alone() {
        let old = [text(&["there was a quiet argument, on tides"])];
        let start_only = made(&old, (0, 11), (0, 26));
        let end_only = made(&old, (0, 6), (0, 11));
        assert_eq!(end_only.quote.as_ref().unwrap().exact, "was a");
        let new = [text(&["new words. there was a quiet argument, on tides"])];
        let out = anchor(&[start_only, end_only], &refs(&new), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 22), (0, 37)));
        assert_eq!(span_of(&out.items[1]), ((0, 17), (0, 22)));
    }

    #[test]
    fn a_line_break_is_not_taken_in_for_a_space() {
        let old = [text(&["there was a quiet argument on tides"])];
        let h = made(&old, (0, 11), (0, 27));
        let new = [text(&["new words. there was a", "quiet argument on tides"])];
        let out = anchor(&[h], &refs(&new), false, true);
        let q = new[0].text().find("quiet").unwrap();
        assert_eq!(span_of(&out.items[0]), ((0, q), (0, q + 15)));
    }

    #[test]
    fn a_double_space_at_the_edge_stays_put_after_a_move() {
        let old = [text(&["there was a  quiet argument  on tides"])];
        let h = made(&old, (0, 12), (0, 28));
        let new = [text(&["new words. there was a  quiet argument  on tides"])];
        let out = anchor(&[h], &refs(&new), false, true);
        assert!(!out.items[0].orphaned);
        let again = anchor(&out.items, &refs(&new), false, true);
        assert_eq!(again.items, out.items);
        assert!(!again.relocated);
    }

    #[test]
    fn never_inside_a_span_another_highlight_keeps() {
        let pages = [text(&["new words. there was a quiet argument on tides"])];
        let kept = made(&pages, (0, 22), (0, 38));
        let other = quoted(
            (0, 60),
            (0, 74),
            "quiet argument",
            "there was a",
            "on tides",
        );
        let out = anchor(&[kept.clone(), other], &refs(&pages), false, true);
        assert_eq!(out.items[0], kept);
        assert!(out.items[1].orphaned);
    }

    #[test]
    fn a_selection_without_edge_spaces_does_not_gain_them() {
        let old = [text(&["there was a quiet argument on tides"])];
        let h = made(&old, (0, 12), (0, 26));
        let new = [text(&["new words. there was a quiet argument on tides"])];
        let out = anchor(&[h], &refs(&new), false, true);
        assert_eq!(span_of(&out.items[0]), ((0, 23), (0, 37)));
    }

    #[test]
    fn a_quote_saved_without_its_edge_spaces_still_holds() {
        let pages = [text(&["there was a quiet argument on tides"])];
        let h = quoted(
            (0, 11),
            (0, 27),
            "quiet argument",
            "there was a",
            "on tides",
        );
        let out = anchor(std::slice::from_ref(&h), &refs(&pages), false, true);
        assert_eq!(out.items, [h]);
        assert!(!out.relocated);
    }

    #[test]
    fn two_highlights_on_one_span_move_together() {
        let old = [text(&["aa the cat bb"])];
        let a = made(&old, (0, 3), (0, 10));
        let b = Highlight {
            colour: HighlightColour::Blue,
            ..a.clone()
        };
        let new = [text(&["more aa the cat bb"])];
        let out = anchor(&[a, b], &refs(&new), false, true);
        for h in &out.items {
            assert!(!h.orphaned);
            assert_eq!(span_of(h), ((0, 8), (0, 15)));
        }
        let again = anchor(&out.items, &refs(&new), false, true);
        assert_eq!(again.items, out.items);
        assert!(!again.relocated);
    }

    #[test]
    fn short_quote_many_times() {
        let line = "a a a a a a a a a a";
        let mut pages: Vec<PageText> = (0..300).map(|_| text(&[line; 5])).collect();
        pages[150] = text(&["a a a x a a a"]);
        let h = quoted((3, 0), (3, 1), "a", "a a x ", " a a");
        let start = Instant::now();
        let out = anchor(&[h], &refs(&pages), false, true);
        let took = start.elapsed();
        assert_eq!(span_of(&out.items[0]), ((150, 8), (150, 9)));
        assert!(took.as_millis() < 100, "took {took:?}");
    }
}
