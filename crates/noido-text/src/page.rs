//! Assembling a page's glyphs into lines and words.

use std::ops::Range;
use std::sync::OnceLock;

pub use noido_engine::Rect;
use noido_engine::{Glyph, Point};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::search::Forms;

/// A move across the baseline further than this many ems starts a new line,
/// as does a move back along it.
const NEW_LINE: f32 = 0.5;
/// A gap wider than this many ems between glyphs is a space.
const SPACE: f32 = 0.1;
/// How far above a target point, in points, the line it names may start:
/// hyperref puts its anchors a little above the text.
pub const LINE_TOLERANCE: f32 = 4.0;

/// A page's text in content-stream order, each character mapped back to the
/// glyph it came from.
#[derive(Debug, Default)]
pub struct PageText {
    /// Lines joined with newlines.
    text: String,
    /// Per character of `text`.
    chars: Vec<CharInfo>,
    /// Per glyph, its bounds.
    bounds: Vec<Rect>,
    /// The forms search matches against, built on the first search.
    pub(crate) forms: OnceLock<Forms>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CharInfo {
    /// The glyph it was drawn with; `None` for inserted spaces and newlines.
    glyph: Option<usize>,
    line: usize,
}

/// A piece of text from one glyph (with any marks joined to it), or
/// inserted between glyphs.
struct Cluster {
    text: String,
    glyph: Option<usize>,
    line: usize,
}

impl PageText {
    pub fn new(glyphs: Vec<Glyph>) -> PageText {
        let bounds = glyphs.iter().map(bounds_of).collect();
        let clusters = assemble(&glyphs);
        let mut text = String::new();
        let mut chars = Vec::new();
        for cluster in clusters {
            for c in cluster.text.nfc() {
                text.push(c);
                chars.push(CharInfo {
                    glyph: cluster.glyph,
                    line: cluster.line,
                });
            }
        }
        PageText {
            text,
            chars,
            bounds,
            forms: OnceLock::new(),
        }
    }

    /// The text, lines joined with newlines.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many `char`s the text has; character indices count these.
    pub fn char_count(&self) -> usize {
        self.chars.len()
    }

    /// The line character `char` is on; a newline is on the line it ends.
    pub fn line_of(&self, char: usize) -> usize {
        self.chars[char].line
    }

    /// The bounds of the glyph character `char` was drawn with; `None` for
    /// inserted spaces and newlines.
    pub fn glyph_bounds(&self, char: usize) -> Option<Rect> {
        self.chars[char].glyph.map(|g| self.bounds[g])
    }

    /// The line a link target at (`x`, `y`) names: of the lines starting
    /// no more than [`LINE_TOLERANCE`] above `y` and reaching past `x`, the
    /// one nearest below, by position on the page. Without `y`, the top
    /// line. Missing coordinates don't constrain.
    pub fn line_near(&self, x: Option<f32>, y: Option<f32>) -> Option<Rect> {
        self.line_rects()
            .into_iter()
            .filter(|r| y.is_none_or(|y| r.y >= y - LINE_TOLERANCE))
            .filter(|r| x.is_none_or(|x| r.x + r.width > x))
            .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
    }

    /// The horizontal extent of the page's glyphs: (left, right).
    pub fn text_span(&self) -> Option<(f32, f32)> {
        let rects = self.line_rects();
        let left = rects.iter().map(|r| r.x).reduce(f32::min)?;
        let right = rects.iter().map(|r| r.x + r.width).reduce(f32::max)?;
        Some((left, right))
    }

    /// Each line's bounds: the union of its glyphs'.
    fn line_rects(&self) -> Vec<Rect> {
        self.lines().into_iter().flatten().collect()
    }

    /// Each line's bounds by line index; `None` for a line without glyphs.
    pub(crate) fn lines(&self) -> Vec<Option<Rect>> {
        let mut lines: Vec<Option<Rect>> = Vec::new();
        for info in &self.chars {
            let Some(glyph) = info.glyph else {
                continue;
            };
            if lines.len() <= info.line {
                lines.resize(info.line + 1, None);
            }
            let b = self.bounds[glyph];
            lines[info.line] = Some(match lines[info.line] {
                None => b,
                Some(r) => union(r, b),
            });
        }
        lines
    }

