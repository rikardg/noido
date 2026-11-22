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
    pub(crate) folded: Vec<char>,
    /// Whether it has accents, so they must match.
    pub(crate) accented: bool,
}

impl Query {
    /// `None` if `text` has nothing to search for.
    pub fn new(text: &str) -> Option<Query> {
        let chars: Vec<char> = text.trim().chars().collect();
        let folded = fold(&chars, false, |_| false);
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
struct Forms {
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
pub(crate) struct Folds {
    pub(crate) accented: Vec<char>,
    pub(crate) plain: Vec<char>,
    pub(crate) accented_source: Vec<usize>,
    pub(crate) plain_source: Vec<usize>,
}

/// Folds `chars`: lowercase, NFKC, whitespace runs as one space, and with
/// `dehyphenate`, line-end hyphens inside a word removed. `plain` also drops
/// accents. `marked(i)` says whether the hyphens of the page `chars[i]` is
/// on are marked (see [`PageText::with_marked_hyphens`]).
pub(crate) fn fold(chars: &[char], dehyphenate: bool, marked: impl Fn(usize) -> bool) -> Folds {
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
            match line_end_hyphen(chars, i, marked(i)) {
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

pub(crate) enum Hyphen {
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
/// break. On a `marked` page (from layout) `-`, `‐`, `–` and `—` always
/// belong to the word, and join what follows whatever it is (the typesetter
/// breaks "pre-1990" after its hyphen): only `¬` and the soft hyphen
/// break it. There a soft hyphen is always the typesetter's own, and
/// breaks whatever is on either side ("x.org/a" / "/b").
pub(crate) fn line_end_hyphen(chars: &[char], i: usize, marked: bool) -> Option<(Hyphen, usize)> {
    let hyphen = chars[i];
    let added = marked && hyphen == '\u{ad}';
    // Typeset lines also break after a dash, and before anything.
    let dash = marked && matches!(hyphen, '\u{2013}' | '\u{2014}');
    if !(dash || matches!(hyphen, '-' | '\u{2010}' | '¬' | '\u{ad}'))
        || i == 0
        || !(added || chars[i - 1].is_alphabetic())
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
    // On a marked page a dash, a hyphen or a soft hyphen joins anything.
    let any_next = marked && hyphen != '¬' && !next.is_whitespace();
    if !(next.is_alphabetic() || any_next) {
        return None;
    }
    let kind = if matches!(hyphen, '¬' | '\u{ad}') || (!marked && next.is_lowercase()) {
        Hyphen::Breaks
    } else {
        Hyphen::Belongs
    };
    Some((kind, j + 1))
}

impl Forms {
    fn new(text: &str, marked: bool) -> Forms {
        let chars: Vec<char> = text.chars().collect();
        let folds = fold(&chars, true, |_| marked);
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
        // Built for this search and dropped with it: a page holds no forms.
        let forms = Forms::new(self.text(), self.marked_hyphens());
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
            context: context(chars, range.clone(), self.marked_hyphens()),
            range,
        }
    }
}

/// The text around `range`, cut at word boundaries, on one line, as
/// [`copy_text`](crate::copy_text) reads it: whitespace runs as one space,
/// and a line-end hyphen inside a word joined to what follows.
fn context(chars: &[char], range: Range<usize>, marked: bool) -> Context {
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
    let mut text = String::new();
    // The hit's characters in `text`, which drops some characters before it.
    let mut bold = 0..0;
    let mut at = 0;
    let mut i = start;
    while i < end {
        let c = chars[i];
        // A line-end hyphen inside a word takes the break with it; one that
        // belongs to the word keeps its hyphen and takes the break.
        let hyphen = line_end_hyphen(chars, i, marked).filter(|(_, after)| *after < end);
        let next = hyphen.as_ref().map_or(i + 1, |&(_, after)| after);
        if (i..next).contains(&range.start) {
            bold.start = at;
        }
        match (hyphen, c) {
            (Some((Hyphen::Belongs, _)), _) => {
                text.push(c);
                at += 1;
            }
            (Some((Hyphen::Breaks, _)), _) => {}
            (None, c) if c.is_whitespace() => {
                if at > 0 && !text.ends_with(' ') {
                    text.push(' ');
                    at += 1;
                }
            }
            (None, c) => {
                text.push(c);
                at += 1;
            }
        }
        if i < range.end {
            bold.end = at;
        }
        i = next;
    }
    Context { text, bold }
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

    #[test]
    fn hit_rects_are_the_selection_rects() {
        use noido_engine::Rotation;
        let glyphs = page(&["the black", "cat sat"]);
        for rotation in Rotation::ALL {
            let page = PageText::new(crate::testing::turned(glyphs.clone(), rotation));
            let hit = &page.find(&Query::new("black cat").unwrap())[0];
            assert_eq!(hit.rects, page.rects(hit.range.clone()), "{rotation:?}");
        }
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

    #[test]
    fn context_joins_a_word_across_a_line_end_hyphen() {
        let hits = find(&["the spe-", "cifically cat"], "specifically");
        assert_eq!(bold(&hits[0]), "specifically");
        assert_eq!(hits[0].context.text, "the specifically cat");
    }

    #[test]
    fn context_joins_a_hyphen_before_a_trailing_space() {
        // OCR lines end with a space after the hyphen.
        let hits = find(&["the imple- ", "ment cat"], "implement");
        assert_eq!(hits[0].context.text, "the implement cat");
        assert_eq!(bold(&hits[0]), "implement");
    }

    #[test]
    fn context_joins_a_line_end_hyphen_outside_the_hit() {
        let hits = find(&["the phenom-", "enology cat"], "cat");
        assert_eq!(hits[0].context.text, "the phenomenology cat");
        assert_eq!(hits[0].context.bold, 18..21);
    }

    #[test]
    fn context_collapses_runs_of_spaces() {
        let hits = find(&["the  cat  sat"], "cat");
        assert_eq!(hits[0].context.bold, 4..7);
        assert_eq!(hits[0].context.text, "the cat sat");
    }

    #[test]
    fn context_bold_is_exact_after_whitespace_and_hyphen_joining() {
        let hits = find(&["a   b spe-", "cifically cat"], "cat");
        assert_eq!(hits[0].context.text, "a b specifically cat");
        assert_eq!(hits[0].context.bold, 17..20);
    }

    #[test]
    fn context_bold_starts_after_a_joined_break() {
        // The hit begins on the line after the hyphen.
        let hits = find(&["the spe-", "cifically cat"], "cifically");
        assert_eq!(bold(&hits[0]), "cifically");
        assert_eq!(hits[0].context.text, "the specifically cat");
        assert_eq!(hits[0].context.bold, 7..16);
    }

    #[test]
    fn context_bold_ends_at_a_joined_break() {
        // The hit ends on the hyphen; the word after it still joins.
        let hits = find(&["the phenom-", "enology cat"], "phenom");
        assert_eq!(bold(&hits[0]), "phenom");
        assert_eq!(hits[0].context.text, "the phenomenology cat");
        assert_eq!(hits[0].context.bold, 4..10);
    }

    #[test]
    fn context_bold_covers_a_collapsed_run_inside_the_hit() {
        let hits = find(&["the  cat  sat"], "cat sat");
        assert_eq!(bold(&hits[0]), "cat sat");
        assert_eq!(hits[0].context.text, "the cat sat");
        assert_eq!(hits[0].context.bold, 4..11);
    }

    #[test]
    fn context_keeps_a_marked_source_hyphen_at_a_line_end() {
        let p = PageText::with_marked_hyphens(page(&["the well-", "known cat"]));
        let hits = p.find(&Query::new("well-known").unwrap());
        assert_eq!(bold(&hits[0]), "well-known");
        assert_eq!(hits[0].context.text, "the well-known cat");
    }

    #[test]
    fn search_on_a_marked_page_finds_a_source_hyphenated_word() {
        let p = PageText::with_marked_hyphens(page(&["the well-", "known cat"]));
        assert_eq!(p.find(&Query::new("well-known").unwrap()).len(), 1);
        assert!(p.find(&Query::new("wellknown").unwrap()).is_empty());
    }

    #[test]
    fn search_on_a_marked_page_finds_text_across_a_dash_at_a_line_end() {
        let p = PageText::with_marked_hyphens(page(&["a self\u{2014}", "evident cat"]));
        assert_eq!(p.find(&Query::new("self\u{2014}evident").unwrap()).len(), 1);
        let p = PageText::with_marked_hyphens(page(&["the pre-", "1990 cat"]));
        assert_eq!(p.find(&Query::new("pre-1990").unwrap()).len(), 1);
        let p = PageText::new(page(&["a self\u{2014}", "evident cat"]));
        assert!(
            p.find(&Query::new("self\u{2014}evident").unwrap())
                .is_empty()
        );
    }

    #[test]
    fn search_on_a_marked_page_finds_a_word_split_by_a_soft_hyphen() {
        let p = PageText::with_marked_hyphens(page(&["the po\u{ad}", "litical cat"]));
        assert_eq!(p.find(&Query::new("political").unwrap()).len(), 1);
    }

    #[test]
    fn search_on_a_marked_page_finds_text_across_any_soft_hyphen_at_a_line_end() {
        // The typesetter's break inside a word too wide for the line.
        let p = PageText::with_marked_hyphens(page(&["see x.org/a\u{ad}", "/b cat"]));
        assert_eq!(p.find(&Query::new("x.org/a/b").unwrap()).len(), 1);
        let p = PageText::with_marked_hyphens(page(&["id 12\u{ad}", "34 cat"]));
        assert_eq!(p.find(&Query::new("1234").unwrap()).len(), 1);
    }
}
