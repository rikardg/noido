//! A markdown file typeset: its pages, their glyphs, where the text came from.

use std::collections::HashMap;
use std::ops::Range;

use hypher::Lang;
use noido_blocks::markdown::parse;
use noido_blocks::{Text, Tree};
use noido_engine::{Glyph, Location, LocationMap, PdfBytes, Point, Quad, Run};
use noido_epub::{Epub, PartKind};
use noido_text::{PageText, on_page};
use unicode_normalization::UnicodeNormalization;

use crate::draw::{Outlines, PageInk};
use crate::house::{LEADING, LEFT, LINES, PAPER, TOP};
use crate::images::{Images, natural};
use crate::lines::{SetGlyph, SetLine, Shaper};
use crate::links::{Marks, epub_marks, marks};
use crate::pages::{Flow, Placed, paginate};
use crate::paragraphs::{Notes, Para, Role, front_matter, no_images, paragraphs};
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
    /// What to draw on each page.
    pub(crate) ink: Vec<PageInk>,
    /// Glyph outlines, drawn once and kept.
    pub(crate) outlines: Outlines,
    /// The images the pages show, to draw from.
    pub(crate) images: Images,
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
    /// Per glyph, the source character an image stands at (an image has
    /// no characters of its own); `None` for other glyphs.
    points: Vec<Option<usize>>,
    /// Lines placed so far: the next line's run.
    lines: u32,
    ink: PageInk,
}

/// What one part holds for setting.
pub(crate) enum Content<'a> {
    /// A part's blocks; for an EPUB's part, its path in the archive, which
    /// its images are found from.
    Blocks(&'a Tree, Option<&'a str>),
    /// Paragraphs drawn without source (a title page, a notice).
    Drawn(Vec<Para>),
}

/// One part as it was set: its paragraphs and lines, and where each
/// line went (pages counted from the book's first).
pub(crate) struct SetPart {
    pub paras: Vec<Para>,
    pub set: Vec<Vec<SetLine>>,
    pub placed: Vec<Placed>,
}

/// A book set part by part, joined.
pub(crate) struct SetBook {
    pages: Vec<Vec<Glyph>>,
    map: LocationMap,
    ink: Vec<PageInk>,
    pub parts: Vec<SetPart>,
}

pub fn typeset(source: &str) -> Book {
    let tree = parse(source);
    let front = front_matter(&tree, source);
    let lang = language(front.lang.as_deref());
    let mut images = Images::none();
    let set = set_parts(&[Content::Blocks(&tree, None)], lang, &mut images);
    let part = &set.parts[0];
    let marks = marks(
        &part.paras,
        &part.set,
        &part.placed,
        set.pages.len(),
        &front,
    );
    Book {
        pages: set.pages,
        map: set.map,
        marks,
        ink: set.ink,
        outlines: Outlines::default(),
        images,
    }
}

/// An EPUB typeset: each spine item a part, the cover a title page, an
/// unreadable item a notice; links and outline resolved through ids and
/// the table of contents.
pub fn typeset_epub(epub: &Epub, bytes: PdfBytes) -> Book {
    let contents: Vec<Content> = epub
        .parts
        .iter()
        .map(|part| match &part.kind {
            PartKind::Text(x) => Content::Blocks(&x.tree, Some(&part.path)),
            PartKind::Cover { .. } => Content::Drawn(title_page(epub)),
            PartKind::Image | PartKind::Unreadable => Content::Drawn(vec![drawn(
                Role::Body { indent: false },
                "This chapter could not be read.",
            )]),
        })
        .collect();
    let mut images = Images::of_epub(bytes);
    let set = set_parts(&contents, epub_language(epub), &mut images);
    let marks = epub_marks(epub, &set.parts, &set.map, set.pages.len());
    Book {
        pages: set.pages,
        map: set.map,
        marks,
        ink: set.ink,
        outlines: Outlines::default(),
        images,
    }
}

/// A paragraph drawn without source.
fn drawn(role: Role, string: &str) -> Para {
    Para {
        role,
        text: Text {
            string: string.to_string(),
            styles: vec![],
            segments: vec![],
        },
        prefix: None,
        new_page: false,
        note: None,
        images: Vec::new(),
    }
}

