//! Search on a page's text: case ignored, compatibility characters (such as
//! ligatures) normalised, accents ignored unless the query has some, and
//! words found across line breaks and line-end hyphens.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::{PageText, Rect};

/// Characters of context before and after a hit, about.
const CONTEXT_BEFORE: usize = 150;
const CONTEXT_AFTER: usize = 150;

/// What to search for, folded the way page text is.
#[derive(Clone, Debug)]
pub struct Query {
    folded: Vec<char>,
    /// Whether it has accents, so they must match.
    accented: bool,
}

impl Query {
    /// `None` if `text` has nothing to search for.
    pub fn new(text: &str) -> Option<Query> {
        let chars: Vec<char> = text.trim().chars().collect();
        let folded = fold(&chars, false);
        if folded.accented.is_empty() {
            return None;
        }
        let accented = folded.accented != folded.plain;
        let folded = if accented {
            folded.accented
        } else {
            folded.plain
        };
        Some(Query { folded, accented })
    }
}

/// A match on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct TextHit {
    /// The characters of the page's text it covers.
    pub range: Range<usize>,
    /// Where to highlight it: one rectangle per line, in page points.
    pub rects: Vec<Rect>,
    pub context: Context,
}

/// A line of the page's text around a hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub text: String,
    /// The characters of `text` that are the hit.
    pub bold: Range<usize>,
}

/// A page's text folded for search, each folded character mapped back to the
/// character of the page's text it came from.
#[derive(Debug, Default)]
pub(crate) struct Forms {
    chars: Vec<char>,
    accented: Folded,
    plain: Folded,
}

#[derive(Debug, Default, PartialEq)]
struct Folded {
    chars: Vec<char>,
    source: Vec<usize>,
}

/// Both folded forms of some text.
struct Folds {
    accented: Vec<char>,
    plain: Vec<char>,
    accented_source: Vec<usize>,
    plain_source: Vec<usize>,
}

/// Folds `chars`: lowercase, NFKC, whitespace runs as one space, and with
/// `dehyphenate`, line-end hyphens inside a word removed. `plain` also drops
/// accents.
fn fold(chars: &[char], dehyphenate: bool) -> Folds {
    let mut out = Folds {
        accented: Vec::new(),
        plain: Vec::new(),
        accented_source: Vec::new(),
        plain_source: Vec::new(),
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if dehyphenate {
            match line_end_hyphen(chars, i) {
                Some((Hyphen::Breaks, next)) => {
                    // The hyphen and the line break go.
                    i = next;
                    continue;
                }
                Some((Hyphen::Belongs, next)) => {
                    // The hyphen stays; the line break goes.
                    out.accented.push(c);
                    out.accented_source.push(i);
                    out.plain.push(c);
                    out.plain_source.push(i);
                    i = next;
                    continue;
                }
                None => {}
            }
        }
        if c.is_whitespace() {
            if out.accented.last() != Some(&' ') {
                out.accented.push(' ');
                out.accented_source.push(i);
                out.plain.push(' ');
                out.plain_source.push(i);
            }
            i += 1;
            continue;
        }
        for n in std::iter::once(c).nfkc() {
            // Decomposed, so a mark drawn or typed apart still matches.
            for d in n.to_lowercase().nfd() {
                out.accented.push(d);
                out.accented_source.push(i);
                if !is_combining_mark(d) {
                    out.plain.push(d);
                    out.plain_source.push(i);
                }
            }
        }
        i += 1;
    }
    out
}

enum Hyphen {
    /// It breaks a word in two: "inter-" / "preter".
    Breaks,
    /// It belongs to the word: "Anglo-" / "Saxon".
    Belongs,
}

/// Whether `chars[i]` is a hyphen at a line end, after a letter and before
/// a letter on the next line, what kind, and where the next line starts.
/// Spaces may come between the hyphen and the line break, as in OCR layers.
/// Before a lowercase letter it breaks the word; before another letter it is
/// the word's own, except OCR's `¬` and the soft hyphen, which always mark a
/// break.
fn line_end_hyphen(chars: &[char], i: usize) -> Option<(Hyphen, usize)> {
    let hyphen = chars[i];
    if !matches!(hyphen, '-' | '\u{2010}' | '¬' | '\u{ad}')
        || i == 0
        || !chars[i - 1].is_alphabetic()
    {
        return None;
    }
    let mut j = i + 1;
    while chars
        .get(j)
        .is_some_and(|&c| c != '\n' && c.is_whitespace())
    {
        j += 1;
    }
    if chars.get(j) != Some(&'\n') {
        return None;
    }
    let next = *chars.get(j + 1)?;
    if !next.is_alphabetic() {
        return None;
    }
    let kind = if next.is_lowercase() || matches!(hyphen, '¬' | '\u{ad}') {
        Hyphen::Breaks
    } else {
        Hyphen::Belongs
    };
    Some((kind, j + 1))
}

