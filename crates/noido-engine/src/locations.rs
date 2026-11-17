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
    /// line after the one before it in the text. The lines fill the
    /// page's characters one after the other.
    pub fn from_lines(pages: Vec<Vec<Run>>) -> Self {
        Self::from_placed(
            pages
                .into_iter()
                .map(|runs| {
                    let mut offset = 0;
                    runs.into_iter()
                        .map(|run| {
                            let placed = (offset, run);
                            offset += run.chars;
                            placed
                        })
                        .collect()
                })
                .collect(),
        )
    }

    /// Each page's runs with their offsets among the page's characters,
    /// for pages with characters that have no source position (a
    /// newline between lines, an added hyphen). Offsets don't decrease
    /// and runs don't overlap; runs are in reading order.
    pub fn from_placed(pages: Vec<Vec<(usize, Run)>>) -> Self {
        let mut lines = Vec::new();
        let mut ranges = Vec::with_capacity(pages.len());
        for (page, runs) in pages.into_iter().enumerate() {
            let first = lines.len();
            for (offset, run) in runs {
                debug_assert!(
                    lines.last().is_none_or(|l: &Line| l.run.end() <= run.start),
                    "lines must come in reading order, without overlap: {run:?}"
                );
                debug_assert!(
                    lines[first..]
                        .last()
                        .is_none_or(|l: &Line| l.offset + l.run.chars <= offset),
                    "offsets must not decrease or overlap on a page: {offset}"
                );
                lines.push(Line { page, offset, run });
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
    /// character count, for the end of its last line. A character in a
    /// gap (no source) is the next run's start; one after the last run
    /// is that run's end. So this is not the inverse of
    /// [`Self::page_char`] there: a gap character's location maps back
    /// to the next run's offset. None for a page without runs.
    pub fn location(&self, page: usize, char: usize) -> Option<Location> {
        if !self.reflows() {
            return Some(Location { part: page, char });
        }
        let lines = &self.lines[self.pages.get(page)?.clone()];
        let Some(line) = lines.iter().find(|l| char < l.offset + l.run.chars) else {
            // At or after the last run's end: unsourced characters
            // trailing it are its end.
            return lines.last().map(|l| l.run.end());
        };
        Some(Location {
            part: line.run.start.part,
            // A character before the run (in a gap) is the run's start.
            char: line.run.start.char + char.saturating_sub(line.offset),
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

    /// The last line on `page` whose top is above `y`: its start and its
    /// top. None for the identity.
    pub fn last_before(&self, page: usize, y: f32) -> Option<(Location, f32)> {
        self.lines[self.pages.get(page)?.clone()]
            .iter()
            .rev()
            .find(|l| l.run.top < y)
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
        assert_eq!(map.location(1, 31), Some(at(0, 150)));
        assert_eq!(map.location(2, 0), None);
        assert_eq!(map.page_start(2), None);
    }

    #[test]
    fn last_before_is_the_last_line_starting_above() {
        let map = two_pages();
        assert_eq!(map.last_before(0, 500.0), Some((at(0, 40), 46.0)));
        assert_eq!(map.last_before(0, 46.0), Some((at(0, 0), 36.0)));
        assert_eq!(map.last_before(0, 36.0), None);
        assert_eq!(LocationMap::identity().last_before(0, 500.0), None);
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

    /// Page 0: "ab\ncd": a run of 2 at offset 0 (chars 10..12), a run of 2
    /// at offset 3 (chars 12..14); the newline at 2 has no source.
    fn with_gap() -> LocationMap {
        LocationMap::from_placed(vec![vec![
            (0, run(0, 10, 2, 36.0)),
            (3, run(0, 12, 2, 50.0)),
        ]])
    }

    #[test]
    fn a_placed_run_keeps_its_offset() {
        assert_eq!(with_gap().location(0, 4), Some(at(0, 13)));
        assert_eq!(with_gap().page_char(at(0, 13)), Some((0, 4)));
    }

    #[test]
    fn a_character_in_a_gap_is_the_next_runs_start() {
        assert_eq!(with_gap().location(0, 2), Some(at(0, 12)));
    }

    #[test]
    fn from_lines_still_sums() {
        let placed = LocationMap::from_placed(vec![
            vec![(0, run(0, 0, 40, 36.0)), (40, run(0, 40, 80, 46.0))],
            vec![(0, run(0, 120, 30, 36.0))],
        ]);
        assert_eq!(two_pages(), placed);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "offsets")]
    fn overlapping_offsets_are_refused() {
        LocationMap::from_placed(vec![vec![
            (0, run(0, 0, 10, 36.0)),
            (5, run(0, 10, 5, 46.0)),
        ]]);
    }

    #[test]
    fn a_character_after_the_last_run_is_its_end() {
        assert_eq!(with_gap().location(0, 5), Some(at(0, 14)));
        assert_eq!(with_gap().location(0, 6), Some(at(0, 14)));
    }

    #[test]
    fn a_gap_before_the_first_run_is_its_start() {
        let map = LocationMap::from_placed(vec![vec![(2, run(0, 10, 2, 36.0))]]);
        assert_eq!(map.location(0, 0), Some(at(0, 10)));
    }
}
