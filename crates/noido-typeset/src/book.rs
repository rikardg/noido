//! A markdown file typeset: its pages, their glyphs, where the text came from.

use std::ops::Range;

use hypher::Lang;
use noido_blocks::Text;
use noido_blocks::markdown::parse;
use noido_engine::{Glyph, LocationMap, Point, Quad, Run};
use noido_text::{PageText, on_page};
use unicode_normalization::UnicodeNormalization;

use crate::house::{LEADING, LEFT, LINES, PAPER, TOP};
use crate::lines::{SetGlyph, SetLine, Shaper};
use crate::links::{Marks, marks};
use crate::pages::{Flow, paginate};
use crate::paragraphs::{Para, Role, front_matter, paragraphs};
use crate::runs::runs;

/// Where a glyph's box reaches above and below its baseline, in ems: the
/// PDF engine's.
const ASCENT: f32 = 0.8;
const DESCENT: f32 = 0.2;

/// A typeset file: the glyphs of each page, and the map from their
/// characters to the source.
pub struct Book {
    pages: Vec<Vec<Glyph>>,
    map: LocationMap,
    pub(crate) marks: Marks,
}

/// What a page is made of while it is built.
#[derive(Default)]
struct Page {
    glyphs: Vec<Glyph>,
    /// Per glyph, the source character of each of its characters
    /// (empty for a glyph without text).
    sources: Vec<Vec<Option<usize>>>,
    /// Per glyph, the top and bottom of its line.
    spans: Vec<(f32, f32)>,
    /// Lines placed so far: the next line's run.
    lines: u32,
}

pub fn typeset(source: &str) -> Book {
    let tree = parse(source);
    let front = front_matter(&tree, source);
    let lang = language(front.lang.as_deref());
    let (paras, notes) = paragraphs(&tree);
    let mut shaper = Shaper::new();
    let set: Vec<Vec<SetLine>> = paras
        .iter()
        .map(|para| {
            let mut lines = shaper.set(para, &notes, lang);
            if lines.is_empty() {
                // A blank line, so the paragraph (a rule) keeps its place.
                lines.push(SetLine {
                    height: LEADING,
                    ..SetLine::default()
                });
            }
            lines
        })
        .collect();
    let flows: Vec<Flow> = paras.iter().zip(&set).map(|(p, l)| flow(p, l)).collect();
    let (placed, count) = paginate(&flows, TOP, LINES as f32 * LEADING);

    let mut pages: Vec<Page> = (0..count).map(|_| Page::default()).collect();
    for at in &placed {
        let line = &set[at.flow][at.line];
        let text = &paras[at.flow].text;
        let page = &mut pages[at.page];
        page.glyphs.extend(glyphs_of(line, at.y, page.lines));
        page.lines += 1;
        for g in &line.glyphs {
            page.sources.push(if g.text.is_empty() {
                Vec::new()
            } else {
                glyph_chars(text, g)
            });
            page.spans.push((at.y, at.y + line.height));
        }
    }

    let mut map_pages = Vec::with_capacity(count);
    let mut glyph_pages = Vec::with_capacity(count);
    for page in pages {
        map_pages.push(page_runs(&page));
        glyph_pages.push(page.glyphs);
    }
    Book {
        pages: glyph_pages,
        map: LocationMap::from_placed(map_pages),
        marks: marks(&paras, &set, &placed, count, &front),
    }
}

/// The runs of a page, read off the characters `PageText` makes of its
/// glyphs, so that the two agree by construction: each character takes
/// the next source of the glyph it came from. A character `PageText`
/// inserted has none, nor has one past the end of its glyph's sources
/// (a mark it joined into the previous glyph leaves that one with more
/// characters than were counted).
///
/// The glyphs are those the window shows (`on_page`), the same set it makes
/// its `PageText` of, so that they agree even if something overflows.
fn page_runs(page: &Page) -> Vec<(usize, Run)> {
    let shown: Vec<usize> = (0..page.glyphs.len())
        .filter(|&i| {
            let one = vec![page.glyphs[i].clone()];
            !on_page(one, PAPER.width, PAPER.height).is_empty()
        })
        .collect();
    let text = PageText::new(shown.iter().map(|&i| page.glyphs[i].clone()).collect());
    let mut next = vec![0; page.glyphs.len()];
    let mut out = Vec::new();
    let mut line: Vec<Option<usize>> = Vec::new();
    let mut first = 0;
    let mut span = (0.0, 0.0);
    for c in 0..text.char_count() {
        if c > 0 && text.line_of(c) != text.line_of(c - 1) {
            flush(&mut out, &line, first, span);
            first = c;
            line.clear();
        }
        let source = text.glyph_of(c).map(|g| shown[g]).and_then(|g| {
            span = page.spans[g];
            let source = page.sources[g].get(next[g]).copied().flatten();
            next[g] += 1;
            source
        });
        line.push(source);
    }
    flush(&mut out, &line, first, span);
    out
}

