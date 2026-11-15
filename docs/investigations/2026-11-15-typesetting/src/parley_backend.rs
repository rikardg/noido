//! parley for shaping. Lines broken two ways: parley's own breaker (greedy,
//! justified by spreading the slack over the spaces), or Knuth–Plass over
//! parley's measured clusters with `hypher` hyphenation.

use std::sync::Arc;

use parley::fontique::Blob;
use parley::{
    Alignment, AlignmentOptions, FontData, FontFamily, FontFamilyName, FontStyle, FontWeight,
    IndentOptions, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
};

use crate::doc::*;
use crate::kp::{self, INFINITY, Item};
use crate::raster::{Canvas, glyph_for};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Breaking {
    Greedy,
    KnuthPlass,
}

#[derive(Clone)]
pub struct Run {
    pub font: FontData,
    pub size: f32,
    /// Glyph id and its pen position from the line start, on the baseline.
    pub glyphs: Vec<(u32, f32, f32)>,
}

#[derive(Clone, Default)]
pub struct Line {
    pub runs: Vec<Run>,
    /// How far the spaces were stretched (positive) or shrunk (negative),
    /// as a share of what they may: the Knuth–Plass adjustment ratio, with
    /// stretch half a space and shrink a third, for both breakers.
    pub ratio: f32,
    pub hyphenated: bool,
    pub last: bool,
}

pub enum Set {
    Heading(Line),
    Para(Vec<Line>),
}

pub struct Parley {
    font_cx: parley::FontContext,
    layout_cx: LayoutContext<()>,
}

const FAMILY: &str = "Libertinus Serif";

impl Parley {
    pub fn new(font_files: &[Vec<u8>]) -> Self {
        let mut font_cx = parley::FontContext::new();
        for data in font_files {
            font_cx
                .collection
                .register_fonts(Blob::new(Arc::new(data.clone())), None);
        }
        Self {
            font_cx,
            layout_cx: LayoutContext::new(),
        }
    }

