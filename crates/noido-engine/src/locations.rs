//! Where each page's characters sit in the document's parts.

use std::ops::Range;

use crate::Location;

/// One line of a laid-out page: `chars` characters from `start`, between
/// `top` and `bottom` in page space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub start: Location,
    pub chars: usize,
    pub top: f32,
    pub bottom: f32,
}

impl Run {
    fn end(&self) -> Location {
        Location {
            part: self.start.part,
            char: self.start.char + self.chars,
        }
    }
}

/// Where each page's characters sit in the parts. A fixed-page document
/// has the identity: each page is its own part. Text laid out on pages
/// (EPUB, markdown) has its lines, which move on every reflow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocationMap {
    /// Every line in reading order, with its page and how many of the
    /// page's characters come before it. Empty for the identity.
    lines: Vec<Line>,
    /// Each page's lines in `lines`. Empty for the identity.
    pages: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
struct Line {
    page: usize,
    offset: usize,
    run: Run,
}

impl LocationMap {
    /// Each page its own part, a page's characters its part's.
    pub fn identity() -> Self {
        Self::default()
    }

    /// Text laid out on pages: each page's lines in reading order, every
    /// line after the one before it in the text.
    pub fn from_lines(pages: Vec<Vec<Run>>) -> Self {
        let mut lines = Vec::new();
        let mut ranges = Vec::with_capacity(pages.len());
        for (page, runs) in pages.into_iter().enumerate() {
            let first = lines.len();
            let mut offset = 0;
            for run in runs {
                debug_assert!(
                    lines.last().is_none_or(|l: &Line| l.run.end() <= run.start),
                    "lines must come in reading order, without overlap: {run:?}"
                );
                lines.push(Line { page, offset, run });
                offset += run.chars;
            }
            ranges.push(first..lines.len());
        }
        Self {
            lines,
            pages: ranges,
        }
    }

    /// Whether pages come from layout rather than the file.
    pub fn reflows(&self) -> bool {
        !self.pages.is_empty()
    }

    /// Where character `char` of `page` is; `char` may be the page's
    /// character count, for the end of its last line.
    pub fn location(&self, page: usize, char: usize) -> Option<Location> {
        if !self.reflows() {
            return Some(Location { part: page, char });
        }
        let lines = &self.lines[self.pages.get(page)?.clone()];
        let line = lines
            .iter()
            .find(|l| char < l.offset + l.run.chars)
            .or_else(|| lines.last().filter(|l| char == l.offset + l.run.chars))?;
        Some(Location {
            part: line.run.start.part,
            char: line.run.start.char + (char - line.offset),
        })
    }

    /// The page and character there that `at` is on: a location where
    /// one line ends and the next starts is on the later one.
    pub fn page_char(&self, at: Location) -> Option<(usize, usize)> {
        if !self.reflows() {
            return Some((at.part, at.char));
        }
        let (line, char) = self.find(at)?;
        Some((line.page, line.offset + char))
    }

    /// The page `at` is on and the line holding it, as for
    /// [`Self::page_char`]. None for the identity, which has no lines.
    pub fn line(&self, at: Location) -> Option<(usize, Run)> {
        self.find(at).map(|(line, _)| (line.page, line.run))
    }

    /// The first line on `page` whose bottom is below `y`: its start and
    /// its top. None for the identity.
    pub fn first_from(&self, page: usize, y: f32) -> Option<(Location, f32)> {
        self.lines[self.pages.get(page)?.clone()]
            .iter()
            .find(|l| l.run.bottom > y)
            .map(|l| (l.run.start, l.run.top))
    }

    /// The first character on `page`; None for a page without text.
    pub fn page_start(&self, page: usize) -> Option<Location> {
        if !self.reflows() {
            return Some(Location {
                part: page,
                char: 0,
            });
        }
        let lines = self.pages.get(page)?;
        self.lines[lines.clone()].first().map(|l| l.run.start)
    }