    /// The line nearest (`x`, `y`). `None` without glyphs.
    fn line_at(&self, x: f32, y: f32) -> Option<usize> {
        self.line_for(x, y).map(|(line, _)| line)
    }

    /// The line for a point at (`x`, `y`), and whether the point is past
    /// its column's first or last line. The column is the one holding the
    /// nearest line; within it, the line is the nearest by height, then by
    /// x. `None` without glyphs.
    fn line_for(&self, x: f32, y: f32) -> Option<(usize, Beyond)> {
        let lines = self.lines();
        let rect = |line: usize| lines[line].expect("a column holds lines with glyphs");
        let gap = |lo: f32, hi: f32, at: f32| (lo - at).max(at - hi).max(0.0);
        let dx = |r: Rect| gap(r.x, r.x + r.width, x);
        let dy = |r: Rect| gap(r.y, r.y + r.height, y);
        let nearest = (0..lines.len())
            .filter(|&l| lines[l].is_some())
            .min_by(|&a, &b| {
                let d = |l| dx(rect(l)).hypot(dy(rect(l)));
                d(a).total_cmp(&d(b))
            })?;
        let column = columns(&lines)
            .into_iter()
            .find(|column| column.contains(&nearest))?;
        let extent = column
            .iter()
            .map(|&l| rect(l))
            .reduce(union)
            .expect("not empty");
        let line = column.into_iter().min_by(|&a, &b| {
            dy(rect(a))
                .total_cmp(&dy(rect(b)))
                .then(dx(rect(a)).total_cmp(&dx(rect(b))))
        })?;
        let beyond = if y < extent.y {
            Beyond::Above
        } else if y > extent.y + extent.height {
            Beyond::Below
        } else {
            Beyond::Within
        };
        Some((line, beyond))
    }

    /// The characters drawn with a glyph on `line`, with their bounds.
    fn glyph_chars(&self, line: usize) -> impl Iterator<Item = (usize, Rect)> + '_ {
        self.chars
            .iter()
            .enumerate()
            .filter(move |(_, info)| info.line == line)
            .filter_map(|(i, info)| info.glyph.map(|g| (i, self.bounds[g])))
    }

    /// The caret (`0..=char_count`) a point at (`x`, `y`) places: on the
    /// nearest line, before the glyph whose horizontal middle is right of
    /// `x`, else after the line's last glyph. Above a column's first line,
    /// at that line's start; below its last line, at that line's end.
    /// `None` without glyphs.
    pub fn caret_at(&self, x: f32, y: f32) -> Option<usize> {
        let (line, beyond) = self.line_for(x, y)?;
        let mut after = None;
        for (i, b) in self.glyph_chars(line) {
            if beyond == Beyond::Above || (beyond == Beyond::Within && x < b.x + b.width / 2.0) {
                return Some(i);
            }
            after = Some(i + 1);
        }
        after
    }

