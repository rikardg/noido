use std::ops::Range;

/// Relative difference under which two render scales count as the same.
const SCALE_TOLERANCE: f32 = 0.02;

fn same_scale(a: f32, b: f32) -> bool {
    (a - b).abs() <= SCALE_TOLERANCE * b.abs()
}

#[derive(Clone, Copy, Default)]
struct PageState {
    ready: Option<f32>,
    pending: Option<f32>,
    failed: Option<f32>,
}

/// Remembers which pages have been requested, rendered or failed at which
/// scale, so each page is requested only when its wanted scale changes.
pub struct RenderTracker {
    pages: Vec<PageState>,
}

impl RenderTracker {
    pub fn new(page_count: usize) -> Self {
        Self {
            pages: vec![PageState::default(); page_count],
        }
    }

    /// Pages in `pages` that need a render at `scale_for(page)`; they are marked requested.
    pub fn wanted(
        &mut self,
        pages: Range<usize>,
        scale_for: impl Fn(usize) -> f32,
    ) -> Vec<(usize, f32)> {
        let end = pages.end.min(self.pages.len());
        let mut wanted = Vec::new();
        for page in pages.start.min(end)..end {
            let scale = scale_for(page);
            let state = &mut self.pages[page];
            let covered = match state.pending {
                Some(pending) => same_scale(pending, scale),
                None => [state.ready, state.failed]
                    .into_iter()
                    .flatten()
                    .any(|s| same_scale(s, scale)),
            };
            if !covered {
                state.pending = Some(scale);
                wanted.push((page, scale));
            }
        }
        wanted
    }

    pub fn mark_ready(&mut self, page: usize, scale: f32) {
        if let Some(state) = self.pages.get_mut(page) {
            state.ready = Some(scale);
            state.failed = None;
            if state.pending.is_some_and(|p| same_scale(p, scale)) {
                state.pending = None;
            }
        }
    }

    pub fn mark_failed(&mut self, page: usize, scale: f32) {
        if let Some(state) = self.pages.get_mut(page) {
            state.failed = Some(scale);
            if state.pending.is_some_and(|p| same_scale(p, scale)) {
                state.pending = None;
            }
        }
    }

    /// Forgets every page outside `keep` and returns those that had an image,
    /// so the caller can drop it.
    pub fn forget_outside(&mut self, keep: Range<usize>) -> Vec<usize> {
        let mut dropped = Vec::new();
        for (page, state) in self.pages.iter_mut().enumerate() {
            if !keep.contains(&page) {
                if state.ready.is_some() {
                    dropped.push(page);
                }
                *state = PageState::default();
            }
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_once() {
        let mut tracker = RenderTracker::new(2);
        assert_eq!(tracker.wanted(0..2, |_| 1.0), vec![(0, 1.0), (1, 1.0)]);
        assert!(tracker.wanted(0..2, |_| 1.0).is_empty());
    }

    #[test]
    fn small_scale_change_ignored_large_rerequested() {
        let mut tracker = RenderTracker::new(1);
        tracker.wanted(0..1, |_| 1.0);
        tracker.mark_ready(0, 1.0);
        assert!(tracker.wanted(0..1, |_| 1.01).is_empty());
        assert_eq!(tracker.wanted(0..1, |_| 1.5), vec![(0, 1.5)]);
    }

    #[test]
    fn failed_page_not_rerequested() {
        let mut tracker = RenderTracker::new(1);
        tracker.wanted(0..1, |_| 1.0);
        tracker.mark_failed(0, 1.0);
        assert!(tracker.wanted(0..1, |_| 1.0).is_empty());
        assert_eq!(tracker.wanted(0..1, |_| 2.0), vec![(0, 2.0)]);
    }

    #[test]
    fn keeps_old_image_while_rerendering() {
        let mut tracker = RenderTracker::new(1);
        tracker.wanted(0..1, |_| 1.0);
        tracker.mark_ready(0, 1.0);
        tracker.wanted(0..1, |_| 1.5);
        assert!(tracker.forget_outside(0..1).is_empty());
    }

    #[test]
    fn forget_outside_returns_ready_pages() {
        let mut tracker = RenderTracker::new(4);
        tracker.wanted(0..4, |_| 1.0);
        for page in 0..4 {
            tracker.mark_ready(page, 1.0);
        }
        assert_eq!(tracker.forget_outside(1..2), vec![0, 2, 3]);
        assert_eq!(
            tracker.wanted(0..4, |_| 1.0),
            vec![(0, 1.0), (2, 1.0), (3, 1.0)]
        );
    }
}
