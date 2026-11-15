//! The pages the reader pins: their own shortlist, kept per document and
//! shown in the sidebar's Pins tab.

use noido_engine::LocationMap;

use crate::{DocPoint, Place};

/// The places the reader pins, in document order, one each. A pin on a
/// fixed page is its top left; a pin in text that reflows is the first
/// character of the page that was showing, and shows on whichever page
/// holds that character now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pins(Vec<Place>);

/// Where a pin sorts: its part and character, a page counting as its
/// start, a page before text at the same key. Only equal pins are one
/// pin: a page without text and the first character of part 0 share a
/// key and are both kept.
fn key(place: &Place) -> (usize, usize, bool) {
    match place {
        Place::Point(p) => (p.page, 0, false),
        Place::Text(at) => (at.part, at.char, true),
    }
}

fn page_top(page: usize) -> Place {
    Place::Point(DocPoint {
        page,
        x: 0.0,
        y: 0.0,
    })
}

impl Pins {
    /// Pins on the fixed pages `pages`, in page order and without
    /// duplicates.
    pub fn from_sorted(pages: impl IntoIterator<Item = usize>) -> Self {
        Self::from_places(pages.into_iter().map(page_top))
    }

    /// The pins `places` names, in document order and without duplicates.
    pub fn from_places(places: impl IntoIterator<Item = Place>) -> Self {
        let mut pins: Vec<Place> = places.into_iter().collect();
        pins.sort_by_key(key);
        pins.dedup();
        Self(pins)
    }

    /// The pins, in document order.
    pub fn places(&self) -> &[Place] {
        &self.0
    }

    /// The pinned pages as `map` lays the document out, in page order,
    /// each once.
    pub fn pages(&self, map: &LocationMap) -> Vec<usize> {
        let mut pages: Vec<usize> = self.0.iter().filter_map(|p| p.page(map)).collect();
        pages.sort_unstable();
        pages.dedup();
        pages
    }

    pub fn contains(&self, page: usize, map: &LocationMap) -> bool {
        self.0.iter().any(|p| p.page(map) == Some(page))
    }

    /// Pins `page` if it is not pinned and unpins it if it is; returns
    /// whether it is pinned now. Unpinning takes every pin on it.
    pub fn toggle(&mut self, page: usize, map: &LocationMap) -> bool {
        if self.contains(page, map) {
            self.remove(page, map);
            return false;
        }
        let place = match map.page_start(page) {
            Some(at) if map.reflows() => Place::Text(at),
            _ => page_top(page),
        };
        let index = self.0.partition_point(|p| key(p) < key(&place));
        self.0.insert(index, place);
        true
    }

    /// Unpins every pin on `page`.
    pub fn remove(&mut self, page: usize, map: &LocationMap) {
        self.0.retain(|p| p.page(map) != Some(page));
    }

    /// Drops pins at or past `page_count`, and pins in text that isn't
    /// there.
    pub fn retain_below(&mut self, page_count: usize, map: &LocationMap) {
        self.0
            .retain(|p| p.page(map).is_some_and(|page| page < page_count));
    }
}

#[cfg(test)]
mod tests {
    use noido_engine::{Location, Run};

    use super::*;

    fn fixed() -> LocationMap {
        LocationMap::identity()
    }

    fn text(char: usize) -> Place {
        Place::Text(Location { part: 0, char })
    }

    fn line(char: usize, chars: usize) -> Run {
        Run {
            start: Location { part: 0, char },
            chars,
            top: 36.0,
            bottom: 46.0,
        }
    }

    /// Part 0 laid out `per_page` characters a page, one line each, over
    /// `pages` pages.
    fn flowed(per_page: usize, pages: usize) -> LocationMap {
        LocationMap::from_lines(
            (0..pages)
                .map(|page| vec![line(page * per_page, per_page)])
                .collect(),
        )
    }