/// The title as a level-1 heading (else the first file's name), the
/// authors below it.
fn title_page(epub: &Epub) -> Vec<Para> {
    let title = epub.meta.title.clone().unwrap_or_else(|| {
        let name = epub.parts.first().map_or("", |p| p.path.as_str());
        let name = name.rsplit('/').next().unwrap_or(name);
        name.rsplit_once('.')
            .map_or(name, |(stem, _)| stem)
            .to_string()
    });
    let mut paras = vec![drawn(Role::Heading(1), &title)];
    if !epub.meta.creators.is_empty() {
        paras.push(drawn(
            Role::Body { indent: false },
            &epub.meta.creators.join(", "),
        ));
    }
    paras
}

/// The package's language; English when it has none.
fn epub_language(epub: &Epub) -> Lang {
    language(epub.meta.language.as_deref())
}

/// Sets each part alone, each starting on a new page, its runs in its
/// own part number; joins them into one book.
pub(crate) fn set_parts(parts: &[Content], lang: Lang, images: &mut Images) -> SetBook {
    let mut shaper = Shaper::new();
    let mut glyph_pages: Vec<Vec<Glyph>> = Vec::new();
    let mut map_pages: Vec<Vec<(usize, Run)>> = Vec::new();
    let mut ink: Vec<PageInk> = Vec::new();
    let mut out = Vec::with_capacity(parts.len());
    for (number, content) in parts.iter().enumerate() {
        let (paras, notes, drawn) = match content {
            Content::Blocks(tree, Some(path)) => {
                let (paras, notes) = paragraphs(tree, &mut |src, width, height| {
                    let src = src.split('#').next().unwrap_or_default();
                    let (id, px) = images.size(&noido_epub::resolve(path, src))?;
                    Some((id, natural(px, width, height)?))
                });
                (paras, notes, false)
            }
            Content::Blocks(tree, None) => {
                let (paras, notes) = paragraphs(tree, &mut no_images);
                (paras, notes, false)
            }
            Content::Drawn(paras) => (paras.clone(), Notes(HashMap::new()), true),
        };
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
        let (mut placed, count) = paginate(&flows, TOP, LINES as f32 * LEADING);

        let mut pages: Vec<Page> = (0..count).map(|_| Page::default()).collect();
        for at in &placed {
            let line = &set[at.flow][at.line];
            let text = &paras[at.flow].text;
            let page = &mut pages[at.page];
            page.glyphs.extend(glyphs_of(line, at.y, page.lines));
            page.lines += 1;
            page.ink.add_line(line, at.y);
            for g in &line.glyphs {
                page.sources.push(if g.text.is_empty() {
                    Vec::new()
                } else if drawn {
                    // Drawn text has no source.
                    vec![None; g.text.nfc().count()]
                } else {
                    glyph_chars(text, g)
                });
                page.spans.push((at.y, at.y + line.height));
                // A figure is a run of no characters at its place; an image
                // in a line has no place of its own, as its stand-in had not.
                let figure = matches!(paras[at.flow].role, Role::Figure { .. });
                page.points.push(match (&g.image, &g.drawn) {
                    (Some(_), Some(range)) if !drawn && figure => Some(text.source_of(range.start)),
                    _ => None,
                });
            }
        }

        let first_page = glyph_pages.len();
        let mut part_runs: Vec<Vec<(usize, Run)>> =
            pages.iter().map(|page| page_runs(page, number)).collect();
        if part_runs.iter().all(|runs| runs.is_empty()) {
            // A place for the part all the same, at its first line.
            let (top, bottom) = placed
                .iter()
                .find(|at| at.page == 0)
                .map_or((TOP, TOP + LEADING), |at| {
                    (at.y, at.y + set[at.flow][at.line].height)
                });
            part_runs[0].push((
                0,
                Run {
                    start: Location {
                        part: number,
                        char: 0,
                    },
                    chars: 0,
                    top,
                    bottom,
                },
            ));
        }
        map_pages.extend(part_runs);
        for page in pages {
            glyph_pages.push(page.glyphs);
            ink.push(page.ink);
        }
        for at in &mut placed {
            at.page += first_page;
        }
        out.push(SetPart { paras, set, placed });
    }
    SetBook {
        pages: glyph_pages,
        map: LocationMap::from_placed(map_pages),
        ink,
        parts: out,
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
fn page_runs(page: &Page, part: usize) -> Vec<(usize, Run)> {
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
            flush(&mut out, &line, first, span, part);
            first = c;
            line.clear();
        }
        let source = text.glyph_of(c).map(|g| shown[g]).and_then(|g| {
            span = page.spans[g];
            if let Some(char) = page.points[g].filter(|_| next[g] == 0) {
                // An image: a run of no characters at its place.
                out.push((
                    c,
                    Run {
                        start: Location { part, char },
                        chars: 0,
                        top: span.0,
                        bottom: span.1,
                    },
                ));
            }
            let source = page.sources[g].get(next[g]).copied().flatten();
            next[g] += 1;
            source
        });
        line.push(source);
    }
    flush(&mut out, &line, first, span, part);
    out
}

