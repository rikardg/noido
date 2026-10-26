//! Assembling a page's glyphs into lines and words.

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
                Some(r) => {
                    let (left, top) = (r.x.min(b.x), r.y.min(b.y));
                    let right = (r.x + r.width).max(b.x + b.width);
                    let bottom = (r.y + r.height).max(b.y + b.height);
                    Rect {
                        x: left,
                        y: top,
                        width: right - left,
                        height: bottom - top,
                    }
                }
            });
        }
        lines.into_iter().flatten().collect()
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
}