    #[test]
    fn toggle_pins_in_page_order() {
        let mut pins = Pins::default();
        assert!(pins.toggle(7, &fixed()));
        assert!(pins.toggle(2, &fixed()));
        assert!(pins.toggle(5, &fixed()));
        assert_eq!(pins.pages(&fixed()), [2, 5, 7]);
    }

    #[test]
    fn a_pin_on_a_fixed_page_is_its_top_left() {
        let mut pins = Pins::default();
        pins.toggle(4, &fixed());
        assert_eq!(
            pins.places(),
            [Place::Point(DocPoint {
                page: 4,
                x: 0.0,
                y: 0.0
            })]
        );
    }

    #[test]
    fn toggle_twice_unpins() {
        let mut pins = Pins::default();
        assert!(pins.toggle(3, &fixed()));
        assert!(!pins.toggle(3, &fixed()));
        assert!(!pins.contains(3, &fixed()));
        assert!(pins.pages(&fixed()).is_empty());
    }

    #[test]
    fn from_sorted_sorts_and_drops_duplicates() {
        let pins = Pins::from_sorted([9, 2, 9, 2, 5]);
        assert_eq!(pins.pages(&fixed()), [2, 5, 9]);
    }

    #[test]
    fn from_places_sorts_and_drops_duplicates() {
        let pins = Pins::from_places([text(300), text(100), text(300)]);
        assert_eq!(pins.places(), [text(100), text(300)]);
    }

    #[test]
    fn a_page_pin_and_a_text_pin_at_its_start_both_stay() {
        let page = Place::Point(DocPoint {
            page: 0,
            x: 0.0,
            y: 0.0,
        });
        let pins = Pins::from_places([text(0), page, text(0), page]);
        assert_eq!(pins.places().len(), 2);
    }

    #[test]
    fn a_pin_in_text_is_the_page_start() {
        let map = flowed(100, 5);
        let mut pins = Pins::default();
        assert!(pins.toggle(2, &map));
        assert_eq!(pins.places(), [text(200)]);
        assert!(pins.contains(2, &map));
    }

    #[test]
    fn a_text_pin_follows_its_character_to_another_page() {
        let mut pins = Pins::default();
        pins.toggle(2, &flowed(100, 5));
        // Smaller pages: character 200 is on page 3 now.
        assert_eq!(pins.pages(&flowed(60, 9)), [3]);
    }

    #[test]
    fn two_text_pins_on_one_page_show_once() {
        let mut pins = Pins::default();
        pins.toggle(1, &flowed(100, 5));
        pins.toggle(2, &flowed(100, 5));
        // Larger pages: characters 100 and 200 are both on page 0.
        let larger = flowed(300, 2);
        assert_eq!(pins.pages(&larger), [0]);
        assert!(!pins.toggle(0, &larger));
        assert!(pins.places().is_empty());
    }

    #[test]
    fn a_page_without_text_is_pinned_as_a_page() {
        let map = LocationMap::from_lines(vec![vec![], vec![line(0, 100)]]);
        let mut pins = Pins::default();
        assert!(pins.toggle(0, &map));
        assert_eq!(pins.pages(&map), [0]);
        assert!(pins.toggle(1, &map));
        assert_eq!(pins.pages(&map), [0, 1]);
    }

    #[test]
    fn retain_below_drops_past_the_end() {
        let mut pins = Pins::from_sorted([0, 4, 5, 9]);
        pins.retain_below(5, &fixed());
        assert_eq!(pins.pages(&fixed()), [0, 4]);
    }

    #[test]
    fn pins_past_the_end_of_the_text_are_dropped() {
        let mut pins = Pins::from_places([text(100), text(450)]);
        pins.retain_below(3, &flowed(100, 3));
        assert_eq!(pins.places(), [text(100)]);
    }

    #[test]
    fn remove_leaves_the_rest_in_order() {
        let mut pins = Pins::from_sorted([1, 4, 6]);
        pins.remove(4, &fixed());
        assert_eq!(pins.pages(&fixed()), [1, 6]);
    }
}