fn flush(
    out: &mut Vec<(usize, Run)>,
    line: &[Option<usize>],
    first: usize,
    span: (f32, f32),
    part: usize,
) {
    for (at, run) in runs(line, part, span.0, span.1) {
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
        Role::Figure {
            height, heading, ..
        } => Flow {
            lines: vec![height],
            before: LEADING / 2.0,
            after: LEADING / 2.0,
            heading: heading.is_some(),
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
            if let Some((_, height)) = g.image {
                // The image's rectangle.
                // On the baseline: a figure's ascent is its height.
                let bottom = baseline - g.rise;
                let top = bottom - height;
                return Glyph {
                    text: g.text.clone(),
                    origin: Point { x: x0, y: bottom },
                    size: g.size,
                    quad: Quad {
                        corners: [
                            Point { x: x0, y: bottom },
                            Point { x: x1, y: bottom },
                            Point { x: x1, y: top },
                            Point { x: x0, y: top },
                        ],
                    },
                    invisible: false,
                    run,
                };
            }
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
            points: vec![None; 3],
            lines: 0,
            ink: PageInk::default(),
        };
        let offsets: Vec<usize> = page_runs(&page, 0).iter().map(|(at, _)| *at).collect();
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
    fn each_part_starts_a_page_and_counts_its_own_characters() {
        let (one, two) = (parse("One.\n"), parse("Two.\n"));
        let book = set_parts(
            &[Content::Blocks(&one, None), Content::Blocks(&two, None)],
            Lang::English,
            &mut Images::none(),
        );
        assert_eq!(book.pages.len(), 2);
        let map = &book.map;
        assert_eq!(map.location(0, 0), Some(Location { part: 0, char: 0 }));
        assert_eq!(map.location(1, 0), Some(Location { part: 1, char: 0 }));
        assert!(book.parts[1].placed.iter().all(|at| at.page == 1));
    }

    #[test]
    fn a_part_without_text_still_has_a_place() {
        let one = parse("One.\n");
        let drawn = Para {
            role: Role::Body { indent: false },
            text: Text {
                string: "A title page".to_string(),
                styles: vec![],
                segments: vec![],
            },
            prefix: None,
            new_page: false,
            note: None,
            images: Vec::new(),
        };
        let book = set_parts(
            &[Content::Blocks(&one, None), Content::Drawn(vec![drawn])],
            Lang::English,
            &mut Images::none(),
        );
        assert_eq!(book.pages.len(), 2);
        let map = &book.map;
        assert_eq!(map.page_start(1), Some(Location { part: 1, char: 0 }));
        let chars = PageText::new(book.pages[1].clone()).char_count();
        assert!(chars > 0);
        for c in 0..chars {
            let at = map.location(1, c);
            assert!(at.is_none() || at == Some(Location { part: 1, char: 0 }));
        }
    }

    #[test]
    fn an_epub_is_set_in_its_language_and_english_without_one() {
        let language_of = |name: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures")
                .join(name);
            let epub = noido_epub::open(&std::fs::read(path).unwrap(), 8 << 20).unwrap();
            epub_language(&epub)
        };
        assert_eq!(language_of("book.epub"), Lang::Swedish);
        assert_eq!(language_of("book2.epub"), Lang::English);
    }

    #[test]
    fn markdown_images_keep_their_stand_in() {
        let book = typeset("![a cat](cat.png)\n");
        let text: String = book.glyphs(0).iter().map(|g| g.text.as_str()).collect();
        assert_eq!(text, "a cat");
        assert!(book.ink.iter().all(|ink| ink.images.is_empty()));
        // Set in italic: another face than the same words as plain text.
        let face_of = |md: &str| {
            let (paras, notes) = paragraphs(&parse(md), &mut no_images);
            let lines = Shaper::new().set(&paras[0], &notes, Lang::English);
            assert!(lines[0].glyphs.iter().all(|g| g.image.is_none()));
            lines[0].glyphs[0].face
        };
        assert_ne!(face_of("![a cat](cat.png)\n"), face_of("a cat\n"));
    }

    #[test]
    fn an_added_hyphen_copies_away() {
        let (paras, notes) = paragraphs(
            &parse("An internationalization of representation, extraordinarily.\n"),
            &mut no_images,
        );
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
