use noido_engine::Size;

use crate::{Action, Layout, LayoutMode, MARGIN, scroll_target};

/// Smallest and largest zoom, in UI points per PDF point.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 4.0;
/// Factor per keyboard zoom step.
pub const ZOOM_STEP: f32 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomMode {
    /// Pages fill the viewport width.
    FitWidth,
    /// The page at the viewport centre fits the viewport.
    FitPage,
    /// A fixed scale set by zooming.
    Custom,
}

/// Offset distance, in UI points, within which the view counts as being at
/// the top or bottom edge.
const EDGE_EPSILON: f32 = 0.5;

/// What the reader sees: page arrangement, zoom, viewport size and scroll
/// position. Scale changes keep an anchor point of the document still on screen.
pub struct View {
    layout: Layout,
    mode: ZoomMode,
    viewport: (f32, f32),
    offset: (f32, f32),
}

impl View {
    pub fn new(page_sizes: Vec<Size>) -> Self {
        let mut view = Self {
            layout: Layout::new(page_sizes),
            mode: ZoomMode::FitWidth,
            viewport: (0.0, 0.0),
            offset: (0.0, 0.0),
        };
        let scale = view.fit_scale(ZoomMode::FitWidth);
        view.layout.arrange(scale, 0.0);
        view
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn mode(&self) -> ZoomMode {
        self.mode
    }

    /// All pages in one scrolling strip, as opposed to one spread at a time.
    pub fn continuous(&self) -> bool {
        self.layout.mode().paged.is_none()
    }

    /// Two-page spreads.
    pub fn dual(&self) -> bool {
        self.layout.mode().dual
    }

    /// The spread shown in paged mode.
    pub fn current_spread(&self) -> Option<usize> {
        self.layout.mode().paged
    }

    pub fn scale(&self) -> f32 {
        self.layout.scale()
    }

    /// The content point at the viewport's top-left.
    pub fn offset(&self) -> (f32, f32) {
        self.offset
    }

    pub fn viewport_size(&self) -> (f32, f32) {
        self.viewport
    }

    /// Resizes the viewport, refitting in fit modes and keeping the document
    /// point at the centre in place.
    pub fn set_viewport_size(&mut self, width: f32, height: f32) {
        if (width, height) == self.viewport {
            return;
        }
        let old_centre = self.centre();
        let was_empty = self.viewport.0 <= 0.0 || self.viewport.1 <= 0.0;
        self.viewport = (width, height);
        let scale = self.fit_scale(self.mode);
        if was_empty {
            // Nothing was on screen yet: start at the top.
            self.layout.arrange(scale, width);
            self.clamp_offset();
        } else {
            self.rescale(scale, old_centre, self.centre());
        }
    }

    /// Records a scroll the frontend performed itself.
    pub fn scrolled_to(&mut self, x: f32, y: f32) {
        self.offset = (x, y);
        self.clamp_offset();
    }

    pub fn apply(&mut self, action: Action) {
        let centre = self.centre();
        match action {
            Action::ZoomIn => self.zoom_to(self.scale() * ZOOM_STEP, centre),
            Action::ZoomOut => self.zoom_to(self.scale() / ZOOM_STEP, centre),
            Action::FitWidth | Action::FitPage => {
                self.mode = if action == Action::FitWidth {
                    ZoomMode::FitWidth
                } else {
                    ZoomMode::FitPage
                };
                let scale = self.fit_scale(self.mode);
                self.rescale(scale, centre, centre);
            }
            Action::ToggleContinuous => self.switch_mode(self.dual(), !self.continuous()),
            Action::ToggleDual => self.switch_mode(!self.dual(), self.continuous()),
            Action::ScrollScreenUp | Action::ScrollScreenDown if !self.continuous() => {
                self.page_in_paged_mode(action);
            }
            Action::ScrollScreenUp | Action::ScrollScreenDown => {
                let y = scroll_target(
                    action,
                    self.offset.1,
                    self.viewport.1,
                    self.layout.content_height(),
                );
                self.scrolled_to(self.offset.0, y);
            }
        }
    }

    /// Zooms by `factor`, keeping the document point under `anchor` (viewport
    /// coordinates; the centre when `None`) in place.
    pub fn zoom_by(&mut self, factor: f32, anchor: Option<(f32, f32)>) {
        if factor.is_finite() && factor > 0.0 {
            let anchor = anchor.unwrap_or_else(|| self.centre());
            self.zoom_to(self.scale() * factor, anchor);
        }
    }

    /// Changes the arrangement, keeping the page at the centre: in paged mode
    /// its spread is shown from the top; in continuous mode the centre point
    /// stays in place.
    fn switch_mode(&mut self, dual: bool, continuous: bool) {
        let (cx, cy) = self.centre();
        let anchor = self
            .layout
            .doc_point(self.offset.0 + cx, self.offset.1 + cy);
        let page = anchor.map_or(0, |p| p.page);

        self.layout.set_mode(LayoutMode { dual, paged: None });
        let paged = (!continuous).then(|| self.layout.spread_of(page));
        self.layout.set_mode(LayoutMode { dual, paged });
        let spread = paged.unwrap_or_else(|| self.layout.spread_of(page));
        let scale = self.fit_scale_for(self.mode, spread);
        self.layout.arrange(scale, self.viewport.0);

        if continuous {
            if let Some((x, y)) = anchor.and_then(|p| self.layout.content_point(p)) {
                self.offset = (x - cx, y - cy);
            }
        } else {
            self.offset.1 = 0.0;
        }
        self.clamp_offset();
    }

    /// PageUp/PageDown in paged mode: a screenful within the spread, or at the
    /// edge a flip to the adjacent spread, shown from the side facing this one.
    fn page_in_paged_mode(&mut self, action: Action) {
        let Some(spread) = self.current_spread() else {
            return;
        };
        let max_y = self.max_offset().1;
        let down = action == Action::ScrollScreenDown;
        let at_edge = if down {
            self.offset.1 >= max_y - EDGE_EPSILON
        } else {
            self.offset.1 <= EDGE_EPSILON
        };
        if !at_edge {
            let y = scroll_target(
                action,
                self.offset.1,
                self.viewport.1,
                self.layout.content_height(),
            );
            self.scrolled_to(self.offset.0, y);
            return;
        }
        let next = if down {
            spread + 1
        } else {
            match spread.checked_sub(1) {
                Some(previous) => previous,
                None => return,
            }
        };
        if next >= self.layout.spread_count() {
            return;
        }
        self.layout.set_mode(LayoutMode {
            paged: Some(next),
            ..self.layout.mode()
        });
        let scale = self.fit_scale_for(self.mode, next);
        self.layout.arrange(scale, self.viewport.0);
        self.offset.1 = if down { 0.0 } else { f32::MAX };
        self.clamp_offset();
    }

    fn zoom_to(&mut self, scale: f32, anchor: (f32, f32)) {
        self.mode = ZoomMode::Custom;
        self.rescale(scale.clamp(MIN_ZOOM, MAX_ZOOM), anchor, anchor);
    }

    fn centre(&self) -> (f32, f32) {
        (self.viewport.0 / 2.0, self.viewport.1 / 2.0)
    }

    /// Re-arranges at `scale`; the document point that was at viewport point
    /// `from` ends up at viewport point `to`.
    fn rescale(&mut self, scale: f32, from: (f32, f32), to: (f32, f32)) {
        let (ox, oy) = self.offset;
        let anchor = self.layout.doc_point(ox + from.0, oy + from.1);
        self.layout.arrange(scale, self.viewport.0);
        if let Some((x, y)) = anchor.and_then(|p| self.layout.content_point(p)) {
            self.offset = (x - to.0, y - to.1);
        }
        self.clamp_offset();
    }

    fn max_offset(&self) -> (f32, f32) {
        (
            (self.layout.content_width() - self.viewport.0).max(0.0),
            (self.layout.content_height() - self.viewport.1).max(0.0),
        )
    }

    fn clamp_offset(&mut self) {
        let (max_x, max_y) = self.max_offset();
        self.offset = (
            self.offset.0.clamp(0.0, max_x),
            self.offset.1.clamp(0.0, max_y),
        );
    }

    /// The fitted scale for `mode`, using the current spread in paged mode or
    /// the spread at the centre in continuous mode.
    fn fit_scale(&self, mode: ZoomMode) -> f32 {
        let spread = self.current_spread().unwrap_or_else(|| {
            let (cx, cy) = self.centre();
            self.layout
                .doc_point(self.offset.0 + cx, self.offset.1 + cy)
                .map_or(0, |p| self.layout.spread_of(p.page))
        });
        self.fit_scale_for(mode, spread)
    }

    fn fit_scale_for(&self, mode: ZoomMode, spread: usize) -> f32 {
        let gap = self.layout.spread_gap(spread);
        let width = self.viewport.0 - 2.0 * MARGIN - gap;
        let height = self.viewport.1 - 2.0 * MARGIN;
        let ratio = |available: f32, length: f32| (length > 0.0).then(|| available / length);
        let scale = match mode {
            ZoomMode::Custom => Some(self.scale()),
            ZoomMode::FitWidth => ratio(width, self.layout.widest_spread()),
            ZoomMode::FitPage => self.layout.spread_size(spread).and_then(|size| {
                match (ratio(width, size.width), ratio(height, size.height)) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }
            }),
        };
        scale.unwrap_or(1.0).clamp(MIN_ZOOM, MAX_ZOOM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocPoint, PAGE_GAP};

    fn size(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    /// Ten Letter pages in a 636×800 viewport: fit width is exactly 1.0.
    fn letter_view() -> View {
        let mut view = View::new(vec![size(612.0, 792.0); 10]);
        view.set_viewport_size(636.0, 800.0);
        view
    }

    /// The document point at viewport point `at`.
    fn doc_at(view: &View, at: (f32, f32)) -> DocPoint {
        let (ox, oy) = view.offset();
        view.layout().doc_point(ox + at.0, oy + at.1).unwrap()
    }

    /// Where document point `p` now appears in the viewport.
    fn on_screen(view: &View, p: DocPoint) -> (f32, f32) {
        let (cx, cy) = view.layout().content_point(p).unwrap();
        let (ox, oy) = view.offset();
        (cx - ox, cy - oy)
    }

    fn assert_near(a: (f32, f32), b: (f32, f32), tolerance: f32) {
        assert!(
            (a.0 - b.0).abs() <= tolerance && (a.1 - b.1).abs() <= tolerance,
            "{a:?} vs {b:?}"
        );
    }

    #[test]
    fn starts_fit_width_at_top() {
        let view = letter_view();
        assert_eq!(view.scale(), 1.0);
        assert_eq!(view.mode(), ZoomMode::FitWidth);
        assert_eq!(view.offset(), (0.0, 0.0));
    }

    #[test]
    fn zoom_in_steps_and_switches_to_custom() {
        let mut view = letter_view();
        view.apply(Action::ZoomIn);
        assert_eq!(view.scale(), 1.25);
        assert_eq!(view.mode(), ZoomMode::Custom);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut view = letter_view();
        for _ in 0..20 {
            view.apply(Action::ZoomIn);
        }
        assert_eq!(view.scale(), MAX_ZOOM);
        for _ in 0..30 {
            view.apply(Action::ZoomOut);
        }
        assert_eq!(view.scale(), MIN_ZOOM);
    }

    #[test]
    fn keyboard_zoom_keeps_centre() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 2000.0);
        let centre = doc_at(&view, (318.0, 400.0));
        view.apply(Action::ZoomIn);
        assert_near(on_screen(&view, centre), (318.0, 400.0), 0.5);
    }