fn flush(out: &mut Vec<(usize, Run)>, line: &[Option<usize>], first: usize, span: (f32, f32)) {
    for (at, run) in runs(line, span.0, span.1) {
        out.push((first + at, run));
    }
}

impl Book {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The glyphs of `page` in reading order.
    pub fn glyphs(&self, page: usize) -> Vec<Glyph> {
        self.pages[page].clone()
    }

    /// Part 0 is the markdown file.
    pub fn locations(&self) -> LocationMap {
        self.map.clone()
    }
}

/// The front matter's language by its first two letters; English when it
/// has none or hypher has no patterns for it.
fn language(lang: Option<&str>) -> Lang {
    let bytes = lang.unwrap_or_default().as_bytes();
    match bytes {
        [a, b, ..] if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => {
            Lang::from_iso([a.to_ascii_lowercase(), b.to_ascii_lowercase()])
                .unwrap_or(Lang::English)
        }
        _ => Lang::English,
    }
}

/// How a paragraph's lines flow: body and code lines are one leading
/// each; a heading's are its own, with room around it.
fn flow(para: &Para, lines: &[SetLine]) -> Flow {
    match para.role {
        Role::Body { .. } | Role::Code => Flow {
            lines: vec![LEADING; lines.len()],
            before: 0.0,
            after: 0.0,
            heading: false,
            new_page: para.new_page,
        },
        Role::Heading(level) => Flow {
            lines: lines.iter().map(|l| l.height).collect(),
            before: match level {
                1 => 0.0,
                2 => LEADING,
                _ => LEADING / 2.0,
            },
            after: LEADING / 2.0,
            heading: true,
            new_page: para.new_page,
        },
    }
}

/// A line's glyphs on the page, the line's top at `top`. A glyph's box
/// runs on over the glyphs after it that have no text of their own, so
/// that no gap shows between it and the next glyph with text.
/// The line is one run (`run`): a typeset line has no letter-spacing.
fn glyphs_of(line: &SetLine, top: f32, run: u32) -> Vec<Glyph> {
    let baseline = top + line.ascent;
    line.glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mut end = g.x + g.advance;
            if !g.text.is_empty() {
                for next in line.glyphs[i + 1..]
                    .iter()
                    .take_while(|n| n.text.is_empty())
                {
                    end = end.max(next.x + next.advance);
                }
            }
            let (x0, x1) = (LEFT + g.x, LEFT + end);
            let y = baseline - g.rise;
            let (above, below) = (y - ASCENT * g.size, y + DESCENT * g.size);
            Glyph {
                text: g.text.clone(),
                origin: Point { x: x0, y },
                size: g.size,
                quad: Quad {
                    corners: [
                        Point { x: x0, y: below },
                        Point { x: x1, y: below },
                        Point { x: x1, y: above },
                        Point { x: x0, y: above },
                    ],
                },
                invisible: false,
                run,
            }
        })
        .collect()
}

/// The page characters a glyph gives (its text after NFC, as
/// `PageText` counts it), each with its source character.
fn glyph_chars(text: &Text, glyph: &SetGlyph) -> Vec<Option<usize>> {
    let shown = glyph.text.nfc().count();
    let Some(range) = glyph.drawn.clone().filter(|r| !r.is_empty()) else {
        return vec![None; shown];
    };
    let drawn = &text.string[range.clone()];
    if glyph.text == drawn && shown == drawn.chars().count() {
        return one_by_one(text, range);
    }
    // Text that is not what was drawn (an image's alt text, a footnote's
    // number, a composed accent): a stretch from the source start, no
    // longer than the source it stands for.
    let start = text.source_of(range.start);
    let len = source_len(text, &range, start);
    (0..shown).map(|i| (i < len).then_some(start + i)).collect()
}

