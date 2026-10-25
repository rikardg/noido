//! Which pages get low-res thumbnails: a window of pages around the current
//! one, nearest first and biased towards the direction of travel.

use std::ops::Range;

use noido_engine::Size;

use crate::render_scale;

/// Pages around the current one that get thumbnails.
pub const THUMBNAIL_BUDGET: usize = 100;
/// Thumbnail width in pixels; the height follows the page.
pub const THUMBNAIL_WIDTH: f32 = 120.0;
/// Longest thumbnail side in pixels, for very tall, narrow pages.
pub const THUMBNAIL_MAX_SIDE: u32 = 1024;

/// The direction the reader last moved through the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Towards higher page numbers.
    Forward,
    /// Towards lower page numbers.
    Backward,
}

/// The `budget` pages nearest `focus`, in the order to render them: `focus`
/// first, then two pages ahead (in `direction`) for every page behind. At a
/// document edge the rest of the budget goes to the other side.
pub fn thumbnail_order(
    count: usize,
    focus: usize,
    direction: Direction,
    budget: usize,
) -> Vec<usize> {
    if count == 0 {
        return vec![];
    }
    let focus = focus.min(count - 1);
    let forward = direction == Direction::Forward;
    let ahead = |distance| page_at(count, focus, distance, forward);
    let behind = |distance| page_at(count, focus, distance, !forward);

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

/// The page `distance` pages from `focus`, forward or backward, if any.
fn page_at(count: usize, focus: usize, distance: usize, forward: bool) -> Option<usize> {
    if forward {
        Some(focus + distance).filter(|&page| page < count)
    } else {
        focus.checked_sub(distance)
    }
}

/// Pages whose thumbnails are kept: within `budget` pages of `focus`.
pub fn thumbnail_keep(count: usize, focus: usize, budget: usize) -> Range<usize> {
    if count == 0 {
        return 0..0;
    }
    let focus = focus.min(count - 1);
    focus.saturating_sub(budget)..(focus + budget + 1).min(count)
}

/// The render scale that makes a page [`THUMBNAIL_WIDTH`] pixels wide, or
/// narrower if its longest side would exceed [`THUMBNAIL_MAX_SIDE`].
pub fn thumbnail_scale(size: Size) -> f32 {
    let scale = if size.width.is_finite() && size.width > 0.0 {
        THUMBNAIL_WIDTH / size.width
    } else {
        1.0
    };
    render_scale(size, scale, THUMBNAIL_MAX_SIDE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_forward_bias() {
        assert_eq!(
            thumbnail_order(1000, 500, Direction::Forward, 7),
            vec![500, 501, 502, 499, 503, 504, 498]
        );
    }

    #[test]
    fn order_backward_bias() {
        assert_eq!(
            thumbnail_order(1000, 500, Direction::Backward, 7),
            vec![500, 499, 498, 501, 497, 496, 502]
        );
    }

    #[test]
    fn order_at_start_uses_one_side() {
        assert_eq!(
            thumbnail_order(1000, 0, Direction::Backward, 5),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn order_at_end_uses_one_side() {
        assert_eq!(
            thumbnail_order(1000, 999, Direction::Forward, 4),
            vec![999, 998, 997, 996]
        );
    }

    #[test]
    fn order_full_budget_split() {
        let order = thumbnail_order(1000, 500, Direction::Forward, 100);
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
        let order = thumbnail_order(3, 1, Direction::Forward, 100);
        assert_eq!(order[0], 1);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2]);
    }

    #[test]
    fn order_zero_pages() {
        assert!(thumbnail_order(0, 0, Direction::Forward, 100).is_empty());
        assert_eq!(thumbnail_order(5, 99, Direction::Forward, 2), vec![4, 3]);
    }

    #[test]
    fn keep_is_clamped() {
        assert_eq!(thumbnail_keep(1000, 50, 100), 0..151);
        assert_eq!(thumbnail_keep(1000, 500, 100), 400..601);
        assert_eq!(thumbnail_keep(0, 0, 100), 0..0);
    }

    #[test]
    fn thumbnail_scale_zero_width() {
        let size = |width| Size {
            width,
            height: 792.0,
        };
        assert_eq!(thumbnail_scale(size(0.0)), 1.0);
        assert_eq!(thumbnail_scale(size(612.0)), THUMBNAIL_WIDTH / 612.0);
    }

    #[test]
    fn thumbnail_of_tall_narrow_page_is_capped() {
        let tall = Size {
            width: 2.0,
            height: 792.0,
        };
        assert!(thumbnail_scale(tall) * 792.0 <= THUMBNAIL_MAX_SIDE as f32);
    }
}