    #[test]
    fn zoom_by_keeps_pointer_point() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 2000.0);
        let under_pointer = doc_at(&view, (100.0, 100.0));
        view.zoom_by(1.5, Some((100.0, 100.0)));
        assert_eq!(view.scale(), 1.5);
        assert_near(on_screen(&view, under_pointer), (100.0, 100.0), 0.5);
    }

    #[test]
    fn zoom_by_without_anchor_uses_centre() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 2000.0);
        let centre = doc_at(&view, (318.0, 400.0));
        view.zoom_by(1.5, None);
        assert_near(on_screen(&view, centre), (318.0, 400.0), 0.5);
    }

    #[test]
    fn anchor_in_gap_is_stable() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 2000.0);
        // Content y 2418 is in the gap below page 2 (which ends at 2412).
        let pointer = Some((318.0, 418.0));
        view.zoom_by(2.0, pointer);
        view.zoom_by(0.5, pointer);
        // Gaps do not scale, so a point inside one can shift by at most a gap.
        assert_near(view.offset(), (0.0, 2000.0), PAGE_GAP);
    }

    #[test]
    fn zoom_out_at_bottom_clamps() {
        let mut view = letter_view();
        let bottom = view.layout().content_height() - 800.0;
        view.scrolled_to(0.0, bottom);
        view.apply(Action::ZoomOut);
        let max_y = (view.layout().content_height() - 800.0).max(0.0);
        assert_eq!(view.offset().1, max_y);
    }

    #[test]
    fn fit_page_fits_current_page() {
        let mut view = View::new(vec![size(612.0, 792.0), size(300.0, 300.0)]);
        view.set_viewport_size(636.0, 400.0);
        view.apply(Action::FitPage);
        assert_eq!(view.mode(), ZoomMode::FitPage);
        assert_eq!(view.scale(), (400.0 - 24.0) / 792.0);
    }

    #[test]
    fn fit_width_after_custom() {
        let mut view = letter_view();
        view.apply(Action::ZoomIn);
        view.apply(Action::FitWidth);
        assert_eq!(view.scale(), 1.0);
        assert_eq!(view.mode(), ZoomMode::FitWidth);
    }

    #[test]
    fn resize_refits_in_fit_mode_and_keeps_centre() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 3000.0);
        let centre = doc_at(&view, (318.0, 400.0));
        view.set_viewport_size(1248.0, 800.0);
        assert_eq!(view.scale(), 2.0);
        assert_near(on_screen(&view, centre), (624.0, 400.0), 0.5);
    }

    #[test]
    fn resize_keeps_scale_in_custom() {
        let mut view = letter_view();
        view.apply(Action::ZoomIn);
        view.set_viewport_size(1000.0, 600.0);
        assert_eq!(view.scale(), 1.25);
    }

    #[test]
    fn fit_scales_are_clamped() {
        let mut view = letter_view();
        view.set_viewport_size(30_000.0, 30_000.0);
        assert_eq!(view.scale(), MAX_ZOOM);
        view.set_viewport_size(10.0, 10.0);
        assert_eq!(view.scale(), MIN_ZOOM);
    }

    #[test]
    fn page_keys_scroll_a_screen() {
        let mut view = letter_view();
        view.apply(Action::ScrollScreenDown);
        assert_eq!(view.offset(), (0.0, 720.0));
        view.apply(Action::ScrollScreenUp);
        assert_eq!(view.offset(), (0.0, 0.0));
    }

    /// `letter_view` switched to paged mode at the top of page 0.
    fn paged_view() -> View {
        let mut view = letter_view();
        view.apply(Action::ToggleContinuous);
        view
    }

    #[test]
    fn toggle_continuous_shows_centre_page_from_top() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 3000.0);
        let page = doc_at(&view, (318.0, 400.0)).page;
        view.apply(Action::ToggleContinuous);
        assert!(!view.continuous());
        assert_eq!(view.current_spread(), Some(page));
        assert_eq!(view.offset().1, 0.0);
    }

    #[test]
    fn toggle_back_keeps_centre_page_visible() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 3000.0);
        let page = doc_at(&view, (318.0, 400.0)).page;
        view.apply(Action::ToggleContinuous);
        view.apply(Action::ToggleContinuous);
        assert!(view.continuous());
        let rect = view.layout().page_rect(page).unwrap();
        let top = view.offset().1;
        assert!(rect.y < top + 800.0 && rect.y + rect.height > top);
    }

    #[test]
    fn paged_page_down_scrolls_then_flips() {
        let mut view = paged_view();
        assert_eq!(view.current_spread(), Some(0));
        view.apply(Action::ScrollScreenDown);
        assert_eq!((view.current_spread(), view.offset().1), (Some(0), 16.0));
        view.apply(Action::ScrollScreenDown);
        assert_eq!((view.current_spread(), view.offset().1), (Some(1), 0.0));
    }

    #[test]
    fn paged_page_up_flips_to_bottom() {
        let mut view = paged_view();
        view.apply(Action::ScrollScreenDown);
        view.apply(Action::ScrollScreenDown);
        view.apply(Action::ScrollScreenUp);
        assert_eq!((view.current_spread(), view.offset().1), (Some(0), 16.0));
    }

    #[test]
    fn paged_flip_stops_at_ends() {
        let mut view = paged_view();
        view.apply(Action::ScrollScreenUp);
        assert_eq!((view.current_spread(), view.offset().1), (Some(0), 0.0));

        let mut view = letter_view();
        view.scrolled_to(0.0, 1e6);
        view.apply(Action::ToggleContinuous);
        assert_eq!(view.current_spread(), Some(9));
        view.apply(Action::ScrollScreenDown);
        view.apply(Action::ScrollScreenDown);
        assert_eq!((view.current_spread(), view.offset().1), (Some(9), 16.0));
    }

    #[test]
    fn paged_zoomed_scrolls_screenfuls() {
        let mut view = paged_view();
        for _ in 0..3 {
            view.apply(Action::ZoomIn);
        }
        view.scrolled_to(0.0, 0.0);
        let max_y = view.layout().content_height() - 800.0;
        view.apply(Action::ScrollScreenDown);
        assert_eq!((view.current_spread(), view.offset().1), (Some(0), 720.0));
        view.apply(Action::ScrollScreenDown);
        assert_eq!((view.current_spread(), view.offset().1), (Some(0), max_y));
        view.apply(Action::ScrollScreenDown);
        assert_eq!((view.current_spread(), view.offset().1), (Some(1), 0.0));
    }

    #[test]
    fn paged_scroll_clamped_to_page() {
        let mut view = paged_view();
        view.scrolled_to(0.0, 99_999.0);
        assert_eq!(view.offset().1, 16.0);
    }

    #[test]
    fn toggle_dual_refits_fit_width() {
        let mut view = letter_view();
        view.apply(Action::ToggleDual);
        assert!(view.dual());
        assert_eq!(view.scale(), (636.0 - 24.0 - 12.0) / 1224.0);
    }

    #[test]
    fn fit_page_in_paged_dual_fits_spread() {
        let mut view = View::new(vec![size(612.0, 792.0); 10]);
        view.set_viewport_size(1260.0, 600.0);
        view.apply(Action::ToggleDual);
        view.apply(Action::ToggleContinuous);
        view.apply(Action::FitPage);
        let expected = ((1260.0 - 24.0 - 12.0) / 1224.0_f32).min((600.0 - 24.0) / 792.0);
        assert_eq!(view.scale(), expected);
    }

    #[test]
    fn zero_pages_is_safe() {
        let mut view = View::new(vec![]);
        view.set_viewport_size(636.0, 800.0);
        for action in [
            Action::ZoomIn,
            Action::ZoomOut,
            Action::FitWidth,
            Action::FitPage,
            Action::ScrollScreenDown,
            Action::ScrollScreenUp,
            Action::ToggleContinuous,
            Action::ScrollScreenDown,
            Action::ScrollScreenUp,
            Action::ToggleDual,
            Action::FitPage,
            Action::ToggleContinuous,
        ] {
            view.apply(action);
        }
        view.zoom_by(2.0, Some((10.0, 10.0)));
        view.scrolled_to(100.0, 100.0);
        assert_eq!(view.offset(), (0.0, 0.0));
    }
}