    fn layout(&mut self, text: &str, size: f32, italic: &[std::ops::Range<usize>], bold: bool) -> Layout<()> {
        let mut b = self.layout_cx.ranged_builder(&mut self.font_cx, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(FontFamilyName::named(FAMILY))));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::LineHeight(LineHeight::Absolute(LEADING)));
        if bold {
            b.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
        }
        for r in italic {
            b.push(StyleProperty::FontStyle(FontStyle::Italic), r.clone());
        }
        b.build(text)
    }

    pub fn set(&mut self, blocks: &[Block], breaking: Breaking) -> Vec<Set> {
        let mut out = Vec::new();
        let mut indent = false;
        for block in blocks {
            match block {
                Block::Heading(h) => {
                    let mut layout = self.layout(h, HEADING_SIZE, &[], true);
                    layout.break_all_lines(None);
                    out.push(Set::Heading(lines_of(&layout).remove(0)));
                    indent = false;
                }
                Block::Para(p) => {
                    let lines = match breaking {
                        Breaking::Greedy => self.greedy(p, indent),
                        Breaking::KnuthPlass => self.knuth_plass(p, indent),
                    };
                    out.push(Set::Para(lines));
                    indent = true;
                }
            }
        }
        out
    }

    fn greedy(&mut self, p: &Para, indent: bool) -> Vec<Line> {
        let mut layout = self.layout(&p.text, FONT_SIZE, &p.italic, false);
        if indent {
            layout.set_text_indent(INDENT, IndentOptions::default());
        }
        // Broken once unjustified, to measure the lines' natural widths, then
        // again justified (the same breaks) for the glyphs.
        layout.break_all_lines(Some(MEASURE));
        let mut ratios = Vec::new();
        // The ratio parley's justification amounts to, on the same scale as
        // Knuth–Plass: slack over half the natural width of the spaces.
        for (n, line) in layout.lines().enumerate() {
            let mut natural = 0.0;
            let mut spaces = 0.0;
            let clusters: Vec<_> = line.runs().flat_map(|r| r.clusters().collect::<Vec<_>>()).collect();
            let trailing = clusters.iter().rev().take_while(|c| c.is_space_or_nbsp()).count();
            for c in &clusters[..clusters.len() - trailing] {
                natural += c.advance();
                if c.is_space_or_nbsp() {
                    spaces += c.advance();
                }
            }
            let width = MEASURE - if n == 0 && indent { INDENT } else { 0.0 };
            let last = n + 1 == layout.len();
            ratios.push(if last || spaces == 0.0 {
                0.0
            } else if natural <= width {
                (width - natural) / (spaces * 0.5)
            } else {
                (width - natural) / (spaces / 3.0)
            });
        }
        layout.set_alignment(Alignment::Justify, AlignmentOptions::default());
        layout.break_all_lines(Some(MEASURE));
        let mut lines = lines_of(&layout);
        let count = lines.len();
        for (n, (line, ratio)) in lines.iter_mut().zip(ratios).enumerate() {
            line.ratio = ratio;
            line.last = n + 1 == count;
        }
        lines
    }

    fn knuth_plass(&mut self, p: &Para, indent: bool) -> Vec<Line> {
        let mut layout = self.layout(&p.text, FONT_SIZE, &p.italic, false);
        layout.break_all_lines(None);
        struct Cluster {
            range: std::ops::Range<usize>,
            advance: f32,
            space: bool,
            font: FontData,
            size: f32,
            glyphs: Vec<(u32, f32, f32)>,
        }
        let mut clusters = Vec::new();
        for line in layout.lines() {
            for run in line.runs() {
                for c in run.clusters() {
                    let mut pen = 0.0;
                    let glyphs = c
                        .glyphs()
                        .map(|g| {
                            let at = (g.id, pen + g.x, g.y);
                            pen += g.advance;
                            at
                        })
                        .collect();
                    clusters.push(Cluster {
                        range: c.text_range(),
                        advance: c.advance(),
                        space: c.is_space_or_nbsp(),
                        font: run.font().clone(),
                        size: run.font_size(),
                        glyphs,
                    });
                }
            }
        }
        let hyphen = clusters
            .first()
            .and_then(|c| glyph_for(c.font.data.data(), c.font.index, '-', c.size));
        let hyphen_width = hyphen.map_or(0.0, |h| h.1);

        // Where a hyphen may go: inside words, by hypher's patterns, and
        // after hyphens and dashes already in the text.
        let starts: std::collections::HashSet<usize> = clusters.iter().map(|c| c.range.start).collect();
        let mut soft = std::collections::HashSet::new();
        let mut hard = std::collections::HashSet::new();
        let text = &p.text;
        let mut word_start = None;
        for (i, ch) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
            if ch.is_alphabetic() {
                word_start.get_or_insert(i);
            } else {
                if let Some(s) = word_start.take() {
                    let word = &text[s..i];
                    if word.chars().count() >= 5 {
                        let mut at = s;
                        let syllables: Vec<&str> = hypher::hyphenate(word, hypher::Lang::English).collect();
                        for syl in &syllables[..syllables.len() - 1] {
                            at += syl.len();
                            if starts.contains(&at) {
                                soft.insert(at);
                            }
                        }
                    }
                }
                if (ch == '-' || ch == '—') && starts.contains(&(i + ch.len_utf8())) {
                    hard.insert(i + ch.len_utf8());
                }
            }
        }

        let mut items = Vec::new();
        if indent {
            items.push(Item::Box {
                width: INDENT,
                cluster: None,
            });
        }
        for (n, c) in clusters.iter().enumerate() {
            if soft.contains(&c.range.start) {
                items.push(Item::Penalty {
                    width: hyphen_width,
                    penalty: 50,
                    flagged: true,
                });
            } else if hard.contains(&c.range.start) {
                items.push(Item::Penalty {
                    width: 0.0,
                    penalty: 50,
                    flagged: true,
                });
            }
            if c.space {
                items.push(Item::Glue {
                    width: c.advance,
                    stretch: c.advance * 0.5,
                    shrink: c.advance / 3.0,
                    cluster: Some(n),
                });
            } else {
                items.push(Item::Box {
                    width: c.advance,
                    cluster: Some(n),
                });
            }
        }
        // The last line is set at its natural width.
        if matches!(items.last(), Some(Item::Glue { .. })) {
            items.pop();
        }
        items.push(Item::Penalty {
            width: 0.0,
            penalty: INFINITY,
            flagged: false,
        });
        items.push(Item::Glue {
            width: 0.0,
            stretch: 10_000.0,
            shrink: 0.0,
            cluster: None,
        });
        items.push(Item::Penalty {
            width: 0.0,
            penalty: -INFINITY,
            flagged: true,
        });

        let breaks = kp::breaks(&items, MEASURE, &kp::Params::default());
        let mut lines = Vec::new();
        let mut from = 0;
        for (k, &(to, ratio)) in breaks.iter().enumerate() {
            let last = k + 1 == breaks.len();
            let mut runs: Vec<Run> = Vec::new();
            let mut x = 0.0;
            let push = |runs: &mut Vec<Run>, font: &FontData, size: f32, g: (u32, f32, f32)| {
                match runs.last_mut() {
                    Some(r) if r.font.data.id() == font.data.id() && r.font.index == font.index && r.size == size => {
                        r.glyphs.push(g)
                    }
                    _ => runs.push(Run {
                        font: font.clone(),
                        size,
                        glyphs: vec![g],
                    }),
                }
            };
            for item in &items[kp::line_start(&items, from)..to] {
                match *item {
                    Item::Box { width, cluster } => {
                        if let Some(n) = cluster {
                            let c = &clusters[n];
                            for &(id, gx, gy) in &c.glyphs {
                                push(&mut runs, &c.font, c.size, (id, x + gx, gy));
                            }
                        }
                        x += width;
                    }
                    Item::Glue {
                        width,
                        stretch,
                        shrink,
                        ..
                    } => {
                        let r = if last { 0.0 } else { ratio };
                        x += width + if r >= 0.0 { r * stretch } else { r * shrink };
                    }
                    Item::Penalty { .. } => {}
                }
            }
            let hyphenated = matches!(items[to], Item::Penalty { width, flagged: true, .. } if width > 0.0);
            if hyphenated && let (Some(h), Some(c)) = (hyphen, clusters.first()) {
                push(&mut runs, &c.font, c.size, (h.0, x, 0.0));
            }
            lines.push(Line {
                runs,
                ratio: if last { 0.0 } else { ratio },
                hyphenated,
                last,
            });
            from = to;
        }
        lines
    }
}