impl Forms {
    fn new(text: &str) -> Forms {
        let chars: Vec<char> = text.chars().collect();
        let folds = fold(&chars, true);
        Forms {
            chars,
            accented: Folded {
                chars: folds.accented,
                source: folds.accented_source,
            },
            plain: Folded {
                chars: folds.plain,
                source: folds.plain_source,
            },
        }
    }
}

impl PageText {
    /// The matches of `query`, in text order, not overlapping.
    pub fn find(&self, query: &Query) -> Vec<TextHit> {
        let forms = self.forms.get_or_init(|| Forms::new(self.text()));
        let folded = if query.accented {
            &forms.accented
        } else {
            &forms.plain
        };
        let needle = &query.folded;
        let mut hits: Vec<TextHit> = Vec::new();
        let mut start = 0;
        while start + needle.len() <= folded.chars.len() {
            if folded.chars[start..start + needle.len()] == needle[..] {
                let first = folded.source[start];
                let last = folded.source[start + needle.len() - 1];
                // Two matches inside one character ("f" twice in "ﬀ") are
                // one hit.
                if hits.last().is_none_or(|hit| first >= hit.range.end) {
                    hits.push(self.hit(&forms.chars, first..last + 1));
                }
                start += needle.len();
            } else {
                start += 1;
            }
        }
        hits
    }

    fn hit(&self, chars: &[char], range: Range<usize>) -> TextHit {
        TextHit {
            rects: self.rects(range.clone()),
            context: context(chars, range.clone()),
            range,
        }
    }

