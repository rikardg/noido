//! Finding text across a few consecutive pages, and taking a quote with its
//! neighbours. Built over a small window of pages, never a whole book.

use crate::search::{Folds, fold};
use crate::{PageText, Query, copy_text};

/// Characters of a quote's prefix and suffix.
const QUOTE_CONTEXT: usize = 32;

/// A range of text over pages, as (document page, char): `end` is one past
/// the last character, on that character's page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

/// Consecutive pages' text as one, joined with `'\n'`, with its folded forms.
/// Dropped with the window; the pages don't keep them.
pub struct Joined<'a> {
    first: usize,
    pages: Vec<&'a PageText>,
    /// Where each page starts in the joined characters.
    starts: Vec<usize>,
    folds: Folds,
}

impl<'a> Joined<'a> {
    /// `first` is the document page number of `pages[0]`.
    pub fn new(first: usize, pages: &[&'a PageText]) -> Joined<'a> {
        let mut chars = Vec::new();
        let mut starts = Vec::new();
        for (i, page) in pages.iter().enumerate() {
            if i > 0 {
                chars.push('\n');
            }
            starts.push(chars.len());
            chars.extend(page.text().chars());
        }
        let folds = fold(&chars, true);
        Joined {
            first,
            pages: pages.to_vec(),
            starts,
            folds,
        }
    }

    /// (document page, char) of joined character `i`. The separator after a
    /// page counts as that page's last-plus-one character.
    fn locate(&self, i: usize) -> (usize, usize) {
        let p = self.starts.partition_point(|&s| s <= i).saturating_sub(1);
        (self.first + p, i - self.starts[p])
    }

    /// The joined index of a (page, char), if inside the window.
    fn index_of(&self, (page, char): (usize, usize)) -> Option<usize> {
        let p = page.checked_sub(self.first)?;
        let text = self.pages.get(p)?;
        (char <= text.char_count()).then(|| self.starts[p] + char)
    }

    /// The matches of `query`, in text order, not overlapping.
    pub fn find(&self, query: &Query) -> Vec<Span> {
        let (chars, source) = if query.accented {
            (&self.folds.accented, &self.folds.accented_source)
        } else {
            (&self.folds.plain, &self.folds.plain_source)
        };
        let needle = &query.folded;
        let mut spans: Vec<Span> = Vec::new();
        let mut last_end = 0;
        let mut start = 0;
        while start + needle.len() <= chars.len() {
            if chars[start..start + needle.len()] == needle[..] {
                let first = source[start];
                let last = source[start + needle.len() - 1];
                // Two matches inside one character are one hit.
                if spans.is_empty() || first >= last_end {
                    let end = self.locate(last);
                    spans.push(Span {
                        start: self.locate(first),
                        end: (end.0, end.1 + 1),
                    });
                    last_end = last + 1;
                }
                start += needle.len();
            } else {
                start += 1;
            }
        }
        spans
    }

    /// How many folded characters before the match agree with the folded
    /// `prefix`, counting back from the match, plus how many after it agree
    /// with `suffix`, counting forward. Stops at the window's edges.
    pub fn context(&self, span: Span, prefix: &str, suffix: &str) -> usize {
        self.context_folded(span, &folded(prefix), &folded(suffix))
    }

    /// [`Self::context`] with `prefix` and `suffix` already folded by
    /// [`folded_chars`], for scoring many matches against one quote.
    pub fn context_folded(&self, span: Span, prefix: &[char], suffix: &[char]) -> usize {
        let (Some(start), Some(end)) = (self.index_of(span.start), self.index_of(span.end)) else {
            return 0;
        };
        let (chars, source) = (&self.folds.plain, &self.folds.plain_source);
        let before = source.partition_point(|&s| s < start);
        let after = source.partition_point(|&s| s < end);
        let back = prefix
            .iter()
            .rev()
            .zip(chars[..before].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let forward = suffix
            .iter()
            .zip(&chars[after..])
            .take_while(|(a, b)| a == b)
            .count();
        back + forward
    }

    /// The copied text of `span`, or `None` when it isn't inside the window.
    pub fn text_at(&self, span: Span) -> Option<String> {
        self.index_of(span.start)?;
        self.index_of(span.end)?;
        if span.end < span.start {
            return None;
        }
        let pieces: Vec<_> = (span.start.0..=span.end.0)
            .map(|page| {
                let text = self.pages[page - self.first];
                let from = if page == span.start.0 {
                    span.start.1
                } else {
                    0
                };
                let to = if page == span.end.0 {
                    span.end.1
                } else {
                    text.char_count()
                };
                (text, from..to)
            })
            .collect();
        Some(copy_text(&pieces))
    }

    /// The exact text, and up to 32 characters of the joined text before and
    /// after it, whitespace runs collapsed to one space.
    pub fn quote(&self, span: Span) -> (String, String, String) {
        let exact = self.text_at(span).unwrap_or_default();
        if exact.is_empty() && self.text_at(span).is_none() {
            return (exact, String::new(), String::new());
        }
        // The neighbours read as copied text, so folding them matches the
        // window's dehyphenated fold.
        let (sp, ep) = (span.start.0 - self.first, span.end.0 - self.first);
        let before: Vec<_> = (0..=sp)
            .map(|p| {
                let to = if p == sp {
                    span.start.1
                } else {
                    self.pages[p].char_count()
                };
                (self.pages[p], 0..to)
            })
            .collect();
        let after: Vec<_> = (ep..self.pages.len())
            .map(|p| {
                let from = if p == ep { span.end.1 } else { 0 };
                (self.pages[p], from..self.pages[p].char_count())
            })
            .collect();
        let mut before: Vec<char> = copy_text(&before).chars().collect();
        let mut after: Vec<char> = copy_text(&after).chars().collect();
        // Copying drops the whitespace at the cut; the window has it.
        let char_at = |p: usize, i: usize| self.pages[p].text().chars().nth(i);
        let left = match span.start.1 {
            0 => (sp > 0).then_some('\n'),
            i => char_at(sp, i - 1),
        };
        if left.is_some_and(char::is_whitespace) && before.last() != Some(&' ') {
            before.push(' ');
        }
        let right = match char_at(ep, span.end.1) {
            None if ep + 1 < self.pages.len() => Some('\n'),
            c => c,
        };
        if right.is_some_and(char::is_whitespace) && after.first() != Some(&' ') {
            after.insert(0, ' ');
        }
        let prefix = &before[before.len().saturating_sub(QUOTE_CONTEXT)..];
        let suffix = &after[..after.len().min(QUOTE_CONTEXT)];
        (exact, collapse(prefix), collapse(suffix))
    }
}

fn collapse(chars: &[char]) -> String {
    let mut out = String::new();
    for &c in chars {
        if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `s` folded plain, not trimmed.
fn folded(s: &str) -> Vec<char> {
    fold(&s.chars().collect::<Vec<_>>(), false).plain
}

/// `s` folded the way [`Joined::context_folded`] wants its neighbours.
pub fn folded_chars(s: &str) -> Vec<char> {
    folded(s)
}

/// Whether `a` and `b` are the same after search folding (accents kept).
pub fn same_text(a: &str, b: &str) -> bool {
    let accented = |s: &str| {
        let mut f = fold(&s.chars().collect::<Vec<_>>(), true).accented;
        while f.first() == Some(&' ') {
            f.remove(0);
        }
        while f.last() == Some(&' ') {
            f.pop();
        }
        f
    };
    accented(a) == accented(b)
}

/// The length of `s` after search folding.
pub fn folded_len(s: &str) -> usize {
    folded(s).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::page;
    use crate::{PageText, Query};

    fn text(lines: &[&str]) -> PageText {
        PageText::new(page(lines))
    }

    fn q(s: &str) -> Query {
        Query::new(s).unwrap()
    }

    #[test]
    fn finds_text_across_a_page_break() {
        let (a, b) = (text(&["the nature and"]), text(&["limits of power"]));
        let j = Joined::new(0, &[&a, &b]);
        let spans = j.find(&q("and limits"));
        assert_eq!(
            spans,
            [Span {
                start: (0, 11),
                end: (1, 6)
            }]
        );
        assert_eq!(j.text_at(spans[0]).unwrap(), "and limits");
    }

    #[test]
    fn window_reports_document_pages() {
        let (a, b) = (text(&["the nature and"]), text(&["limits of power"]));
        let j = Joined::new(7, &[&a, &b]);
        let spans = j.find(&q("and limits"));
        assert_eq!(spans[0].start, (7, 11));
        assert_eq!(spans[0].end, (8, 6));
    }

    #[test]
    fn finds_a_word_hyphenated_across_pages() {
        let (a, b) = (text(&["the magis-"]), text(&["trates ruled"]));
        let j = Joined::new(3, &[&a, &b]);
        let spans = j.find(&q("magistrates"));
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, (3, 4));
        assert_eq!(spans[0].end, (4, 6));
    }

    #[test]
    fn finds_with_search_folding() {
        let a = text(&["Ligature \u{fb01}ne"]);
        let j = Joined::new(0, &[&a]);
        assert_eq!(j.find(&q("ligature fine")).len(), 1);
    }

    #[test]
    fn context_prefers_matching_neighbours() {
        let a = text(&["the cat sat. the cat ran."]);
        let j = Joined::new(0, &[&a]);
        let spans = j.find(&q("the cat"));
        assert_eq!(spans.len(), 2);
        let first = j.context(spans[0], "sat. ", " ran");
        let second = j.context(spans[1], "sat. ", " ran");
        assert!(second > first);
        assert_eq!(second, folded_len("sat. ") + folded_len(" ran"));
    }

    #[test]
    fn quote_takes_32_characters_each_side() {
        let line: String = (0..100)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let a = text(&[&line]);
        let j = Joined::new(0, &[&a]);
        let (exact, prefix, suffix) = j.quote(Span {
            start: (0, 40),
            end: (0, 50),
        });
        assert_eq!(exact, line[40..50]);
        assert_eq!(prefix, line[8..40]);
        assert_eq!(suffix, line[50..82]);
        let (_, prefix, _) = j.quote(Span {
            start: (0, 0),
            end: (0, 5),
        });
        assert_eq!(prefix, "");
    }

    #[test]
    fn quote_neighbours_cross_pages() {
        let (a, b) = (text(&["end of one"]), text(&["start of two"]));
        let j = Joined::new(0, &[&a, &b]);
        let (exact, prefix, _) = j.quote(Span {
            start: (1, 0),
            end: (1, 5),
        });
        assert_eq!(exact, "start");
        assert_eq!(prefix, "end of one ");
    }

    #[test]
    fn same_text_folds() {
        assert!(same_text("Magis-\ntrates", "magistrates"));
        assert!(!same_text("café", "cafe"));
    }

    #[test]
    fn a_quote_scores_full_across_hyphens_in_its_neighbours() {
        let a = text(&["a inter-", "preter one magis-"]);
        let b = text(&["trates two NEEDLE five", "magis-"]);
        let c = text(&["trates end"]);
        let j = Joined::new(0, &[&a, &b, &c]);
        let span = j.find(&q("needle"))[0];
        let (exact, prefix, suffix) = j.quote(span);
        assert_eq!(exact, "NEEDLE");
        assert!(prefix.contains("magistrates"), "{prefix:?}");
        assert!(suffix.contains("magistrates"), "{suffix:?}");
        assert_eq!(
            j.context(span, &prefix, &suffix),
            folded_len(&prefix) + folded_len(&suffix)
        );
    }
}
