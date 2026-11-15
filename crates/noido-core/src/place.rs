//! Where the reader is, or where a jump goes: a point on a fixed page, or
//! a character in text that reflows.

use noido_engine::{Location, LocationMap};

use crate::DocPoint;

/// Where the reader is, or where a jump goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// On a fixed page: a point in its page space (PDF, CBZ).
    Point(DocPoint),
    /// In text that reflows: a character, the first in view when it was
    /// taken.
    Text(Location),
}

impl Place {
    /// `page` as a place to keep: its top left on a fixed page, its first
    /// character in text that reflows (its top left if it has no text).
    pub fn of_page(page: usize, map: &LocationMap) -> Self {
        match map.page_start(page) {
            Some(at) if map.reflows() => Self::Text(at),
            _ => Self::Point(DocPoint {
                page,
                x: 0.0,
                y: 0.0,
            }),
        }
    }

    /// The page it is on now.
    pub fn page(&self, map: &LocationMap) -> Option<usize> {
        match self {
            Self::Point(p) => Some(p.page),
            Self::Text(at) => map.page_char(*at).map(|(page, _)| page),
        }
    }

    /// Where the view goes for it: the point itself, or the left edge of
    /// its line's top (its page's top on fixed pages).
    pub fn point(&self, map: &LocationMap) -> Option<DocPoint> {
        match self {
            Self::Point(p) => Some(*p),
            Self::Text(at) => {
                let (page, y) = match map.line(*at) {
                    Some((page, run)) => (page, run.top),
                    None => (map.page_char(*at)?.0, 0.0),
                };
                Some(DocPoint { page, x: 0.0, y })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use noido_engine::Run;

    use super::*;

    fn at(part: usize, char: usize) -> Location {
        Location { part, char }
    }

    /// Part 0 over two pages, page 1's first line starting at char 120,
    /// 50 points down.
    fn two_pages() -> LocationMap {
        let run = |char, chars, top: f32| Run {
            start: at(0, char),
            chars,
            top,
            bottom: top + 10.0,
        };
        LocationMap::from_lines(vec![vec![run(0, 120, 36.0)], vec![run(120, 30, 50.0)]])
    }

    #[test]
    fn a_point_is_its_own_page() {
        let p = DocPoint {
            page: 4,
            x: 1.0,
            y: 2.0,
        };
        let place = Place::Point(p);
        assert_eq!(place.page(&LocationMap::identity()), Some(4));
        assert_eq!(place.point(&LocationMap::identity()), Some(p));
    }

    #[test]
    fn text_goes_to_its_line_top() {
        let place = Place::Text(at(0, 125));
        assert_eq!(place.page(&two_pages()), Some(1));
        assert_eq!(
            place.point(&two_pages()),
            Some(DocPoint {
                page: 1,
                x: 0.0,
                y: 50.0
            })
        );
    }

    #[test]
    fn text_on_fixed_pages_goes_to_its_page_top() {
        let place = Place::Text(at(3, 125));
        assert_eq!(
            place.point(&LocationMap::identity()),
            Some(DocPoint {
                page: 3,
                x: 0.0,
                y: 0.0
            })
        );
    }

    #[test]
    fn a_page_is_its_top_left_or_its_first_character() {
        assert_eq!(
            Place::of_page(3, &LocationMap::identity()),
            Place::Point(DocPoint {
                page: 3,
                x: 0.0,
                y: 0.0
            })
        );
        assert_eq!(Place::of_page(1, &two_pages()), Place::Text(at(0, 120)));
    }

    #[test]
    fn text_past_the_end_has_no_page() {
        let place = Place::Text(at(0, 151));
        assert_eq!(place.page(&two_pages()), None);
        assert_eq!(place.point(&two_pages()), None);
    }
}