    /// One rectangle per line `range` touches: the union of its glyphs there.
    fn rects(&self, range: Range<usize>) -> Vec<Rect> {
        let mut rects: Vec<(usize, Rect)> = Vec::new();
        for c in range {
            let Some(bounds) = self.glyph_bounds(c) else {
                continue;
            };
            let line = self.line_of(c);
            match rects.last_mut() {
                Some((l, rect)) if *l == line => *rect = union(*rect, bounds),
                _ => rects.push((line, bounds)),
            }
        }
        rects.into_iter().map(|(_, rect)| rect).collect()
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

/// The text around `range`, cut at word boundaries, on one line.
fn context(chars: &[char], range: Range<usize>) -> Context {
    let mut start = range.start.saturating_sub(CONTEXT_BEFORE);
    if start > 0 {
        // Forward to the start of a word, but not into the hit.
        while start < range.start && !chars[start - 1].is_whitespace() {
            start += 1;
        }
    }
    let mut end = (range.end + CONTEXT_AFTER).min(chars.len());
    if end < chars.len() {
        while end > range.end && !chars[end].is_whitespace() {
            end -= 1;
        }
    }
    // No whitespace at either end.
    while start < range.start && chars[start].is_whitespace() {
        start += 1;
    }
    while end > range.end && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    Context {
        text: chars[start..end]
            .iter()
            .map(|&c| if c == '\n' { ' ' } else { c })
            .collect(),
        bold: range.start - start..range.end - start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{glyph, line, page};

    fn find(lines: &[&str], query: &str) -> Vec<TextHit> {
        PageText::new(page(lines)).find(&Query::new(query).unwrap())
    }

    /// The text of each hit.
    fn found(lines: &[&str], query: &str) -> Vec<String> {
        let page = PageText::new(page(lines));
        page.find(&Query::new(query).unwrap())
            .iter()
            .map(|hit| {
                page.text()
                    .chars()
                    .skip(hit.range.start)
                    .take(hit.range.len())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn case_is_ignored() {
        assert_eq!(found(&["Kant kant KANT"], "kant").len(), 3);
        assert_eq!(found(&["kant"], "KANT"), ["kant"]);
    }

    #[test]
    fn ligature_matches_letters() {
        let mut glyphs = vec![glyph("ﬁ", 72.0, 100.0, 10.0)];
        glyphs.extend(line("le", 77.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        let hits = page.find(&Query::new("file").unwrap());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].range, 0..3);
    }

    #[test]
    fn query_is_folded_like_text() {
        assert_eq!(found(&["a file"], "ﬁle"), ["file"]);
        assert_eq!(found(&["Gödel"], "GÖDEL"), ["Gödel"]);
    }

    #[test]
    fn accents_ignored_without_accent_in_query() {
        assert_eq!(found(&["Gödel and Godel"], "godel"), ["Gödel", "Godel"]);
        assert_eq!(found(&["café"], "cafe"), ["café"]);
    }

    #[test]
    fn accents_kept_with_accent_in_query() {
        assert_eq!(found(&["Gödel and Godel"], "gödel"), ["Gödel"]);
    }

    #[test]
    fn matches_across_line_break() {
        assert_eq!(
            found(&["the black", "cat sat"], "black cat"),
            ["black\ncat"]
        );
    }

    #[test]
    fn hyphen_at_line_end_joins_word() {
        assert_eq!(
            found(&["phenom-", "enology"], "phenomenology"),
            ["phenom-\nenology"]
        );
        assert_eq!(found(&["inter¬", "preter"], "interpreter").len(), 1);
    }

    #[test]
    fn capital_after_hyphen_keeps_it() {
        assert_eq!(found(&["Anglo-", "Saxon"], "anglo-saxon").len(), 1);
        assert!(found(&["Anglo-", "Saxon"], "anglosaxon").is_empty());
    }

    #[test]
    fn mid_line_hyphen_stays() {
        assert_eq!(
            found(&["a self-induced b"], "self-induced"),
            ["self-induced"]
        );
        assert!(found(&["a self-induced b"], "selfinduced").is_empty());
    }

    #[test]
    fn whitespace_runs_match_one_space() {
        assert_eq!(found(&["a  b"], "a b"), ["a  b"]);
        assert_eq!(found(&["a b"], "a   b"), ["a b"]);
    }

    #[test]
    fn hits_do_not_overlap() {
        assert_eq!(found(&["aaaa"], "aa").len(), 2);
    }

    #[test]
    fn hyphen_before_trailing_space_joins_word() {
        // OCR layers end lines with a space.
        assert_eq!(found(&["imple- ", "ment"], "implement").len(), 1);
        assert_eq!(found(&["Anglo- ", "Saxon"], "anglo-saxon").len(), 1);
    }

    #[test]
    fn unicode_hyphens_at_line_end_join_word() {
        assert_eq!(found(&["imple\u{2010}", "ment"], "implement").len(), 1);
        assert_eq!(found(&["imple\u{ad}", "ment"], "implement").len(), 1);
    }

    #[test]
    fn decomposed_accents_match_composed() {
        assert_eq!(found(&["Gödel"], "Go\u{308}del"), ["Gödel"]);
        assert_eq!(found(&["élan"], "E\u{301}LAN"), ["élan"]);
        // A mark left unjoined on the page.
        assert_eq!(found(&["cafe\u{301}"], "café").len(), 1);
    }

    #[test]
    fn one_ligature_char_is_one_hit() {
        let mut glyphs = vec![glyph("ﬀ", 72.0, 100.0, 10.0)];
        glyphs.extend(line("ect", 77.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        assert_eq!(page.find(&Query::new("f").unwrap()).len(), 1);
    }

    #[test]
    fn blank_query_is_none() {
        assert!(Query::new("").is_none());
        assert!(Query::new("   ").is_none());
    }

    #[test]
    fn query_is_trimmed() {
        assert_eq!(found(&["a cat"], " cat "), ["cat"]);
    }

    #[test]
    fn hit_rects_one_per_line() {
        let page = PageText::new(page(&["the black", "cat sat"]));
        let hits = page.find(&Query::new("black cat").unwrap());
        let rects = &hits[0].rects;
        assert_eq!(rects.len(), 2);
        // "black" is chars 4..9 on line 0, at 72 + 4 × 5.
        let first = rects[0];
        assert!((first.x - 92.0).abs() < 0.01, "{first:?}");
        assert!((first.width - 25.0).abs() < 0.01, "{first:?}");
        assert!((first.y - 92.0).abs() < 0.01, "{first:?}");
        assert!((first.height - 10.0).abs() < 0.01, "{first:?}");
        // "cat" on line 1, 14 pt lower.
        let second = rects[1];
        assert!((second.x - 72.0).abs() < 0.01, "{second:?}");
        assert!((second.width - 15.0).abs() < 0.01, "{second:?}");
        assert!((second.y - 106.0).abs() < 0.01, "{second:?}");
    }

    #[test]
    fn hit_on_one_line_has_one_rect() {
        let hits = find(&["the black cat"], "black");
        assert_eq!(hits[0].rects.len(), 1);
    }

    fn bold(hit: &TextHit) -> String {
        hit.context
            .text
            .chars()
            .skip(hit.context.bold.start)
            .take(hit.context.bold.len())
            .collect()
    }

    #[test]
    fn context_has_bold_hit() {
        let words = "lorem ipsum dolor sit amet ".repeat(12);
        let text = format!("{words}needle {words}");
        let hits = find(&[&text], "needle");
        let context = &hits[0].context;
        assert_eq!(bold(&hits[0]), "needle");
        let count = context.text.chars().count();
        assert!((280..=306).contains(&count), "{count}: {:?}", context.text);
        // Cut at word boundaries.
        assert!(!context.text.starts_with(' ') && !context.text.ends_with(' '));
        for word in context.text.split(' ') {
            assert!(
                ["lorem", "ipsum", "dolor", "sit", "amet", "needle"].contains(&word),
                "{word:?} in {:?}",
                context.text
            );
        }
    }

    #[test]
    fn context_at_page_start() {
        let hits = find(&["needle in a haystack"], "needle");
        assert_eq!(hits[0].context.text, "needle in a haystack");
        assert_eq!(hits[0].context.bold, 0..6);
    }

    #[test]
    fn context_with_newline_is_one_line() {
        let hits = find(&["black", "cat"], "black cat");
        assert_eq!(hits[0].context.text, "black cat");
        assert_eq!(bold(&hits[0]), "black cat");
    }
}
