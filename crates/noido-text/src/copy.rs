//! The text a selection copies: lines joined into running text.

use std::ops::Range;

use crate::PageText;
use crate::search::{Hyphen, line_end_hyphen};

/// A step from one line to the next more than this many times the page's
/// usual step is a paragraph break.
const PARAGRAPH_STEP: f32 = 1.4;

/// The text of `pieces`, ranges of characters on pages in document order:
/// lines and pages joined with spaces, a line-end hyphen inside a word
/// removed (or kept for a word that has its own), and a newline where the
/// gap between lines shows a paragraph break.
pub fn copy_text(pieces: &[(&PageText, Range<usize>)]) -> String {
    let mut out = String::new();
    for (page, range) in pieces {
        let text = piece(page, range.clone());
        if let Some(first) = text.chars().next() {
            let sep = join(&mut out, first);
            out.push_str(sep);
        }
        out.push_str(&text);
    }
    collapse(&out)
}

/// What goes between `out` and a piece starting with `first`: nothing
/// after a line-end hyphen (which goes if it breaks a word), else a space.
fn join(out: &mut String, first: char) -> &'static str {
    let tail: Vec<char> = out.chars().rev().take(2).collect();
    let [hyphen, before] = tail[..] else {
        return if out.is_empty() { "" } else { " " };
    };
    match line_end_hyphen(&[before, hyphen, '\n', first], 1) {
        Some((Hyphen::Breaks, _)) => {
            out.pop();
            ""
        }
        Some((Hyphen::Belongs, _)) => "",
        None => " ",
    }
}

/// `range` of `page` with its line breaks turned into spaces, newlines
/// (paragraphs) or nothing (after a hyphen inside a word).
fn piece(page: &PageText, range: Range<usize>) -> String {
    let chars: Vec<char> = page.text().chars().collect();
    let usual = usual_step(page);
    let mut out = String::new();
    let mut i = range.start;
    while i < range.end.min(chars.len()) {
        let c = chars[i];
        if let Some((kind, next)) = line_end_hyphen(&chars, i)
            && next < range.end
        {
            if matches!(kind, Hyphen::Belongs) {
                out.push(c);
            }
            i = next;
            continue;
        }
        if c == '\n' {
            let step = match (page.line_rect_of(i), page.line_rect_of(i + 1)) {
                (Some(a), Some(b)) => b.y - a.y,
                _ => 0.0,
            };
            let paragraph = usual.is_some_and(|u| step > PARAGRAPH_STEP * u);
            out.push(if paragraph { '\n' } else { ' ' });
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// The median step between the tops of the page's consecutive lines.
fn usual_step(page: &PageText) -> Option<f32> {
    let tops: Vec<f32> = page.lines().into_iter().flatten().map(|r| r.y).collect();
    let mut steps: Vec<f32> = tops
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|s| *s > 0.0)
        .collect();
    steps.sort_by(f32::total_cmp);
    steps.get(steps.len() / 2).copied()
}

/// Whitespace runs as one space, or one newline if they hold one; trimmed.
fn collapse(text: &str) -> String {
    let mut out = String::new();
    let mut gap: Option<char> = None;
    for c in text.chars() {
        if c.is_whitespace() {
            if c == '\n' || gap.is_none() {
                gap = Some(if c == '\n' { '\n' } else { gap.unwrap_or(' ') });
            }
            continue;
        }
        if let Some(g) = gap.take()
            && !out.is_empty()
        {
            out.push(g);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{line, page};

    fn all(page: &PageText) -> String {
        copy_text(&[(page, 0..page.char_count())])
    }

    #[test]
    fn lines_join_with_spaces() {
        assert_eq!(
            all(&PageText::new(page(&["the cat", "sat down"]))),
            "the cat sat down"
        );
    }

    #[test]
    fn line_end_hyphen_rejoins() {
        assert_eq!(
            all(&PageText::new(page(&["inter-", "preter"]))),
            "interpreter"
        );
        assert_eq!(
            all(&PageText::new(page(&["Anglo-", "Saxon"]))),
            "Anglo-Saxon"
        );
        assert_eq!(all(&PageText::new(page(&["word¬", "end"]))), "wordend");
    }

    #[test]
    fn paragraph_gap_is_newline() {
        let mut glyphs = Vec::new();
        for (text, y) in [
            ("one", 100.0),
            ("two", 114.0),
            ("three", 128.0),
            ("four", 160.0),
            ("five", 174.0),
        ] {
            glyphs.extend(line(text, 72.0, y, 10.0));
        }
        assert_eq!(all(&PageText::new(glyphs)), "one two three\nfour five");
    }

    #[test]
    fn pages_join() {
        let a = PageText::new(page(&["end of page"]));
        let b = PageText::new(page(&["next page"]));
        assert_eq!(
            copy_text(&[(&a, 0..a.char_count()), (&b, 0..b.char_count())]),
            "end of page next page"
        );
        let a = PageText::new(page(&["inter-"]));
        let b = PageText::new(page(&["preter"]));
        assert_eq!(
            copy_text(&[(&a, 0..a.char_count()), (&b, 0..b.char_count())]),
            "interpreter"
        );
        let a = PageText::new(page(&["Anglo-"]));
        let b = PageText::new(page(&["Saxon"]));
        assert_eq!(
            copy_text(&[(&a, 0..a.char_count()), (&b, 0..b.char_count())]),
            "Anglo-Saxon"
        );
    }

    #[test]
    fn partial_ranges() {
        let p = PageText::new(page(&["the cat sat"]));
        assert_eq!(copy_text(&[(&p, 4..7)]), "cat");
    }

    #[test]
    fn spaces_collapse_and_trim() {
        let p = PageText::new(page(&["the  cat "]));
        assert_eq!(all(&p), "the cat");
    }
}