/// The source character of each character of the drawn `range`.
fn one_by_one(text: &Text, range: Range<usize>) -> Vec<Option<usize>> {
    let segments = &text.segments;
    let mut i = segments.partition_point(|s| s.drawn <= range.start) - 1;
    let mut k = text.string[segments[i].drawn..range.start].chars().count();
    let mut exact = one_to_one(text, i);
    let mut out = Vec::new();
    for (b, _) in text.string[range.clone()].char_indices() {
        let at = range.start + b;
        while at >= segment_end(text, i) {
            i += 1;
            k = 0;
            exact = one_to_one(text, i);
        }
        let segment = &segments[i];
        out.push((exact || k < segment.source.len()).then_some(segment.source.start + k));
        k += 1;
    }
    out
}

/// How many source characters the drawn `range` covers, from `start`.
fn source_len(text: &Text, range: &Range<usize>, start: usize) -> usize {
    let segments = &text.segments;
    let last = segments.partition_point(|s| s.drawn < range.end) - 1;
    let segment = &segments[last];
    let end = if one_to_one(text, last) {
        segment.source.start + text.string[segment.drawn..range.end].chars().count()
    } else {
        segment.source.end
    };
    end.saturating_sub(start)
}

fn segment_end(text: &Text, i: usize) -> usize {
    text.segments
        .get(i + 1)
        .map_or(text.string.len(), |next| next.drawn)
}

fn one_to_one(text: &Text, i: usize) -> bool {
    let segment = &text.segments[i];
    text.string[segment.drawn..segment_end(text, i)]
        .chars()
        .count()
        == segment.source.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paragraphs::paragraphs;
    use noido_text::copy_text;

    fn glyph_at(text: &str, x: f32) -> Glyph {
        let (x0, x1, y0, y1) = (x, x + 6.0, 100.0, 110.0);
        Glyph {
            text: text.to_string(),
            origin: Point { x: x0, y: y1 },
            size: 10.0,
            quad: Quad {
                corners: [
                    Point { x: x0, y: y1 },
                    Point { x: x1, y: y1 },
                    Point { x: x1, y: y0 },
                    Point { x: x0, y: y0 },
                ],
            },
            invisible: false,
            run: 0,
        }
    }

    #[test]
    fn runs_leave_out_glyphs_the_window_does_not_show() {
        let page = Page {
            glyphs: vec![
                glyph_at("a", 10.0),
                glyph_at("b", 1000.0),
                glyph_at("c", 16.0),
            ],
            sources: vec![vec![Some(0)], vec![Some(1)], vec![Some(2)]],
            spans: vec![(100.0, 110.0); 3],
            lines: 0,
        };
        let offsets: Vec<usize> = page_runs(&page).iter().map(|(at, _)| *at).collect();
        assert_eq!(offsets, [0, 1]);
    }

    #[test]
    fn each_line_on_a_page_is_its_own_run_counted_from_0() {
        let book = typeset("# Title\n\nOne paragraph.\n\nAnother one.\n");
        let glyphs = book.glyphs(0);
        let mut lines: Vec<(f32, u32)> = Vec::new();
        for g in &glyphs {
            match lines.last() {
                Some(&(y, run)) if y == g.origin.y => assert_eq!(g.run, run),
                _ => lines.push((g.origin.y, g.run)),
            }
        }
        let runs: Vec<u32> = lines.iter().map(|&(_, run)| run).collect();
        assert_eq!(runs, [0, 1, 2]);
    }

    #[test]
    fn an_added_hyphen_copies_away() {
        let (paras, notes) = paragraphs(&parse(
            "An internationalization of representation, extraordinarily.\n",
        ));
        let lines = Shaper::new().set_at(&paras[0], &notes, Lang::English, 120.0);
        let hyphenated = lines
            .iter()
            .any(|l| l.glyphs.iter().any(|g| g.text == "\u{ad}"));
        assert!(hyphenated, "{lines:#?}");
        let glyphs: Vec<Glyph> = lines
            .iter()
            .enumerate()
            .flat_map(|(i, l)| glyphs_of(l, TOP + i as f32 * LEADING, i as u32))
            .collect();
        let page = PageText::with_marked_hyphens(glyphs);
        let copied = copy_text(&[(&page, 0..page.char_count())]);
        assert_eq!(
            copied,
            "An internationalization of representation, extraordinarily."
        );
    }
}
