//! The pages the reader pins: their own shortlist, kept per document and
//! shown in the sidebar's Pins tab.

/// The pages the reader pins, in page order, one each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pins(Vec<usize>);

impl Pins {
    /// The pins `pages` names, in page order and without duplicates.
    pub fn from_sorted(pages: impl IntoIterator<Item = usize>) -> Self {
        let mut pins: Vec<usize> = pages.into_iter().collect();
        pins.sort_unstable();
        pins.dedup();
        Self(pins)
    }

    /// The pinned pages, in page order.
    pub fn pages(&self) -> &[usize] {
        &self.0
    }

    pub fn contains(&self, page: usize) -> bool {
        self.0.binary_search(&page).is_ok()
    }

    /// Pins `page` if it is not pinned and unpins it if it is; returns
    /// whether it is pinned now.
    pub fn toggle(&mut self, page: usize) -> bool {
        match self.0.binary_search(&page) {
            Ok(index) => {
                self.0.remove(index);
                false
            }
            Err(index) => {
                self.0.insert(index, page);
                true
            }
        }
    }

    pub fn remove(&mut self, page: usize) {
        if let Ok(index) = self.0.binary_search(&page) {
            self.0.remove(index);
        }
    }

    /// Drops pins at or past `page_count`.
    pub fn retain_below(&mut self, page_count: usize) {
        let keep = self.0.partition_point(|&page| page < page_count);
        self.0.truncate(keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_pins_in_page_order() {
        let mut pins = Pins::default();
        assert!(pins.toggle(7));
        assert!(pins.toggle(2));
        assert!(pins.toggle(5));
        assert_eq!(pins.pages(), [2, 5, 7]);
    }

    #[test]
    fn toggle_twice_unpins() {
        let mut pins = Pins::default();
        assert!(pins.toggle(3));
        assert!(!pins.toggle(3));
        assert!(!pins.contains(3));
        assert!(pins.pages().is_empty());
    }

    #[test]
    fn from_sorted_sorts_and_drops_duplicates() {
        let pins = Pins::from_sorted([9, 2, 9, 2, 5]);
        assert_eq!(pins.pages(), [2, 5, 9]);
    }

    #[test]
    fn retain_below_drops_past_the_end() {
        let mut pins = Pins::from_sorted([0, 4, 5, 9]);
        pins.retain_below(5);
        assert_eq!(pins.pages(), [0, 4]);
    }

    #[test]
    fn remove_leaves_the_rest_in_order() {
        let mut pins = Pins::from_sorted([1, 4, 6]);
        pins.remove(4);
        assert_eq!(pins.pages(), [1, 6]);
    }
}
