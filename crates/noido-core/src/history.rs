use crate::{DocPoint, Place};

/// How many places Back can go to; the oldest go first.
pub const HISTORY_LIMIT: usize = 100;

/// The places the reader jumped away from, for Back and Forward. Each entry
/// is where the reader was when they left it, not where a jump landed; the
/// document's zoom and arrangement are not part of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    /// Oldest first.
    back: Vec<Place>,
    /// The next place Forward goes to is last.
    forward: Vec<Place>,
}

impl History {
    /// Restores saved entries, keeping the newest [`HISTORY_LIMIT`] of each.
    pub fn from_parts(back: Vec<Place>, forward: Vec<Place>) -> Self {
        let newest = |mut entries: Vec<Place>| {
            let extra = entries.len().saturating_sub(HISTORY_LIMIT);
            entries.drain(..extra);
            entries
        };
        Self {
            back: newest(back),
            forward: newest(forward),
        }
    }

    /// The reader jumps away from `here`. Forward is forgotten.
    pub fn leave(&mut self, here: Place) {
        self.forward.clear();
        if self.back.last() != Some(&here) {
            self.back.push(here);
        }
        let extra = self.back.len().saturating_sub(HISTORY_LIMIT);
        self.back.drain(..extra);
    }

    /// The place before `here`, which Forward then returns to.
    pub fn back(&mut self, here: Place) -> Option<Place> {
        let to = self.back.pop()?;
        self.forward.push(here);
        Some(to)
    }

    /// The place Back left, which Back then returns from.
    pub fn forward(&mut self, here: Place) -> Option<Place> {
        let to = self.forward.pop()?;
        self.back.push(here);
        Some(to)
    }

    /// Applies `f` to every entry on `page`, Back's and Forward's: for a
    /// page that changed shape, so they keep pointing at the same spot.
    /// A character in text is where it was on any shape.
    pub fn map_points(&mut self, page: usize, f: impl Fn(DocPoint) -> DocPoint) {
        for entry in self.back.iter_mut().chain(self.forward.iter_mut()) {
            if let Place::Point(p) = entry
                && p.page == page
            {
                *p = f(*p);
            }
        }
    }

    pub fn back_entries(&self) -> &[Place] {
        &self.back
    }

    pub fn forward_entries(&self) -> &[Place] {
        &self.forward
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_points_touches_only_that_page() {
        let at = |page, x, y| Place::Point(DocPoint { page, x, y });
        let mut history = History::from_parts(
            vec![at(1, 1.0, 2.0), at(2, 3.0, 4.0)],
            vec![at(1, 5.0, 6.0)],
        );
        history.map_points(1, |p| DocPoint {
            x: p.y,
            y: p.x,
            ..p
        });
        assert_eq!(history.back_entries(), &[at(1, 2.0, 1.0), at(2, 3.0, 4.0)]);
        assert_eq!(history.forward_entries(), &[at(1, 6.0, 5.0)]);
    }

    use super::*;
    use crate::DocPoint;

    fn at(page: usize) -> Place {
        Place::Point(DocPoint {
            page,
            x: 0.0,
            y: 0.0,
        })
    }

    fn text(char: usize) -> Place {
        Place::Text(noido_engine::Location { part: 0, char })
    }

    #[test]
    fn map_points_leaves_text_alone() {
        let mut h = History::from_parts(vec![text(1), at(0)], vec![]);
        h.map_points(0, |p| DocPoint { y: 9.0, ..p });
        assert_eq!(h.back_entries()[0], text(1));
        assert_ne!(h.back_entries()[1], at(0));
    }

    #[test]
    fn leave_twice_at_the_same_text_keeps_one() {
        let mut h = History::default();
        h.leave(text(40));
        h.leave(text(40));
        assert_eq!(h.back_entries(), &[text(40)]);
        assert_eq!(h.back(text(90)), Some(text(40)));
    }

    #[test]
    fn back_returns_where_you_left() {
        let mut h = History::default();
        h.leave(at(3));
        assert_eq!(h.back(at(40)), Some(at(3)));
    }

    #[test]
    fn forward_returns_where_you_read_on_to() {
        let mut h = History::default();
        h.leave(at(3));
        h.back(at(45));
        assert_eq!(h.forward(at(3)), Some(at(45)));
    }

    #[test]
    fn empty_back_and_forward_do_nothing() {
        let mut h = History::default();
        assert_eq!(h.back(at(1)), None);
        assert_eq!(h.forward(at(1)), None);
        assert_eq!(h, History::default());
    }

    #[test]
    fn a_jump_after_back_drops_forward() {
        let mut h = History::default();
        h.leave(at(3));
        h.back(at(40));
        h.leave(at(3));
        assert_eq!(h.forward(at(8)), None);
    }

    #[test]
    fn the_same_place_left_twice_is_one_entry() {
        let mut h = History::default();
        h.leave(at(3));
        h.leave(at(3));
        assert_eq!(h.back_entries(), &[at(3)]);
    }

    #[test]
    fn at_most_the_limit_going_back() {
        let mut h = History::default();
        for page in 0..HISTORY_LIMIT + 5 {
            h.leave(at(page));
        }
        assert_eq!(h.back_entries().len(), HISTORY_LIMIT);
        assert_eq!(h.back_entries()[0], at(5));
    }

    #[test]
    fn from_parts_keeps_the_newest() {
        let back = (0..HISTORY_LIMIT + 2).map(at).collect();
        let h = History::from_parts(back, vec![at(1)]);
        assert_eq!(h.back_entries()[0], at(2));
        assert_eq!(h.forward_entries(), &[at(1)]);
    }
}
