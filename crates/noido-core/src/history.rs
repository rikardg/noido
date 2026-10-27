use crate::ViewState;

/// How many places Back can go to; the oldest go first.
pub const HISTORY_LIMIT: usize = 100;

/// The places the reader jumped away from, for Back and Forward. Each entry
/// is where the reader was when they left it, not where a jump landed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    /// Oldest first.
    back: Vec<ViewState>,
    /// The next place Forward goes to is last.
    forward: Vec<ViewState>,
}

impl History {
    /// Restores saved entries, keeping the newest [`HISTORY_LIMIT`] of each.
    pub fn from_parts(back: Vec<ViewState>, forward: Vec<ViewState>) -> Self {
        let newest = |mut entries: Vec<ViewState>| {
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
    pub fn leave(&mut self, here: ViewState) {
        self.forward.clear();
        if self.back.last() != Some(&here) {
            self.back.push(here);
        }
        let extra = self.back.len().saturating_sub(HISTORY_LIMIT);
        self.back.drain(..extra);
    }

    /// The place before `here`, which Forward then returns to.
    pub fn back(&mut self, here: ViewState) -> Option<ViewState> {
        let to = self.back.pop()?;
        self.forward.push(here);
        Some(to)
    }

    /// The place Back left, which Back then returns from.
    pub fn forward(&mut self, here: ViewState) -> Option<ViewState> {
        let to = self.forward.pop()?;
        self.back.push(here);
        Some(to)
    }

    pub fn back_entries(&self) -> &[ViewState] {
        &self.back
    }

    pub fn forward_entries(&self) -> &[ViewState] {
        &self.forward
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocPoint, ZoomMode};

    fn at(page: usize) -> ViewState {
        ViewState {
            zoom: ZoomMode::FitWidth,
            scale: 1.0,
            continuous: true,
            dual: false,
            odd_left: true,
            centre: DocPoint {
                page,
                x: 0.0,
                y: 0.0,
            },
        }
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
