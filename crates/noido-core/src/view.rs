use noido_engine::Size;

use crate::{Action, Layout, MARGIN, scroll_target};

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

/// What the reader sees: zoom, viewport size and scroll position. Scale
/// changes keep an anchor point of the document still on screen.
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

    fn clamp_offset(&mut self) {
        let max_x = (self.layout.content_width() - self.viewport.0).max(0.0);
        let max_y = (self.layout.content_height() - self.viewport.1).max(0.0);
        self.offset = (
            self.offset.0.clamp(0.0, max_x),
            self.offset.1.clamp(0.0, max_y),
        );
    }

    fn fit_scale(&self, mode: ZoomMode) -> f32 {
        let (width, height) = (
            self.viewport.0 - 2.0 * MARGIN,
            self.viewport.1 - 2.0 * MARGIN,
        );
        let ratio = |available: f32, length: f32| (length > 0.0).then(|| available / length);
        let scale = match mode {
            ZoomMode::Custom => Some(self.scale()),
            ZoomMode::FitWidth => ratio(width, self.layout.widest_page()),
            ZoomMode::FitPage => {
                let (cx, cy) = self.centre();
                let page = self
                    .layout
                    .doc_point(self.offset.0 + cx, self.offset.1 + cy)
                    .map_or(0, |p| p.page);
                self.layout.page_size(page).and_then(|size| {
                    match (ratio(width, size.width), ratio(height, size.height)) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    }
                })
            }
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
        ] {
            view.apply(action);
        }
        view.zoom_by(2.0, Some((10.0, 10.0)));
        view.scrolled_to(100.0, 100.0);
        assert_eq!(view.offset(), (0.0, 0.0));
    }
}
