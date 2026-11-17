//! Shaping paragraphs with parley and setting their lines.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use noido_blocks::Style;
use parley::{FontContext, FontFamily, FontStyle, FontWeight, LayoutContext, StyleProperty};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

use crate::fonts::{FaceId, Family, fonts};
use crate::house::{BODY, CODE_SCALE, HEADING, INDENT_EM, LEADING, MEASURE};
use crate::kp::{self, INFINITY, Item};
use crate::paragraphs::{Notes, Para, Role};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SetGlyph {
    pub face: FaceId,
    pub id: u16,
    pub size: f32,
    /// From the line's left edge.
    pub x: f32,
    pub advance: f32,
    /// Baseline shift (footnote numbers), 0 otherwise.
    pub rise: f32,
    /// What it stands for: " " for a space, "\u{ad}" for an added hyphen.
    pub text: String,
    /// Bytes of the paragraph's drawn text; `None` for a prefix and an
    /// added hyphen.
    pub drawn: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decoration {
    Underline,
    Strike,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SetLine {
    pub glyphs: Vec<SetGlyph>,
    /// Kind, x0, x1.
    pub decorations: Vec<(Decoration, f32, f32)>,
    /// Destination, x0, x1.
    pub links: Vec<(String, f32, f32)>,
    pub height: f32,
    pub ascent: f32,
}

/// Footnote numbers are set at this share of the size, raised by this
/// share of it.
const NOTE_SCALE: f32 = 0.7;
const NOTE_RISE: f32 = 0.33;
/// The cost of a hyphenation point.
const HYPHEN_PENALTY: i32 = 50;
/// A word shorter than this many characters is not hyphenated.
const MIN_HYPHENATED: usize = 5;
/// The cost of breaking inside a word wider than the measure (R17):
/// more than any ordinary break, below a forbidden one.
const EMERGENCY_PENALTY: i32 = 5000;
/// The slack, per cluster, the breaker sees in such a word.
const EMERGENCY_STRETCH: f32 = 1.0;

/// The shaping contexts for one layout pass: keep one and set every
/// paragraph with it, so that parley's caches are shared.
pub(crate) struct Shaper {
    font_cx: FontContext,
    layout_cx: LayoutContext<()>,
}

impl Shaper {
    pub(crate) fn new() -> Self {
        Shaper {
            font_cx: fonts().context(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// The lines of a paragraph at the house measure.
    pub(crate) fn set(&mut self, para: &Para, notes: &Notes, lang: hypher::Lang) -> Vec<SetLine> {
        self.set_width(para, notes, lang, MEASURE)
    }

    #[cfg(test)]
    pub(crate) fn set_at(
        &mut self,
        para: &Para,
        notes: &Notes,
        lang: hypher::Lang,
        width: f32,
    ) -> Vec<SetLine> {
        self.set_width(para, notes, lang, width)
    }

    fn set_width(
        &mut self,
        para: &Para,
        notes: &Notes,
        lang: hypher::Lang,
        width: f32,
    ) -> Vec<SetLine> {
        let (family, size, bold) = match para.role {
            Role::Body { .. } => (Family::Body, BODY, false),
            Role::Heading(level) => {
                let level = usize::from(level.clamp(1, 6)) - 1;
                (Family::Heading, HEADING[level], true)
            }
            Role::Code => (Family::Code, BODY * CODE_SCALE, false),
        };
        let shape = Shape::new(para, notes);
        let layout = self.shape(&shape, para, family, size, bold);
        let metrics = Metrics::of(&layout, size);
        let atoms = atoms_of(&layout, &shape.text);

        let ctx = Ctx {
            para,
            shape: &shape,
            size,
            lang,
            width,
            raised: shape
                .reps
                .iter()
                .zip(&shape.kinds)
                .filter(|(_, k)| **k == RepKind::Note)
                .map(|(r, _)| r.shape.clone())
                .collect(),
        };
        let mut lines = Vec::new();
        for (n, segment) in segments(&atoms).into_iter().enumerate() {
            let atoms = &atoms[segment];
            match para.role {
                Role::Code => ctx.set_code(atoms, &mut lines),
                Role::Heading(_) => ctx.set_text(atoms, false, true, &mut lines),
                Role::Body { indent } => ctx.set_text(atoms, indent && n == 0, false, &mut lines),
            }
        }
        for line in &mut lines {
            line.height = metrics.height;
            line.ascent = metrics.ascent;
            ctx.decorate(line);
        }
        lines
    }

    fn shape(
        &mut self,
        shape: &Shape,
        para: &Para,
        family: Family,
        size: f32,
        bold: bool,
    ) -> parley::Layout<()> {
        let text = shape.text.as_str();
        let mut b = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(family.stack())));
        b.push_default(StyleProperty::FontSize(size));
        if bold {
            b.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
        }
        for styled in &para.text.styles {
            let range = shape.pos(styled.drawn.start)..shape.pos(styled.drawn.end);
            if range.is_empty() {
                continue;
            }
            match styled.style {
                Style::Emphasis | Style::Image { .. } => {
                    b.push(StyleProperty::FontStyle(FontStyle::Italic), range)
                }
                Style::Strong => b.push(StyleProperty::FontWeight(FontWeight::BOLD), range),
                Style::Code => {
                    b.push(
                        StyleProperty::FontFamily(FontFamily::from(Family::Code.stack())),
                        range.clone(),
                    );
                    b.push(StyleProperty::FontSize(size * CODE_SCALE), range);
                }
                Style::FootnoteRef { .. } => {
                    b.push(StyleProperty::FontSize(size * NOTE_SCALE), range)
                }
                Style::Strikethrough | Style::Link { .. } | Style::TaskBox { .. } => {}
            }
        }
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepKind {
    Image,
    Task,
    Note,
}

/// A stand-in for drawn text: the shaped string holds `shape` where the
/// drawn text has `drawn`.
struct Replacement {
    drawn: Range<usize>,
    shape: Range<usize>,
}

/// What is shaped: the prefix, then the drawn text with its stand-ins.
struct Shape {
    text: String,
    prefix: usize,
    reps: Vec<Replacement>,
    kinds: Vec<RepKind>,
}

impl Shape {
    fn new(para: &Para, notes: &Notes) -> Self {
        let drawn = &para.text.string;
        let mut stand_ins: Vec<(Range<usize>, String, RepKind)> = Vec::new();
        for styled in &para.text.styles {
            let (text, kind) = match &styled.style {
                Style::Image { alt, .. } if alt.is_empty() => {
                    ("[image]".to_string(), RepKind::Image)
                }
                Style::Image { alt, .. } => (alt.clone(), RepKind::Image),
                Style::TaskBox { checked } => (
                    (if *checked { "\u{2611}" } else { "\u{2610}" }).to_string(),
                    RepKind::Task,
                ),
                Style::FootnoteRef { label } => (
                    notes
                        .0
                        .get(label)
                        .map_or_else(|| label.clone(), usize::to_string),
                    RepKind::Note,
                ),
                _ => continue,
            };
            stand_ins.push((styled.drawn.clone(), text, kind));
        }
        stand_ins.sort_by_key(|s| s.0.start);

        let mut text = para.prefix.clone().unwrap_or_default();
        let prefix = text.len();
        let (mut reps, mut kinds) = (Vec::new(), Vec::new());
        let mut at = 0;
        for (range, stand_in, kind) in stand_ins {
            if range.start < at {
                continue; // overlapping: the first wins
            }
            text.push_str(&drawn[at..range.start]);
            let start = text.len();
            text.push_str(&stand_in);
            reps.push(Replacement {
                drawn: range.clone(),
                shape: start..text.len(),
            });
            kinds.push(kind);
            at = range.end;
        }
        text.push_str(&drawn[at..]);
        Shape {
            text,
            prefix,
            reps,
            kinds,
        }
    }

    /// The position in the shaped string of a position in the drawn text.
    fn pos(&self, drawn: usize) -> usize {
        match self.reps.iter().rev().find(|r| r.drawn.end <= drawn) {
            Some(r) => r.shape.end + (drawn - r.drawn.end),
            None => self.prefix + drawn,
        }
    }

    /// The drawn text a stretch of the shaped string stands for.
    fn drawn(&self, shaped: &Range<usize>) -> Option<Range<usize>> {
        if shaped.start < self.prefix {
            return None;
        }
        if let Some(r) = self
            .reps
            .iter()
            .find(|r| r.shape.start <= shaped.start && shaped.start < r.shape.end)
        {
            // The stand-in's first glyph carries the whole of it.
            return (shaped.start == r.shape.start).then(|| r.drawn.clone());
        }
        let back = |p: usize| match self.reps.iter().rev().find(|r| r.shape.end <= p) {
            Some(r) => r.drawn.end + (p - r.shape.end),
            None => p - self.prefix,
        };
        Some(back(shaped.start)..back(shaped.end))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Space,
    Newline,
    Other,
}

/// A grapheme cluster as shaped, ligatures merged into one.
struct Atom {
    range: Range<usize>,
    advance: f32,
    kind: Kind,
    face: FaceId,
    font_index: u32,
    size: f32,
    /// Glyph id, offset from the atom's left, advance.
    glyphs: Vec<(u16, f32, f32)>,
}

fn glyph_id(id: u32) -> u16 {
    u16::try_from(id).unwrap_or(0)
}

fn atoms_of(layout: &parley::Layout<()>, text: &str) -> Vec<Atom> {
    let mut atoms: Vec<Atom> = Vec::new();
    for line in layout.lines() {
        for run in line.runs() {
            let font = run.font();
            let face = fonts().face_of(font.data.data());
            debug_assert!(face.is_some(), "a shaped run uses a bundled face");
            let face = face.unwrap_or(FaceId(0));
            for c in run.clusters() {
                if c.is_ligature_continuation()
                    && let Some(last) = atoms.last_mut()
                {
                    last.range.end = c.text_range().end;
                    last.advance += c.advance();
                    continue;
                }
                let mut pen = 0.0;
                let glyphs = c
                    .glyphs()
                    .map(|g| {
                        let at = (glyph_id(g.id), pen + g.x, g.advance);
                        pen += g.advance;
                        at
                    })
                    .collect();
                let kind = if c.is_hard_line_break() {
                    Kind::Newline
                } else if c.is_space_or_nbsp() && &text[c.text_range()] != "\u{a0}" {
                    Kind::Space
                } else {
                    Kind::Other
                };
                atoms.push(Atom {
                    range: c.text_range(),
                    advance: c.advance(),
                    kind,
                    face,
                    font_index: font.index,
                    size: run.font_size(),
                    glyphs,
                });
            }
        }
    }
    atoms
}

/// The runs of atoms between hard line breaks. A final break ends the
/// last line, it does not start another.
fn segments(atoms: &[Atom]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, atom) in atoms.iter().enumerate() {
        if atom.kind == Kind::Newline {
            out.push(from..i);
            from = i + 1;
        }
    }
    if from < atoms.len() || out.is_empty() {
        out.push(from..atoms.len());
    }
    out
}

struct Metrics {
    height: f32,
    ascent: f32,
}

impl Metrics {
    /// The line box: a multiple of the leading, the text centred in it.
    fn of(layout: &parley::Layout<()>, size: f32) -> Self {
        let (mut ascent, mut descent) = (0.0f32, 0.0f32);
        for run in layout.lines().flat_map(|l| l.runs().collect::<Vec<_>>()) {
            ascent = ascent.max(run.font_metrics().ascent);
            descent = descent.max(run.font_metrics().descent);
        }
        if ascent + descent == 0.0 {
            (ascent, descent) = (size * 0.8, size * 0.2);
        }
        let height = LEADING * (size / LEADING).ceil().max(1.0);
        Metrics {
            height,
            ascent: (height - (ascent + descent)) / 2.0 + ascent,
        }
    }
}

/// The hyphen of a font at a size: its glyph and advance.
/// What a line broken at a penalty ends with.
#[derive(Clone, Copy)]
enum LineEnd {
    /// An added hyphen (glyph id, advance) after atom `n`.
    Hyphen(u16, f32, usize),
    /// A break mark after atom `n`.
    Mark(usize),
}

/// A break inside a word that has no hyphen to show (R17, and a wrapped
/// code line): a soft hyphen with no width, so that copy and search join
/// the word again. It is drawn as the font's space, which shows nothing.
fn break_mark(atom: &Atom, x: f32) -> Option<SetGlyph> {
    let font = FontRef::from_index(fonts().data(atom.face), atom.font_index).ok()?;
    let id = font.charmap().map(' ')?;
    Some(SetGlyph {
        face: atom.face,
        id: glyph_id(id.to_u32()),
        size: atom.size,
        x,
        advance: 0.0,
        rise: 0.0,
        text: "\u{ad}".to_string(),
        drawn: None,
    })
}

fn hyphen_of(atom: &Atom) -> Option<(u16, f32)> {
    let font = FontRef::from_index(fonts().data(atom.face), atom.font_index).ok()?;
    let id = font.charmap().map('-')?;
    let advance = font
        .glyph_metrics(Size::new(atom.size), LocationRef::default())
        .advance_width(id)?;
    Some((glyph_id(id.to_u32()), advance))
}

struct Ctx<'a> {
    para: &'a Para,
    shape: &'a Shape,
    /// The paragraph's size, the em of a footnote's rise.
    size: f32,
    lang: hypher::Lang,
    width: f32,
    /// Shaped ranges raised as footnote numbers.
    raised: Vec<Range<usize>>,
}

impl Ctx<'_> {
    /// One atom's glyphs at `x`. A space takes the `advance` it was given.
    fn emit(&self, atom: &Atom, x: f32, advance: Option<f32>, out: &mut Vec<SetGlyph>) {
        let text = &self.shape.text[atom.range.clone()];
        let drawn = self.shape.drawn(&atom.range);
        let rise = if self.raised.iter().any(|r| r.contains(&atom.range.start)) {
            NOTE_RISE * self.size
        } else {
            0.0
        };
        let glyph = |i: usize, id: u16, dx: f32, adv: f32| SetGlyph {
            face: atom.face,
            id,
            size: atom.size,
            x: x + dx,
            advance: adv,
            rise,
            text: if i == 0 {
                text.to_string()
            } else {
                String::new()
            },
            drawn: if i == 0 { drawn.clone() } else { None },
        };
        if let Some(advance) = advance {
            let id = atom.glyphs.first().map_or(0, |g| g.0);
            out.push(glyph(0, id, 0.0, advance));
        } else if atom.glyphs.is_empty() {
            // Whatever shaping did, the text is kept: as .notdef.
            out.push(glyph(0, 0, 0.0, atom.advance));
        } else {
            for (i, &(id, dx, adv)) in atom.glyphs.iter().enumerate() {
                out.push(glyph(i, id, dx, adv));
            }
        }
    }

    /// Where a hyphen may go: inside words by hypher's patterns, except
    /// in code; none in a heading.
    fn soft_points(&self) -> HashSet<usize> {
        let mut points = HashSet::new();
        let text = &self.shape.text;
        let code: Vec<Range<usize>> = self
            .para
            .text
            .styles
            .iter()
            .filter(|s| s.style == Style::Code)
            .map(|s| self.shape.pos(s.drawn.start)..self.shape.pos(s.drawn.end))
            .collect();
        let mut word_start = None;
        for (i, ch) in text
            .char_indices()
            .chain(std::iter::once((text.len(), ' ')))
        {
            if ch.is_alphabetic() {
                word_start.get_or_insert(i);
                continue;
            }
            let Some(start) = word_start.take() else {
                continue;
            };
            let word = &text[start..i];
            let in_code = code.iter().any(|c| start < c.end && i > c.start);
            if start < self.shape.prefix || in_code || word.chars().count() < MIN_HYPHENATED {
                continue;
            }
            let mut at = start;
            let mut syllables = hypher::hyphenate(word, self.lang).peekable();
            while let Some(syllable) = syllables.next() {
                at += syllable.len();
                if syllables.peek().is_some() {
                    points.insert(at);
                }
            }
        }
        points
    }

    /// The atoms (by index) that continue a word wider than the measure:
    /// the places an emergency break may go.
    fn overlong_boundaries(&self, atoms: &[Atom]) -> HashSet<usize> {
        let mut out = HashSet::new();
        let mut from = 0;
        for end in 0..=atoms.len() {
            if end < atoms.len() && atoms[end].kind != Kind::Space {
                continue;
            }
            let width: f32 = atoms[from..end].iter().map(|a| a.advance).sum();
            if width > self.width {
                out.extend(from + 1..end);
            }
            from = end + 1;
        }
        out
    }

    /// Justified (or ragged) Knuth–Plass lines of one run of atoms.
    fn set_text(&self, atoms: &[Atom], indent: bool, ragged: bool, lines: &mut Vec<SetLine>) {
        let soft = if ragged {
            HashSet::new()
        } else {
            self.soft_points()
        };
        let text = &self.shape.text;
        // Trailing spaces hang: they are set, with no width, after the
        // last word.
        let end = atoms
            .iter()
            .rposition(|a| a.kind != Kind::Space)
            .map_or(0, |i| i + 1);

        let mut items: Vec<Item> = Vec::new();
        let mut ends: Vec<Option<LineEnd>> = Vec::new();
        let mut push = |item: Item, end: Option<LineEnd>| {
            items.push(item);
            ends.push(end);
        };
        if indent {
            push(
                Item::Box {
                    width: INDENT_EM * self.size,
                    cluster: None,
                },
                None,
            );
        }
        let overlong = self.overlong_boundaries(&atoms[..end]);
        let mut hyphen_cache: HashMap<(u16, u32, u32), Option<(u16, f32)>> = HashMap::new();
        for (n, atom) in atoms[..end].iter().enumerate() {
            if n > 0 && atoms[n - 1].kind != Kind::Space && atom.kind != Kind::Space {
                let previous = &atoms[n - 1];
                let start = atom.range.start;
                let before = text[..start].chars().next_back();
                if soft.contains(&start) {
                    let key = (
                        previous.face.0,
                        previous.font_index,
                        previous.size.to_bits(),
                    );
                    let hyphen = *hyphen_cache
                        .entry(key)
                        .or_insert_with(|| hyphen_of(previous));
                    if let Some((id, advance)) = hyphen {
                        push(
                            Item::Penalty {
                                width: advance,
                                penalty: HYPHEN_PENALTY,
                                flagged: true,
                            },
                            Some(LineEnd::Hyphen(id, advance, n - 1)),
                        );
                    }
                } else if matches!(before, Some('-' | '\u{2013}' | '\u{2014}')) {
                    push(
                        Item::Penalty {
                            width: 0.0,
                            penalty: 0,
                            flagged: false,
                        },
                        None,
                    );
                } else if overlong.contains(&n) {
                    // Inside a word wider than the measure: break at any
                    // cluster, at a price no ordinary break comes near.
                    push(
                        Item::Penalty {
                            width: 0.0,
                            penalty: EMERGENCY_PENALTY,
                            flagged: false,
                        },
                        Some(LineEnd::Mark(n - 1)),
                    );
                }
                if overlong.contains(&n) {
                    // Slack for the breaker to judge such a line by; it is
                    // not set (a glue with no cluster adds no space).
                    push(
                        Item::Glue {
                            width: 0.0,
                            stretch: EMERGENCY_STRETCH,
                            shrink: 0.0,
                            cluster: None,
                        },
                        None,
                    );
                }
            }
            let item = if atom.kind == Kind::Space {
                Item::Glue {
                    width: atom.advance,
                    stretch: atom.advance * 0.5,
                    shrink: atom.advance / 3.0,
                    cluster: Some(n),
                }
            } else {
                Item::Box {
                    width: atom.advance,
                    cluster: Some(n),
                }
            };
            push(item, None);
        }
        push(
            Item::Penalty {
                width: 0.0,
                penalty: INFINITY,
                flagged: false,
            },
            None,
        );
        push(
            Item::Glue {
                width: 0.0,
                stretch: 10_000.0,
                shrink: 0.0,
                cluster: None,
            },
            None,
        );
        push(
            Item::Penalty {
                width: 0.0,
                penalty: -INFINITY,
                flagged: true,
            },
            None,
        );

        let breaks = kp::breaks(&items, self.width, &kp::Params::default());
        let mut from = 0;
        for (k, &(to, ratio)) in breaks.iter().enumerate() {
            let last = k + 1 == breaks.len();
            // A last or ragged line is not stretched; shrinking is allowed
            // so that it still fits.
            let ratio = if last || ragged {
                ratio.min(0.0)
            } else {
                ratio
            };
            let mut glyphs = Vec::new();
            let mut x = 0.0;
            for item in &items[kp::line_start(&items, from).min(to)..to] {
                match *item {
                    Item::Box { width, cluster } => {
                        if let Some(n) = cluster {
                            self.emit(&atoms[n], x, None, &mut glyphs);
                        }
                        x += width;
                    }
                    Item::Glue {
                        width,
                        stretch,
                        shrink,
                        cluster,
                    } => {
                        let width = width + ratio * if ratio >= 0.0 { stretch } else { shrink };
                        if let Some(n) = cluster {
                            self.emit(&atoms[n], x, Some(width), &mut glyphs);
                            x += width;
                        }
                    }
                    Item::Penalty { .. } => {}
                }
            }
            match ends.get(to).copied().flatten() {
                Some(LineEnd::Hyphen(id, advance, n)) => {
                    let at = &atoms[n];
                    glyphs.push(SetGlyph {
                        face: at.face,
                        id,
                        size: at.size,
                        x,
                        advance,
                        rise: 0.0,
                        text: "\u{ad}".to_string(),
                        drawn: None,
                    });
                }
                Some(LineEnd::Mark(n)) => glyphs.extend(break_mark(&atoms[n], x)),
                None => {}
            }
            // The spaces the break swallowed hang at the end of the line.
            let next = kp::line_start(&items, to).max(to);
            for item in &items[to..next.min(items.len())] {
                if let Item::Glue {
                    cluster: Some(n), ..
                } = item
                {
                    self.emit(&atoms[*n], x, Some(0.0), &mut glyphs);
                }
            }
            if last {
                for atom in &atoms[end..] {
                    self.emit(atom, x, Some(0.0), &mut glyphs);
                }
            }
            lines.push(SetLine {
                glyphs,
                ..SetLine::default()
            });
            from = to;
        }
    }

    /// Code: the line as it is, wrapped at any character when too long.
    fn set_code(&self, atoms: &[Atom], lines: &mut Vec<SetLine>) {
        let mut glyphs = Vec::new();
        let mut x = 0.0;
        for (n, atom) in atoms.iter().enumerate() {
            if x > 0.0 && x + atom.advance > self.width + 0.01 {
                // Copy joins the line to the next, unless the next starts
                // with a space (a mark there would be kept as text).
                if atom.kind != Kind::Space {
                    glyphs.extend(break_mark(&atoms[n - 1], x));
                }
                lines.push(SetLine {
                    glyphs: std::mem::take(&mut glyphs),
                    ..SetLine::default()
                });
                x = 0.0;
            }
            self.emit(atom, x, None, &mut glyphs);
            x += atom.advance;
        }
        lines.push(SetLine {
            glyphs,
            ..SetLine::default()
        });
    }

    /// Underlines, strike-throughs and link areas over the line's glyphs.
    fn decorate(&self, line: &mut SetLine) {
        for styled in &self.para.text.styles {
            let kinds: &[Decoration] = match styled.style {
                Style::Strikethrough => &[Decoration::Strike],
                Style::Link { .. } => &[Decoration::Underline],
                _ => continue,
            };
            let mut extent: Option<(f32, f32)> = None;
            for g in &line.glyphs {
                let Some(d) = &g.drawn else { continue };
                if d.start >= styled.drawn.start && d.end <= styled.drawn.end {
                    let (x0, x1) = extent.unwrap_or((g.x, g.x + g.advance));
                    extent = Some((x0.min(g.x), x1.max(g.x + g.advance)));
                }
            }
            let Some((x0, x1)) = extent else { continue };
            for &kind in kinds {
                line.decorations.push((kind, x0, x1));
            }
            if let Style::Link { dest } = &styled.style {
                line.links.push((dest.clone(), x0, x1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::house::{CODE_SCALE, MEASURE};
    use crate::paragraphs::paragraphs;
    use hypher::Lang;
    use noido_blocks::markdown::parse;

    const LONG: &str = "The quick brown fox jumps over the lazy dog while the information \
        age continues to produce documentation about internationalization and \
        representation, which nobody reads completely; nevertheless, typographers \
        insist that wonderfully even paragraphs matter, because readers notice \
        rivers of white space even when they cannot name them. A second sentence \
        follows, with enough words to fill several lines of a narrow page, and a \
        third one finishes the thought about extraordinarily careful line breaking.";

    fn set_md(md: &str) -> Vec<SetLine> {
        let (paras, notes) = paragraphs(&parse(md));
        Shaper::new().set(&paras[0], &notes, Lang::English)
    }

    fn set_md_at(md: &str, width: f32) -> Vec<SetLine> {
        let (paras, notes) = paragraphs(&parse(md));
        Shaper::new().set_at(&paras[0], &notes, Lang::English, width)
    }

    fn text_of(line: &SetLine) -> String {
        line.glyphs.iter().map(|g| g.text.as_str()).collect()
    }

    fn end_of(line: &SetLine) -> f32 {
        let g = line.glyphs.last().expect("a glyph");
        g.x + g.advance
    }

    #[test]
    fn a_long_paragraph_is_justified_to_the_measure() {
        let lines = set_md(LONG);
        assert!(lines.len() > 3);
        for line in &lines[..lines.len() - 1] {
            assert!(
                (end_of(line) - MEASURE).abs() < 0.5,
                "{}: {}",
                text_of(line),
                end_of(line)
            );
        }
        assert!(end_of(lines.last().unwrap()) < MEASURE);
    }

    #[test]
    fn spaces_are_glyphs_and_other_gaps_stay_under_a_tenth_em() {
        for line in set_md(LONG) {
            for pair in line.glyphs.windows(2) {
                let space = pair[0].text == " " || pair[1].text == " ";
                let gap = pair[1].x - (pair[0].x + pair[0].advance);
                assert!(space || gap < 0.1 * pair[0].size, "{pair:?}");
            }
        }
        let all: String = set_md(LONG).iter().map(text_of).collect();
        assert_eq!(all.matches(' ').count(), LONG.matches(' ').count());
    }

    #[test]
    fn an_added_hyphen_is_a_soft_hyphen_without_source() {
        let lines = set_md_at(
            "Internationalization representation extraordinarily documentation \
             typographers wonderfully",
            60.0,
        );
        let hyphenated: Vec<_> = lines
            .iter()
            .filter_map(|l| l.glyphs.last())
            .filter(|g| g.text == "\u{ad}")
            .collect();
        assert!(!hyphenated.is_empty(), "{lines:#?}");
        assert!(hyphenated.iter().all(|g| g.drawn.is_none()));
    }

    #[test]
    fn inline_code_is_not_hyphenated() {
        let lines = set_md_at(
            "`Internationalization` `representation` `documentation`",
            60.0,
        );
        // No hyphen shows; a word wider than the line may still break,
        // with a mark that has no width.
        assert!(
            lines
                .iter()
                .flat_map(|l| &l.glyphs)
                .all(|g| g.text != "\u{ad}" || g.advance == 0.0)
        );
        // Whereas the same words in the body are.
        let body = set_md_at("Internationalization representation documentation", 60.0);
        assert!(
            body.iter()
                .flat_map(|l| &l.glyphs)
                .any(|g| g.text == "\u{ad}")
        );
    }

    #[test]
    fn a_word_wider_than_the_measure_is_broken_at_the_measure_without_loss() {
        let url = "https://example.org/a/very/long/path/that/never/ends/and/keeps/going/on/and/on/\
                   until/it/is/far/wider/than/the/measure/of/any/book/page/ever";
        let md = format!("See {url} now.");
        let lines = set_md(&md);
        assert!(lines.len() > 2);
        assert!(
            lines.iter().all(|l| end_of(l) <= MEASURE + 0.01),
            "{lines:#?}"
        );
        let (paras, _) = paragraphs(&parse(&md));
        let drawn = &paras[0].text.string;
        let mut covered = vec![false; drawn.len()];
        for g in lines.iter().flat_map(|l| &l.glyphs) {
            if let Some(r) = &g.drawn {
                covered[r.clone()].fill(true);
            }
        }
        assert!(covered.iter().all(|&c| c), "{covered:?}");
    }

    #[test]
    fn a_character_no_font_has_is_kept() {
        let lines = set_md("a \u{1F642} b");
        assert!(
            lines
                .iter()
                .flat_map(|l| &l.glyphs)
                .any(|g| g.text == "\u{1F642}")
        );
    }

    #[test]
    fn code_keeps_its_lines() {
        let (paras, notes) = paragraphs(&parse("```\na\nbb\n```\n"));
        let lines = Shaper::new().set(&paras[0], &notes, Lang::English);
        assert_eq!(lines.len(), 2, "{lines:#?}");
        assert_eq!(text_of(&lines[0]), "a");
        assert_eq!(text_of(&lines[1]), "bb");
        assert!((lines[0].glyphs[0].size - 11.0 * CODE_SCALE).abs() < 1e-3);
    }

    #[test]
    fn a_long_code_line_wraps_at_the_measure_at_any_character() {
        let long = "x".repeat(100);
        let (paras, notes) = paragraphs(&parse(&format!("```\n{long}\n```\n")));
        let lines = Shaper::new().set(&paras[0], &notes, Lang::English);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| end_of(l) <= MEASURE + 0.01));
        let total: usize = lines.iter().map(|l| l.glyphs.len()).sum();
        // Each wrap ends its line with a break mark.
        assert_eq!(total, 100 + lines.len() - 1);
        for line in &lines[..lines.len() - 1] {
            let mark = line.glyphs.last().unwrap();
            assert_eq!((mark.text.as_str(), mark.advance), ("\u{ad}", 0.0));
        }
    }

    #[test]
    fn a_footnote_reference_is_raised_and_smaller() {
        let lines = set_md("Word[^n] more.\n\n[^n]: Note.\n");
        let glyphs: Vec<_> = lines.iter().flat_map(|l| &l.glyphs).collect();
        let note = glyphs.iter().find(|g| g.text == "1").expect("the number");
        let body = glyphs.iter().find(|g| g.text == "W").unwrap();
        assert!((note.size - 0.7 * body.size).abs() < 1e-3);
        assert!((note.rise - 0.33 * body.size).abs() < 1e-3);
        assert_eq!(body.rise, 0.0);
    }

    #[test]
    fn the_first_line_is_indented_one_em_when_asked() {
        let (paras, notes) = paragraphs(&parse("One.\n\nTwo words here.\n"));
        let mut shaper = Shaper::new();
        let first = shaper.set(&paras[0], &notes, Lang::English);
        let second = shaper.set(&paras[1], &notes, Lang::English);
        assert!(first[0].glyphs[0].x.abs() < 1e-3);
        assert!((second[0].glyphs[0].x - 11.0).abs() < 1e-3);
    }

    #[test]
    fn a_prefix_is_drawn_without_source() {
        let (paras, notes) = paragraphs(&parse("- item\n"));
        let lines = Shaper::new().set(&paras[0], &notes, Lang::English);
        assert_eq!(text_of(&lines[0]), "\u{2022} item");
        let g = &lines[0].glyphs;
        assert!(g[0].drawn.is_none() && g[1].drawn.is_none());
        assert_eq!(g[2].drawn, Some(0..1));
    }

    #[test]
    fn styles_choose_faces() {
        let lines = set_md("plain *em* **st**");
        let face = |text: &str| {
            lines[0]
                .glyphs
                .iter()
                .find(|g| g.text == text)
                .unwrap()
                .face
        };
        for text in ["p", "e", "s"] {
            let family = noido_fonts::faces()[usize::from(face(text).0)].family;
            assert_eq!(family, "Libron");
        }
        assert_ne!(face("p"), face("e"));
        assert_ne!(face("p"), face("s"));
        assert_ne!(face("e"), face("s"));
    }

    #[test]
    fn strikethrough_and_links_decorate_their_glyphs() {
        let lines = set_md("a ~~gone~~ b [link](http://x.org) c");
        let line = &lines[0];
        assert_eq!(line.decorations.len(), 2, "{line:#?}");
        let strike = line
            .decorations
            .iter()
            .find(|d| d.0 == Decoration::Strike)
            .unwrap();
        let g = line.glyphs.iter().find(|g| g.text == "g").unwrap();
        assert!((strike.1 - g.x).abs() < 1e-3 && strike.2 > strike.1);
        assert_eq!(line.links.len(), 1);
        assert_eq!(line.links[0].0, "http://x.org");
        let l = line.glyphs.iter().find(|g| g.text == "l").unwrap();
        assert!((line.links[0].1 - l.x).abs() < 1e-3);
    }

    #[test]
    fn an_image_is_its_alt_text_and_a_task_box_a_box() {
        let lines = set_md("![a cat](c.png) end");
        assert!(text_of(&lines[0]).starts_with("a cat end"));
        let drawn: Vec<_> = lines[0].glyphs[..5]
            .iter()
            .map(|g| g.drawn.clone())
            .collect();
        assert_eq!(drawn, [Some(0..3), None, None, None, None]);
        let lines = set_md("- [x] done\n");
        assert!(text_of(&lines[0]).contains('\u{2611}'));
    }

    #[test]
    fn drawn_ranges_never_overlap_and_cover_the_text() {
        let md = "An office *em* [l](http://u.org) ![a cat](c.png) text[^n][^m] `code` \
                  word  \nnext internationalization representation.\n\n\
                  - [x] task with ![](x.png) a documentation\n\n\
                  1. extraordinarily wonderfully\n\n[^n]: Note one.\n\n[^m]: Note two.\n";
        let (paras, notes) = paragraphs(&parse(md));
        let mut shaper = Shaper::new();
        for width in [60.0, 120.0, MEASURE] {
            for para in &paras {
                let lines = shaper.set_at(para, &notes, Lang::English, width);
                let mut covered = vec![false; para.text.string.len()];
                let mut end = 0;
                for g in lines.iter().flat_map(|l| &l.glyphs) {
                    let Some(r) = &g.drawn else { continue };
                    assert!(r.start >= end, "{r:?} after {end}: {lines:#?}");
                    end = r.end;
                    covered[r.clone()].fill(true);
                }
                for (i, &c) in covered.iter().enumerate() {
                    assert!(
                        c || para.text.string.as_bytes()[i] == b'\n',
                        "byte {i} of {:?} at {width}",
                        para.text.string
                    );
                }
            }
        }
    }

    #[test]
    fn a_no_break_space_neither_breaks_nor_stretches() {
        let md = "aaaa bbbb\u{a0}cccc";
        let lines = set_md_at(md, 60.0);
        let text: Vec<String> = lines.iter().map(text_of).collect();
        assert!(
            text.iter().any(|t| t.contains("bbbb\u{a0}cccc")),
            "{text:?}"
        );
        let nbsp: Vec<_> = lines
            .iter()
            .flat_map(|l| &l.glyphs)
            .filter(|g| g.text == "\u{a0}")
            .collect();
        assert_eq!(nbsp.len(), 1);
        // Not stretched: as wide as when set alone on a loose last line.
        let alone = set_md("bbbb\u{a0}cccc");
        let natural = alone[0].glyphs.iter().find(|g| g.text == "\u{a0}").unwrap();
        assert!((nbsp[0].advance - natural.advance).abs() < 1e-3);
    }

    #[test]
    fn headings_are_bold_sans_and_ragged() {
        let (paras, notes) = paragraphs(&parse("## A heading\n"));
        let lines = Shaper::new().set(&paras[0], &notes, Lang::English);
        let g = &lines[0].glyphs[0];
        let face = &noido_fonts::faces()[usize::from(g.face.0)];
        assert_eq!(face.family, "Libertinus Sans");
        assert_eq!(face.weight, 700);
        assert!((g.size - 14.0).abs() < 1e-3);

        let long = "## The very long heading of a chapter which surely needs several lines to \
                    be set in a narrow measure\n";
        let (paras, notes) = paragraphs(&parse(long));
        let lines = Shaper::new().set(&paras[0], &notes, Lang::English);
        assert!(lines.len() > 1);
        assert!(
            lines[..lines.len() - 1]
                .iter()
                .any(|l| end_of(l) < MEASURE - 1.0)
        );
    }

    #[test]
    fn a_hard_break_ends_a_line() {
        let lines = set_md("one  \ntwo\n");
        assert_eq!(lines.len(), 2, "{lines:#?}");
    }

    #[test]
    fn a_ligature_is_one_glyph_with_all_its_characters() {
        let lines = set_md("office");
        let ligature = lines[0].glyphs.iter().find(|g| g.text.chars().count() > 1);
        assert!(ligature.is_some(), "{lines:#?}");
    }

    #[test]
    fn a_line_has_a_height_and_an_ascent_within_it() {
        let lines = set_md("Text.");
        assert_eq!(lines[0].height, 14.0);
        assert!(lines[0].ascent > 0.0 && lines[0].ascent < 14.0);
    }
}