fn lines_of(layout: &Layout<()>) -> Vec<Line> {
    layout
        .lines()
        .map(|line| {
            let baseline = line.metrics().baseline;
            let mut runs = Vec::new();
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(gr) = item {
                    let run = gr.run();
                    runs.push(Run {
                        font: run.font().clone(),
                        size: run.font_size(),
                        glyphs: gr
                            .positioned_glyphs()
                            .map(|g| (g.id, g.x, g.y - baseline))
                            .collect(),
                    });
                }
            }
            Line {
                runs,
                ..Default::default()
            }
        })
        .collect()
}

/// Pages of lines on a baseline grid: a heading takes a blank line above
/// (except at the top of a page) and below, and keeps two lines of what
/// follows with it; a paragraph leaves no single line at the foot of a page
/// (orphan) or the head of the next (widow).
pub fn paginate(sets: &[Set]) -> Vec<Vec<(Line, usize)>> {
    let mut pages: Vec<Vec<(Line, usize)>> = vec![Vec::new()];
    let mut slot = 0usize;
    let new_page = |pages: &mut Vec<Vec<(Line, usize)>>, slot: &mut usize| {
        pages.push(Vec::new());
        *slot = 0;
    };
    for set in sets {
        match set {
            Set::Heading(line) => {
                let above = if slot == 0 { 0 } else { 2 };
                if slot + above + 1 + 1 + 2 > LINES_PER_PAGE {
                    new_page(&mut pages, &mut slot);
                } else {
                    slot += above;
                }
                pages.last_mut().unwrap().push((line.clone(), slot));
                slot += 2;
            }
            Set::Para(lines) => {
                let mut i = 0;
                while i < lines.len() {
                    let room = LINES_PER_PAGE - slot;
                    let left = lines.len() - i;
                    let mut take = room.min(left);
                    if take < left {
                        // The page fills before the paragraph ends.
                        if take == 1 && i == 0 && left > 1 {
                            take = 0; // orphan
                        } else if left - take == 1 && take > 1 {
                            take -= 1; // widow
                        }
                    }
                    for line in &lines[i..i + take] {
                        pages.last_mut().unwrap().push((line.clone(), slot));
                        slot += 1;
                    }
                    i += take;
                    if i < lines.len() {
                        new_page(&mut pages, &mut slot);
                    }
                }
            }
        }
    }
    pages
}

pub fn render(pages: &[Vec<(Line, usize)>], scale: f32) -> Vec<tiny_skia::Pixmap> {
    pages
        .iter()
        .map(|page| {
            let mut canvas = Canvas::new(PAGE_W, PAGE_H, scale);
            for (line, slot) in page {
                let baseline = MARGIN_TOP + 0.8 * FONT_SIZE + *slot as f32 * LEADING;
                for run in &line.runs {
                    for &(id, x, y) in &run.glyphs {
                        canvas.glyph(run.font.data.data(), run.font.index, id, run.size, MARGIN_X + x, baseline + y);
                    }
                }
            }
            canvas.pixmap
        })
        .collect()
}
