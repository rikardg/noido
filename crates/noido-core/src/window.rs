//! Windows of pages (or spreads) around the current one, rendered nearest
//! first and biased towards the direction of travel. Used for full-resolution
//! renders and, with a larger budget, for thumbnails.

use std::ops::Range;

/// The direction the reader last moved through the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Towards higher page numbers.
    Forward,
    /// Towards lower page numbers.
    Backward,
}

/// The `budget` items (pages or spreads) nearest `focus`, in the order to
/// render them: `focus` first, then two ahead (in `direction`) for every one
/// behind. At a document edge the rest of the budget goes to the other side.
pub fn window_order(count: usize, focus: usize, direction: Direction, budget: usize) -> Vec<usize> {
    if count == 0 {
        return vec![];
    }
    let focus = focus.min(count - 1);
    let forward = direction == Direction::Forward;
    let ahead = |distance| item_at(count, focus, distance, forward);
    let behind = |distance| item_at(count, focus, distance, !forward);

    let mut order = vec![focus];
    let (mut ahead_distance, mut behind_distance) = (1, 1);
    let mut step = 0;
    while order.len() < budget.min(count) {
        let use_ahead = match (ahead(ahead_distance), behind(behind_distance)) {
            (Some(_), Some(_)) => step % 3 != 2,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if use_ahead {
            order.extend(ahead(ahead_distance));
            ahead_distance += 1;
        } else {
            order.extend(behind(behind_distance));
            behind_distance += 1;
        }
        step += 1;
    }
    order
}

/// The item `distance` items from `focus`, forward or backward, if any.
fn item_at(count: usize, focus: usize, distance: usize, forward: bool) -> Option<usize> {
    if forward {
        Some(focus + distance).filter(|&page| page < count)
    } else {
        focus.checked_sub(distance)
    }
}

/// The items within `budget` of `focus`, the range whose renders are kept.
pub fn window_keep(count: usize, focus: usize, budget: usize) -> Range<usize> {
    if count == 0 {
        return 0..0;
    }
    let focus = focus.min(count - 1);
    focus.saturating_sub(budget)..(focus + budget + 1).min(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_forward_bias() {
        assert_eq!(
            window_order(1000, 500, Direction::Forward, 7),
            vec![500, 501, 502, 499, 503, 504, 498]
        );
    }

    #[test]
    fn order_backward_bias() {
        assert_eq!(
            window_order(1000, 500, Direction::Backward, 7),
            vec![500, 499, 498, 501, 497, 496, 502]
        );
    }

    #[test]
    fn order_at_start_uses_one_side() {
        assert_eq!(
            window_order(1000, 0, Direction::Backward, 5),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn order_at_end_uses_one_side() {
        assert_eq!(
            window_order(1000, 999, Direction::Forward, 4),
            vec![999, 998, 997, 996]
        );
    }

    #[test]
    fn order_full_budget_split() {
        let order = window_order(1000, 500, Direction::Forward, 100);
        assert_eq!(order.len(), 100);
        assert_eq!(order.iter().filter(|&&p| p > 500).count(), 66);
        assert_eq!(order.iter().filter(|&&p| p < 500).count(), 33);
        let mut unique = order.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 100);
    }

    #[test]
    fn order_small_document() {
        let order = window_order(3, 1, Direction::Forward, 100);
        assert_eq!(order[0], 1);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2]);
    }

    #[test]
    fn order_zero_pages() {
        assert!(window_order(0, 0, Direction::Forward, 100).is_empty());
        assert_eq!(window_order(5, 99, Direction::Forward, 2), vec![4, 3]);
    }

    #[test]
    fn keep_is_clamped() {
        assert_eq!(window_keep(1000, 50, 100), 0..151);
        assert_eq!(window_keep(1000, 500, 100), 400..601);
        assert_eq!(window_keep(0, 0, 100), 0..0);
    }
}