    /// The line holding `at`, and how far into it `at` is. A location in
    /// a gap between lines (characters that aren't drawn) is the next
    /// line's start; the end of a part's last line is that line's end.
    fn find(&self, at: Location) -> Option<(&Line, usize)> {
        let after = self.lines.partition_point(|l| l.run.start <= at);
        if let Some(line) = after.checked_sub(1).map(|i| &self.lines[i])
            && line.run.start.part == at.part
            && at < line.run.end()
        {
            return Some((line, at.char - line.run.start.char));
        }
        if let Some(next) = self.lines.get(after)
            && next.run.start.part == at.part
        {
            return Some((next, 0));
        }
        let last = &self.lines[after.checked_sub(1)?];
        (last.run.end() == at).then_some((last, last.run.chars))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(part: usize, char: usize) -> Location {
        Location { part, char }
    }

    fn run(part: usize, char: usize, chars: usize, top: f32) -> Run {
        Run {
            start: at(part, char),
            chars,
            top,
            bottom: top + 10.0,
        }
    }

    /// Part 0 over two pages: page 0 has lines of 40 and 80, page 1 one of
    /// 30 starting at char 120.
    fn two_pages() -> LocationMap {
        LocationMap::from_lines(vec![
            vec![run(0, 0, 40, 36.0), run(0, 40, 80, 46.0)],
            vec![run(0, 120, 30, 36.0)],
        ])
    }

    #[test]
    fn identity_is_page_and_char() {
        let map = LocationMap::identity();
        assert!(!map.reflows());
        assert_eq!(map.location(3, 17), Some(at(3, 17)));
        assert_eq!(map.page_char(at(3, 17)), Some((3, 17)));
        assert_eq!(map.page_start(3), Some(at(3, 0)));
        assert_eq!(map.line(at(3, 17)), None);
        assert_eq!(map.first_from(3, 0.0), None);
    }

    #[test]
    fn runs_map_both_ways() {
        let map = two_pages();
        assert!(map.reflows());
        assert_eq!(map.location(1, 5), Some(at(0, 125)));
        assert_eq!(map.page_char(at(0, 125)), Some((1, 5)));
        assert_eq!(map.location(0, 45), Some(at(0, 45)));
        assert_eq!(map.page_char(at(0, 45)), Some((0, 45)));
        assert_eq!(map.page_start(1), Some(at(0, 120)));
    }

    #[test]
    fn a_location_between_runs_is_on_the_later_one() {
        let map = two_pages();
        assert_eq!(map.line(at(0, 40)), Some((0, run(0, 40, 80, 46.0))));
        assert_eq!(map.page_char(at(0, 120)), Some((1, 0)));
        assert_eq!(map.line(at(0, 120)), Some((1, run(0, 120, 30, 36.0))));
    }

    #[test]
    fn the_end_of_the_text_is_on_the_last_page() {
        let map = two_pages();
        assert_eq!(map.page_char(at(0, 150)), Some((1, 30)));
        assert_eq!(map.location(1, 30), Some(at(0, 150)));
        assert_eq!(map.line(at(0, 150)), Some((1, run(0, 120, 30, 36.0))));
    }

    #[test]
    fn past_the_end_is_none() {
        let map = two_pages();
        assert_eq!(map.page_char(at(0, 151)), None);
        assert_eq!(map.page_char(at(1, 0)), None);
        assert_eq!(map.location(1, 31), None);
        assert_eq!(map.location(2, 0), None);
        assert_eq!(map.page_start(2), None);
    }

    #[test]
    fn first_from_skips_lines_above() {
        let map = two_pages();
        assert_eq!(map.first_from(0, 0.0), Some((at(0, 0), 36.0)));
        // Inside line 1 (36..46): still line 1.
        assert_eq!(map.first_from(0, 40.0), Some((at(0, 0), 36.0)));
        // At line 1's bottom: line 2.
        assert_eq!(map.first_from(0, 46.0), Some((at(0, 40), 46.0)));
        assert_eq!(map.first_from(0, 56.0), None);
    }

    #[test]
    fn a_location_in_a_gap_is_the_next_line_start() {
        // Characters 10..12 aren't drawn (markup, say).
        let map = LocationMap::from_lines(vec![vec![run(0, 0, 10, 36.0), run(0, 12, 5, 46.0)]]);
        assert_eq!(map.page_char(at(0, 11)), Some((0, 10)));
        assert_eq!(map.location(0, 10), Some(at(0, 12)));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "in reading order")]
    fn lines_out_of_order_are_refused() {
        LocationMap::from_lines(vec![vec![run(0, 40, 10, 36.0), run(0, 0, 10, 46.0)]]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "in reading order")]
    fn overlapping_lines_are_refused() {
        LocationMap::from_lines(vec![vec![run(0, 0, 10, 36.0)], vec![run(0, 5, 10, 36.0)]]);
    }

    #[test]
    fn a_page_without_text_has_no_start() {
        let map = LocationMap::from_lines(vec![vec![], vec![run(0, 0, 10, 36.0)]]);
        assert_eq!(map.page_start(0), None);
        assert_eq!(map.page_char(at(0, 0)), Some((1, 0)));
        assert_eq!(map.location(0, 0), None);
    }
}