    /// The character under or nearest (`x`, `y`) on the nearest line.
    fn char_at(&self, x: f32, y: f32) -> Option<usize> {
        let line = self.line_at(x, y)?;
        self.glyph_chars(line)
            .map(|(i, b)| (i, (b.x - x).max(x - (b.x + b.width)).max(0.0)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The word at (`x`, `y`): the run of letters, digits, apostrophes and
    /// hyphens around the character there, or that character alone.
    pub fn word_at(&self, x: f32, y: f32) -> Option<Range<usize>> {
        let hit = self.char_at(x, y)?;
        let chars: Vec<char> = self.text.chars().collect();
        let in_word = |c: char| c.is_alphanumeric() || matches!(c, '\'' | '’' | '-');
        if !in_word(chars[hit]) {
            return Some(hit..hit + 1);
        }
        let start = (0..hit)
            .rev()
            .take_while(|&i| in_word(chars[i]))
            .last()
            .unwrap_or(hit);
        let end = (hit..chars.len())
            .take_while(|&i| in_word(chars[i]))
            .last()
            .unwrap_or(hit)
            + 1;
        Some(start..end)
    }

    /// The characters of the line nearest (`x`, `y`), without its newline.
    pub fn line_range_at(&self, x: f32, y: f32) -> Option<Range<usize>> {
        let line = self.line_at(x, y)?;
        let start = self.chars.iter().position(|c| c.line == line)?;
        let mut end = self.chars.iter().rposition(|c| c.line == line)? + 1;
        if self.text.chars().nth(end - 1) == Some('\n') {
            end -= 1;
        }
        Some(start..end)
    }

    /// One rectangle per line `range` touches: the union of its glyphs
    /// there, as tall as the whole line.
    pub fn rects(&self, range: Range<usize>) -> Vec<Rect> {
        let lines = self.lines();
        let mut out: Vec<(usize, Rect)> = Vec::new();
        for i in range.start..range.end.min(self.chars.len()) {
            let info = self.chars[i];
            let (Some(g), Some(Some(line))) = (info.glyph, lines.get(info.line)) else {
                continue;
            };
            let b = self.bounds[g];
            let b = Rect {
                y: line.y,
                height: line.height,
                ..b
            };
            match out.last_mut() {
                Some((l, r)) if *l == info.line => *r = union(*r, b),
                _ => out.push((info.line, b)),
            }
        }
        out.into_iter().map(|(_, r)| r).collect()
    }

    /// A zero-width rectangle at `caret`, as tall as its line: at the left
    /// edge of the glyph after it, or the right edge of the glyph before
    /// it (the end of a line). `None` without glyphs.
    pub fn caret_rect(&self, caret: usize) -> Option<Rect> {
        let lines = self.lines();
        let at = |i: usize, right: bool| {
            let info = self.chars[i];
            let b = self.bounds[info.glyph?];
            let line = (*lines.get(info.line)?)?;
            Some(Rect {
                x: if right { b.x + b.width } else { b.x },
                y: line.y,
                width: 0.0,
                height: line.height,
            })
        };
        if caret < self.chars.len()
            && let Some(r) = at(caret, false)
        {
            return Some(r);
        }
        let before = (0..caret.min(self.chars.len()))
            .rev()
            .find_map(|i| at(i, true));
        before.or_else(|| (caret..self.chars.len()).find_map(|i| at(i, false)))
    }

    /// The bounds of the line `char` is on; the last line for the caret at
    /// the end.
    pub fn line_rect_of(&self, char: usize) -> Option<Rect> {
        let line = self.chars.get(char).or(self.chars.last())?.line;
        (*self.lines().get(line)?)?.into()
    }
}

pub(crate) fn union(a: Rect, b: Rect) -> Rect {
    let (left, top) = (a.x.min(b.x), a.y.min(b.y));
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

fn bounds_of(glyph: &Glyph) -> Rect {
    let (min, max) = glyph.quad.bounds();
    Rect {
        x: min.x,
        y: min.y,
        width: max.x - min.x,
        height: max.y - min.y,
    }
}

/// A glyph's baseline: where it starts, its unit direction and its length.
struct Baseline {
    origin: Point,
    dir: (f32, f32),
    advance: f32,
    size: f32,
}

impl Baseline {
    fn of(glyph: &Glyph) -> Baseline {
        let [start, end, ..] = glyph.quad.corners;
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        let advance = dx.hypot(dy);
        let dir = if advance > 0.0 {
            (dx / advance, dy / advance)
        } else {
            (1.0, 0.0)
        };
        Baseline {
            origin: glyph.origin,
            dir,
            advance,
            size: glyph.size,
        }
    }

    /// How far `p` lies along this baseline from its start, and across it.
    fn project(&self, p: Point) -> (f32, f32) {
        let (dx, dy) = (p.x - self.origin.x, p.y - self.origin.y);
        (
            dx * self.dir.0 + dy * self.dir.1,
            dy * self.dir.0 - dx * self.dir.1,
        )
    }

    /// Whether `glyph`'s centre lies over this glyph's span.
    fn covers(&self, glyph: &Glyph) -> bool {
        let c = glyph.quad.corners;
        let centre = Point {
            x: c.iter().map(|p| p.x).sum::<f32>() / 4.0,
            y: c.iter().map(|p| p.y).sum::<f32>() / 4.0,
        };
        let (along, across) = self.project(centre);
        (0.0..=self.advance).contains(&along) && across.abs() <= self.size
    }
}

/// Joins glyphs in content-stream order into clusters of text.
fn assemble(glyphs: &[Glyph]) -> Vec<Cluster> {
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut line = 0;
    // The last glyph placed as text, and its cluster.
    let mut prev: Option<(Baseline, usize)> = None;
    // A mark not over the previous glyph, waiting to see the next one: the
    // mark and its glyph's index.
    let mut pending: Option<(char, usize)> = None;
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.text.is_empty() {
            continue;
        }
        if let Some(mark) = mark_of(&glyph.text) {
            match &prev {
                Some((base, cluster)) if base.covers(glyph) => {
                    join_mark(&mut clusters[*cluster], mark);
                }
                _ => {
                    unjoined(pending.take(), glyphs, &mut clusters, line);
                    pending = Some((mark, index));
                }
            }
            continue;
        }
        let baseline = Baseline::of(glyph);
        if let Some((p, _)) = &prev {
            let em = p.size.max(glyph.size).max(1.0);
            let (along, across) = p.project(glyph.origin);
            if across.abs() > NEW_LINE * em || along < -NEW_LINE * em {
                unjoined(pending.take(), glyphs, &mut clusters, line);
                clusters.push(Cluster {
                    text: "\n".into(),
                    glyph: None,
                    line,
                });
                line += 1;
            } else if along - p.advance > SPACE * em
                && !clusters
                    .last()
                    .is_some_and(|c| c.text.ends_with(char::is_whitespace))
                && !glyph.text.starts_with(char::is_whitespace)
            {
                clusters.push(Cluster {
                    text: " ".into(),
                    glyph: None,
                    line,
                });
            }
        }
        if let Some((mark, mark_index)) = pending.take() {
            if !baseline.covers(&glyphs[mark_index]) {
                unjoined(Some((mark, mark_index)), glyphs, &mut clusters, line);
            } else {
                pending = Some((mark, mark_index));
            }
        }
        clusters.push(Cluster {
            text: glyph.text.clone(),
            glyph: Some(index),
            line,
        });
        let cluster = clusters.len() - 1;
        if let Some((mark, _)) = pending.take() {
            join_mark(&mut clusters[cluster], mark);
        }
        prev = Some((baseline, cluster));
    }
    unjoined(pending, glyphs, &mut clusters, line);
    clusters
}

/// Places a mark that joined no glyph as the character it was drawn as.
fn unjoined(
    pending: Option<(char, usize)>,
    glyphs: &[Glyph],
    clusters: &mut Vec<Cluster>,
    line: usize,
) {
    if let Some((_, index)) = pending {
        clusters.push(Cluster {
            text: glyphs[index].text.clone(),
            glyph: Some(index),
            line,
        });
    }
}

/// The combining mark `text` is, or is drawn as: spacing diacritics count.
fn mark_of(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let (c, None) = (chars.next()?, chars.next()) else {
        return None;
    };
    if is_combining_mark(c) {
        return Some(c);
    }
    let mark = match c {
        '\u{a8}' => '\u{308}', // ¨
        '\u{b4}' => '\u{301}', // ´
        '`' => '\u{300}',
        '\u{2c6}' | '^' => '\u{302}', // ˆ
        '\u{2dc}' => '\u{303}',       // ˜
        '\u{b8}' => '\u{327}',        // ¸
        '\u{2c7}' => '\u{30c}',       // ˇ
        '\u{2da}' => '\u{30a}',       // ˚
        '\u{af}' => '\u{304}',        // ¯
        '\u{2d8}' => '\u{306}',       // ˘
        '\u{2d9}' => '\u{307}',       // ˙
        '\u{2dd}' => '\u{30b}',       // ˝
        '\u{2db}' => '\u{328}',       // ˛
        _ => return None,
    };
    Some(mark)
}

/// Adds `mark` to `cluster`'s text; a dotless i or j under a mark above
/// becomes the dotted letter, which is what the mark replaces the dot of.
fn join_mark(cluster: &mut Cluster, mark: char) {
    let above = !matches!(mark, '\u{327}' | '\u{328}');
    if above {
        cluster.text = cluster.text.replace('ı', "i").replace('ȷ', "j");
    }
    cluster.text.push(mark);
}
/// The glyphs that are at least partly on a page `width` by `height`
/// points: text drawn wholly outside it, like a printer's slug above the
/// trim, is never shown.
pub fn on_page(glyphs: Vec<Glyph>, width: f32, height: f32) -> Vec<Glyph> {
    glyphs
        .into_iter()
        .filter(|g| {
            let xs = g.quad.corners.map(|p| p.x);
            let ys = g.quad.corners.map(|p| p.y);
            let (min_x, max_x) = (
                xs.into_iter().fold(f32::MAX, f32::min),
                xs.into_iter().fold(f32::MIN, f32::max),
            );
            let (min_y, max_y) = (
                ys.into_iter().fold(f32::MAX, f32::min),
                ys.into_iter().fold(f32::MIN, f32::max),
            );
            max_x >= 0.0 && min_x <= width && max_y >= 0.0 && min_y <= height
        })
        .collect()
}

/// Where a point is against its column's lines, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Beyond {
    Above,
    Within,
    Below,
}

/// The lines with glyphs, in text order, split into columns: a line that
/// doesn't overlap the one before it horizontally starts a new column.
fn columns(lines: &[Option<Rect>]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut before: Option<Rect> = None;
    for (line, rect) in lines.iter().enumerate() {
        let Some(r) = *rect else {
            continue;
        };
        let overlaps = before.is_some_and(|b| r.x <= b.x + b.width && b.x <= r.x + r.width);
        match out.last_mut() {
            Some(column) if overlaps => column.push(line),
            _ => out.push(vec![line]),
        }
        before = Some(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{glyph, glyph_along, line, page};

    fn text(glyphs: Vec<noido_engine::Glyph>) -> String {
        PageText::new(glyphs).text().to_owned()
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn line_near_takes_first_line_below_anchor() {
        // Line tops at 92, 106, 120; the anchor is 2 pt above the second.
        let page = PageText::new(page(&["one", "two", "three"]));
        assert_eq!(
            page.line_near(Some(72.0), Some(104.0)),
            Some(rect(72.0, 106.0, 15.0, 10.0))
        );
    }

    #[test]
    fn line_near_ignores_lines_left_of_x() {
        let mut glyphs = line("left", 72.0, 120.0, 10.0);
        glyphs.extend(line("right", 300.0, 130.0, 10.0));
        let page = PageText::new(glyphs);
        assert_eq!(
            page.line_near(Some(300.0), Some(110.0)),
            Some(rect(300.0, 122.0, 25.0, 10.0))
        );
    }

    #[test]
    fn line_near_without_y_is_top_line() {
        let mut glyphs = line("low", 72.0, 200.0, 10.0);
        glyphs.extend(line("high", 72.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        assert_eq!(
            page.line_near(None, None),
            Some(rect(72.0, 92.0, 20.0, 10.0))
        );
    }

    #[test]
    fn line_near_below_all_text_is_none() {
        let page = PageText::new(page(&["a"]));
        assert_eq!(page.line_near(None, Some(500.0)), None);
        assert_eq!(PageText::new(Vec::new()).line_near(None, None), None);
    }

    #[test]
    fn text_span_covers_all_lines() {
        let page = PageText::new(page(&["ab", "abcd"]));
        assert_eq!(page.text_span(), Some((72.0, 92.0)));
    }

    #[test]
    fn empty_page_has_no_span() {
        assert_eq!(PageText::new(Vec::new()).text_span(), None);
    }

    #[test]
    fn one_line_text() {
        assert_eq!(text(line("Hello", 72.0, 100.0, 10.0)), "Hello");
    }

    #[test]
    fn gap_becomes_space() {
        // "ab" ends at 82; "cd" starts 2 pt (0.2 em) later.
        let mut glyphs = line("ab", 72.0, 100.0, 10.0);
        glyphs.extend(line("cd", 84.0, 100.0, 10.0));
        assert_eq!(text(glyphs), "ab cd");
    }

    #[test]
    fn tight_gap_is_no_space() {
        let mut glyphs = line("ab", 72.0, 100.0, 10.0);
        glyphs.extend(line("cd", 82.5, 100.0, 10.0));
        assert_eq!(text(glyphs), "abcd");
    }

    #[test]
    fn space_glyph_not_doubled() {
        // A space glyph, then a gap anyway.
        let mut glyphs = line("ab ", 72.0, 100.0, 10.0);
        glyphs.extend(line("cd", 90.0, 100.0, 10.0));
        assert_eq!(text(glyphs), "ab cd");
    }

    #[test]
    fn drop_starts_new_line() {
        assert_eq!(text(page(&["ab", "cd"])), "ab\ncd");
    }

    #[test]
    fn move_left_starts_new_line() {
        // Second column's first line: up and to the right of the first
        // column's last line.
        let mut glyphs = line("ab", 72.0, 300.0, 10.0);
        glyphs.extend(line("cd", 300.0, 100.0, 10.0));
        // Back left, one line down: the column's next line.
        glyphs.extend(line("ef", 300.0, 114.0, 10.0));
        assert_eq!(text(glyphs), "ab\ncd\nef");
        // Back left on the same baseline.
        let mut glyphs = line("ab", 200.0, 100.0, 10.0);
        glyphs.extend(line("cd", 72.0, 100.0, 10.0));
        assert_eq!(text(glyphs), "ab\ncd");
    }

    #[test]
    fn vertical_line_is_one_line() {
        let glyphs = "Hello"
            .chars()
            .enumerate()
            .map(|(i, c)| {
                glyph_along(
                    &c.to_string(),
                    300.0,
                    72.0 + i as f32 * 5.0,
                    10.0,
                    0.5,
                    (0.0, 1.0),
                )
            })
            .collect();
        assert_eq!(text(glyphs), "Hello");
    }

    #[test]
    fn empty_glyph_text_is_skipped() {
        let mut glyphs = line("ab", 72.0, 100.0, 10.0);
        glyphs.insert(1, glyph("", 77.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        assert_eq!(page.text(), "ab");
        assert_eq!(page.char_count(), 2);
    }

    #[test]
    fn multi_char_glyph_maps_each_char() {
        let fi = glyph("fi", 72.0, 100.0, 10.0);
        let (min, max) = fi.quad.bounds();
        let page = PageText::new(vec![fi]);
        assert_eq!(page.text(), "fi");
        let bounds = Rect {
            x: min.x,
            y: min.y,
            width: max.x - min.x,
            height: max.y - min.y,
        };
        assert_eq!(page.glyph_bounds(0), Some(bounds));
        assert_eq!(page.glyph_bounds(1), Some(bounds));
    }

    #[test]
    fn separate_diaeresis_joins_base() {
        let mut glyphs = line("na", 72.0, 100.0, 10.0);
        glyphs.push(glyph("ı", 82.0, 100.0, 10.0));
        // The mark is narrower and centred over the ı.
        glyphs.push(glyph_along("¨", 83.0, 100.0, 10.0, 0.3, (1.0, 0.0)));
        glyphs.extend(line("ve", 87.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        assert_eq!(page.text(), "naïve");
        assert_eq!(
            page.glyph_bounds(2),
            page.glyph_bounds(1).map(|r| Rect { x: r.x + 5.0, ..r })
        );
    }

    #[test]
    fn diaeresis_before_base_joins_base() {
        let mut glyphs = line("na", 72.0, 100.0, 10.0);
        glyphs.push(glyph_along("¨", 83.0, 100.0, 10.0, 0.3, (1.0, 0.0)));
        glyphs.push(glyph("ı", 82.0, 100.0, 10.0));
        glyphs.extend(line("ve", 87.0, 100.0, 10.0));
        assert_eq!(text(glyphs), "naïve");
    }

    #[test]
    fn lone_diaeresis_stays() {
        let mut glyphs = line("a ", 72.0, 100.0, 10.0);
        glyphs.push(glyph("¨", 90.0, 100.0, 10.0));
        assert_eq!(text(glyphs), "a ¨");
    }

    #[test]
    fn inserted_space_has_no_glyph() {
        let mut glyphs = line("ab", 72.0, 100.0, 10.0);
        glyphs.extend(line("cd", 90.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        assert_eq!(page.text(), "ab cd");
        assert!(page.glyph_bounds(1).is_some());
        assert_eq!(page.glyph_bounds(2), None);
    }

    #[test]
    fn lines_are_numbered() {
        let page = PageText::new(page(&["ab", "cd"]));
        let lines: Vec<usize> = (0..page.char_count()).map(|c| page.line_of(c)).collect();
        // The newline belongs to the line it ends.
        assert_eq!(lines, [0, 0, 0, 1, 1]);
    }

    #[test]
    fn empty_page() {
        let page = PageText::new(Vec::new());
        assert_eq!(page.text(), "");
        assert_eq!(page.char_count(), 0);
    }

    /// "the cat sat" / "on the mat": glyphs 5 pt wide from x 72, lines
    /// 92..102 and 106..116 high.
    fn cat_page() -> PageText {
        PageText::new(page(&["the cat sat", "on the mat"]))
    }

    #[test]
    fn caret_at_glyph_halves() {
        let page = cat_page();
        assert_eq!(page.caret_at(73.0, 97.0), Some(0));
        assert_eq!(page.caret_at(76.0, 97.0), Some(1));
    }

    #[test]
    fn caret_at_left_and_right_of_line() {
        let page = cat_page();
        assert_eq!(page.caret_at(10.0, 97.0), Some(0));
        assert_eq!(page.caret_at(500.0, 97.0), Some(11));
    }

    #[test]
    fn caret_at_above_and_below() {
        let page = cat_page();
        assert_eq!(page.caret_at(73.0, 0.0), Some(0));
        assert_eq!(page.caret_at(500.0, 700.0), Some(page.char_count()));
    }

    #[test]
    fn on_page_drops_glyphs_wholly_outside() {
        let mut glyphs = line("in", 72.0, 100.0, 10.0);
        glyphs.extend(line("above", 72.0, -30.0, 10.0));
        glyphs.extend(line("left", -100.0, 300.0, 10.0));
        glyphs.extend(line("edge", 72.0, 4.0, 10.0));
        let kept: String = on_page(glyphs, 612.0, 792.0)
            .into_iter()
            .map(|g| g.text)
            .collect();
        assert_eq!(kept, "inedge");
    }

    /// A paragraph with a short last line.
    fn ragged_page() -> PageText {
        PageText::new(page(&["a long first line here", "short"]))
    }

    #[test]
    fn caret_below_last_line_goes_to_its_end() {
        let page = ragged_page();
        assert_eq!(page.caret_at(80.0, 125.0), Some(page.char_count()));
    }

    #[test]
    fn caret_below_short_last_line_does_not_go_up_a_line() {
        let page = ragged_page();
        assert_eq!(page.caret_at(150.0, 125.0), Some(page.char_count()));
    }

    #[test]
    fn caret_above_first_line_goes_to_its_start() {
        let page = ragged_page();
        assert_eq!(page.caret_at(150.0, 50.0), Some(0));
    }

    /// Two columns, the right one a line longer.
    fn columns_page() -> PageText {
        let mut glyphs = Vec::new();
        for (i, text) in ["left one", "left two"].iter().enumerate() {
            glyphs.extend(line(text, 72.0, 100.0 + i as f32 * 14.0, 10.0));
        }
        for (i, text) in ["right one", "right two", "right three"].iter().enumerate() {
            glyphs.extend(line(text, 300.0, 100.0 + i as f32 * 14.0, 10.0));
        }
        PageText::new(glyphs)
    }

    #[test]
    fn caret_below_a_column_goes_to_its_last_line_end() {
        let page = columns_page();
        let end = page.text().find("left two").unwrap() + "left two".len();
        assert_eq!(page.caret_at(80.0, 135.0), Some(end));
        assert_eq!(page.caret_at(310.0, 160.0), Some(page.char_count()));
    }

    #[test]
    fn caret_in_a_right_column_under_a_full_width_title_stays_there() {
        let mut glyphs = line(&"a title running right across".repeat(2), 72.0, 100.0, 10.0);
        for (i, text) in ["left one", "left two"].iter().enumerate() {
            glyphs.extend(line(text, 72.0, 114.0 + i as f32 * 14.0, 10.0));
        }
        for (i, text) in ["right one", "right two"].iter().enumerate() {
            glyphs.extend(line(text, 300.0, 114.0 + i as f32 * 14.0, 10.0));
        }
        let page = PageText::new(glyphs);
        let two = page.text().find("right two").unwrap();
        assert_eq!(page.caret_at(312.0, 125.0), Some(two + 2));
        assert_eq!(page.caret_at(312.0, 150.0), Some(page.char_count()));
    }

    #[test]
    fn caret_beside_a_column_stays_in_it() {
        let page = columns_page();
        let start = page.text().find("right two").unwrap();
        assert_eq!(page.caret_at(290.0, 111.0), Some(start));
    }

    #[test]
    fn caret_at_between_lines_takes_nearest() {
        let page = cat_page();
        assert_eq!(page.caret_at(73.0, 103.0), Some(0));
        assert_eq!(page.caret_at(73.0, 105.5), Some(12));
    }

    #[test]
    fn caret_at_two_columns() {
        let mut glyphs = line("left col", 72.0, 100.0, 10.0);
        glyphs.extend(line("right col", 300.0, 100.0, 10.0));
        let page = PageText::new(glyphs);
        let caret = page.caret_at(311.0, 97.0).unwrap();
        let right = page.text().find("right").unwrap();
        assert_eq!(caret, right + 2);
    }

    #[test]
    fn caret_at_without_text() {
        assert_eq!(PageText::new(Vec::new()).caret_at(0.0, 0.0), None);
    }

    #[test]
    fn word_and_line_at() {
        let page = cat_page();
        assert_eq!(page.word_at(93.0, 97.0), Some(4..7));
        assert_eq!(page.line_range_at(78.0, 111.0), Some(12..22));
        assert_eq!(PageText::new(Vec::new()).word_at(0.0, 0.0), None);
    }

    #[test]
    fn rects_per_line() {
        let page = cat_page();
        let rects = page.rects(4..15);
        assert_eq!(
            rects,
            vec![rect(92.0, 92.0, 35.0, 10.0), rect(72.0, 106.0, 15.0, 10.0)]
        );
    }

    #[test]
    fn caret_rect_at_ends() {
        let page = cat_page();
        assert_eq!(page.caret_rect(0), Some(rect(72.0, 92.0, 0.0, 10.0)));
        assert_eq!(page.caret_rect(11), Some(rect(127.0, 92.0, 0.0, 10.0)));
        assert_eq!(
            page.caret_rect(page.char_count()),
            Some(rect(122.0, 106.0, 0.0, 10.0))
        );
        let empty = PageText::new(Vec::new());
        assert_eq!(empty.caret_rect(0), None);
        assert!(empty.rects(0..0).is_empty());
    }

    #[test]
    fn line_rect_of_char() {
        let page = cat_page();
        assert_eq!(page.line_rect_of(13), Some(rect(72.0, 106.0, 50.0, 10.0)));
        assert_eq!(page.line_rect_of(11), Some(rect(72.0, 92.0, 55.0, 10.0)));
    }
}
