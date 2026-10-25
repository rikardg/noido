use noido_engine::Size;

use crate::{Action, DocPoint, Layout, LayoutMode, MARGIN, scroll_target};

/// Smallest and largest zoom, in UI points per PDF point.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 4.0;
/// Factor per keyboard zoom step.
pub const ZOOM_STEP: f32 = 1.25;
/// How far the arrow keys scroll, in points: egui's scroll per mouse wheel
/// line on native.
pub const SCROLL_STEP: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomMode {
    /// Pages fill the viewport width.
    FitWidth,
    /// The page at the viewport centre fits the viewport.
    FitPage,
    /// A fixed scale set by zooming.
    Custom,
}

/// What is remembered about a view between runs: zoom, arrangement and the
/// document point at the viewport centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub zoom: ZoomMode,
    /// The scale; only used to restore custom zoom (fit modes refit).
    pub scale: f32,
    pub continuous: bool,
    pub dual: bool,
    pub centre: DocPoint,
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
    /// A restored centre point waiting for the first non-empty viewport.
    pending: Option<DocPoint>,
}

impl View {
    pub fn new(page_sizes: Vec<Size>) -> Self {
        let mut view = Self {
            layout: Layout::new(page_sizes),
            mode: ZoomMode::FitWidth,
            viewport: (0.0, 0.0),
            offset: (0.0, 0.0),
            pending: None,
        };
        let scale = view.fit_scale(ZoomMode::FitWidth);
        view.layout.arrange(scale, 0.0, 0.0);
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
        if was_empty && self.pending.is_some() {
            self.place_pending();
        } else if was_empty {
            // Nothing was on screen yet: start at the top.
            self.layout.arrange(scale, width, height);
            self.clamp_offset();
        } else {
            self.rescale(scale, old_centre, self.centre());
        }
    }

    /// The page at the viewport centre; `None` with zero pages.
    pub fn centre_page(&self) -> Option<usize> {
        self.state().map(|state| state.centre.page)
    }

    /// A snapshot to restore in a later run; `None` with zero pages.
    pub fn state(&self) -> Option<ViewState> {
        if self.layout.page_count() == 0 {
            return None;
        }
        let (cx, cy) = self.centre();
        let centre = self
            .pending
            .or_else(|| {
                self.layout
                    .doc_point(self.offset.0 + cx, self.offset.1 + cy)
            })
            .unwrap_or(DocPoint {
                page: 0,
                x: 0.0,
                y: 0.0,
            });
        Some(ViewState {
            zoom: self.mode,
            scale: self.scale(),
            continuous: self.continuous(),
            dual: self.dual(),
            centre,
        })
    }

    /// Restores a snapshot from [`View::state`], clamped to this document.
    /// The centre point is placed now, or on the first non-empty viewport.
    pub fn restore(&mut self, state: &ViewState) {
        let count = self.layout.page_count();
        if count == 0 {
            return;
        }
        let page = state.centre.page.min(count - 1);
        let size = self.layout.page_size(page).unwrap_or(Size {
            width: 0.0,
            height: 0.0,
        });
        // A hand-edited or damaged value must not move the view off the page.
        let on_page = |v: f32, length: f32| {
            if v.is_finite() {
                v.clamp(0.0, length.max(0.0))
            } else {
                0.0
            }
        };
        let centre = DocPoint {
            page,
            x: on_page(state.centre.x, size.width),
            y: on_page(state.centre.y, size.height),
        };
        let dual = state.dual;
        self.layout.set_mode(LayoutMode { dual, paged: None });
        let paged = (!state.continuous).then(|| self.layout.spread_of(centre.page));
        self.layout.set_mode(LayoutMode { dual, paged });
        self.mode = state.zoom;
        if state.zoom == ZoomMode::Custom {
            let scale = if state.scale.is_finite() {
                state.scale.clamp(MIN_ZOOM, MAX_ZOOM)
            } else {
                1.0
            };
            self.layout.arrange(scale, self.viewport.0, self.viewport.1);
        }
        self.pending = Some(centre);
        if self.viewport.0 > 0.0 && self.viewport.1 > 0.0 {
            self.place_pending();
        }
    }

    /// Fits the view and puts the pending restored point at the centre.
    fn place_pending(&mut self) {
        let Some(point) = self.pending.take() else {
            return;
        };
        let scale = self.fit_scale_for(self.mode, self.layout.spread_of(point.page));
        self.layout.arrange(scale, self.viewport.0, self.viewport.1);
        if let Some((x, y)) = self.layout.content_point(point) {
            let (cx, cy) = self.centre();
            self.offset = (x - cx, y - cy);
        }
        self.clamp_offset();
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
            // Like the wheel: flips where it flips, else a small step.
            // Sideways only when there's no room to scroll sideways either.
            Action::ScrollUp if self.wheel_flips() => self.wheel_flip(false),
            Action::ScrollDown if self.wheel_flips() => self.wheel_flip(true),
            Action::ScrollLeft if self.wheel_flips() && self.fits_width() => self.wheel_flip(false),
            Action::ScrollRight if self.wheel_flips() && self.fits_width() => self.wheel_flip(true),
            Action::ScrollUp => self.scrolled_to(self.offset.0, self.offset.1 - SCROLL_STEP),
            Action::ScrollDown => self.scrolled_to(self.offset.0, self.offset.1 + SCROLL_STEP),
            Action::ScrollLeft => self.scrolled_to(self.offset.0 - SCROLL_STEP, self.offset.1),
            Action::ScrollRight => self.scrolled_to(self.offset.0 + SCROLL_STEP, self.offset.1),
            // Not about the page view; frontends handle these.
            Action::ToggleSidebar
            | Action::FocusSearch
            | Action::NextHit
            | Action::PreviousHit
            | Action::Quit => {}
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
        self.layout.arrange(scale, self.viewport.0, self.viewport.1);

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
        self.flip(spread, down);
    }

    /// A wheel step in paged mode: flips to the adjacent spread when the
    /// current one fits the viewport vertically. Otherwise the wheel scrolls
    /// within the spread, which the frontend handles, so this does nothing.
    pub fn wheel_flip(&mut self, down: bool) {
        if let Some(spread) = self.current_spread()
            && self.wheel_flips()
        {
            self.flip(spread, down);
        }
    }

    /// Whether the wheel flips spreads ([`View::wheel_flip`]) rather than
    /// scrolling: paged mode with the spread fitting the viewport vertically.
    pub fn wheel_flips(&self) -> bool {
        !self.continuous() && self.max_offset().1 <= EDGE_EPSILON
    }

    /// Whether the content fits the viewport sideways.
    fn fits_width(&self) -> bool {
        self.max_offset().0 <= EDGE_EPSILON
    }

    /// Shows the spread after (`down`) or before `spread`, from the side facing
    /// it: the top when going down, the bottom when going up. Does nothing at
    /// either end of the document.
    fn flip(&mut self, spread: usize, down: bool) {
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
        self.show_spread(next, down);
    }

    /// Shows `spread` in paged mode, refitted, from the top or the bottom.
    fn show_spread(&mut self, spread: usize, from_top: bool) {
        self.layout.set_mode(LayoutMode {
            paged: Some(spread),
            ..self.layout.mode()
        });
        let scale = self.fit_scale_for(self.mode, spread);
        self.layout.arrange(scale, self.viewport.0, self.viewport.1);
        self.offset.1 = if from_top { 0.0 } else { f32::MAX };
        self.clamp_offset();
    }

    /// Goes to `page` (the last page if past it) without changing zoom or
    /// arrangement: in continuous mode its top lands at the top of the
    /// viewport, as far as scrolling allows; in paged mode its spread is
    /// shown from the top.
    pub fn go_to_page(&mut self, page: usize) {
        let Some(last) = self.layout.page_count().checked_sub(1) else {
            return;
        };
        let page = page.min(last);
        if self.continuous() {
            if let Some(rect) = self.layout.page_rect(page) {
                self.scrolled_to(self.offset.0, rect.y - MARGIN);
            }
        } else {
            self.show_spread(self.layout.spread_of(page), true);
        }
    }

    /// Brings `rects`, in PDF points on `page`, into view without changing
    /// zoom: in paged mode the page's spread is shown first. Already fully
    /// visible, nothing moves; otherwise their union is centred along each
    /// axis it doesn't fit in already.
    pub fn reveal(&mut self, page: usize, rects: &[noido_text::Rect]) {
        if page >= self.layout.page_count() || rects.is_empty() {
            return;
        }
        if self.layout.page_rect(page).is_none() {
            self.show_spread(self.layout.spread_of(page), true);
        }
        let Some(on_page) = self.layout.page_rect(page) else {
            return;
        };
        let scale = self.layout.scale();
        let left = rects.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
        let top = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let right = rects
            .iter()
            .map(|r| r.x + r.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = rects
            .iter()
            .map(|r| r.y + r.height)
            .fold(f32::NEG_INFINITY, f32::max);
        let span = |from: f32, to: f32, origin: f32| (origin + from * scale, origin + to * scale);
        let (left, right) = span(left, right, on_page.x);
        let (top, bottom) = span(top, bottom, on_page.y);
        let (mut x, mut y) = self.offset;
        let (width, height) = self.viewport;
        if left < x || right > x + width {
            x = (left + right - width) / 2.0;
        }
        if top < y || bottom > y + height {
            y = (top + bottom - height) / 2.0;
        }
        self.scrolled_to(x, y);
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
        self.layout.arrange(scale, self.viewport.0, self.viewport.1);
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
    use crate::PAGE_GAP;

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
    fn wheel_flips_when_page_fits() {
        let mut view = paged_view();
        view.apply(Action::FitPage);
        view.wheel_flip(true);
        assert_eq!(view.current_spread(), Some(1));
        view.wheel_flip(false);
        assert_eq!(view.current_spread(), Some(0));
    }

    #[test]
    fn wheel_does_not_flip_when_page_is_taller() {
        // Fit width leaves the Letter page 16 points taller than the window.
        let mut view = paged_view();
        view.wheel_flip(true);
        assert_eq!(view.current_spread(), Some(0));
    }

    #[test]
    fn wheel_flip_stops_at_ends() {
        let mut view = paged_view();
        view.apply(Action::FitPage);
        view.wheel_flip(false);
        assert_eq!(view.current_spread(), Some(0));
    }

    #[test]
    fn wheel_flip_ignored_in_continuous_mode() {
        let mut view = letter_view();
        view.apply(Action::FitPage);
        let before = view.offset();
        view.wheel_flip(true);
        assert_eq!(view.offset(), before);
    }

    #[test]
    fn arrows_scroll_a_step_in_continuous_mode() {
        let mut view = letter_view();
        view.apply(Action::ScrollDown);
        assert_eq!(view.offset().1, SCROLL_STEP);
        view.apply(Action::ScrollUp);
        view.apply(Action::ScrollUp);
        assert_eq!(view.offset().1, 0.0);
    }

    #[test]
    fn arrows_flip_when_page_fits() {
        let mut view = paged_view();
        view.apply(Action::FitPage);
        view.apply(Action::ScrollDown);
        assert_eq!(view.current_spread(), Some(1));
        view.apply(Action::ScrollRight);
        assert_eq!(view.current_spread(), Some(2));
        view.apply(Action::ScrollLeft);
        assert_eq!(view.current_spread(), Some(1));
        view.apply(Action::ScrollUp);
        assert_eq!(view.current_spread(), Some(0));
        view.apply(Action::ScrollLeft);
        assert_eq!(view.current_spread(), Some(0));
    }

    #[test]
    fn arrows_scroll_tall_page_in_paged_mode() {
        // Fit width leaves the Letter page 16 points taller than the window.
        let mut view = paged_view();
        view.apply(Action::ScrollDown);
        assert_eq!(view.current_spread(), Some(0));
        assert!(view.offset().1 > 0.0);
    }

    #[test]
    fn arrows_scroll_sideways_when_zoomed_in() {
        let mut view = letter_view();
        for _ in 0..3 {
            view.apply(Action::ZoomIn);
        }
        let (x, y) = view.offset();
        view.apply(Action::ScrollRight);
        assert_eq!(view.offset(), (x + SCROLL_STEP, y));
        view.apply(Action::ScrollLeft);
        assert_eq!(view.offset(), (x, y));
    }

    #[test]
    fn left_right_scroll_wide_page_that_fits_vertically() {
        // Landscape pages: zoomed in, wider than the window but not taller.
        let mut view = View::new(vec![size(792.0, 612.0); 10]);
        view.set_viewport_size(636.0, 800.0);
        view.apply(Action::ToggleContinuous);
        view.apply(Action::FitPage);
        view.apply(Action::ZoomIn);
        assert!(view.wheel_flips());
        let (x, y) = view.offset();
        view.apply(Action::ScrollRight);
        assert_eq!(view.current_spread(), Some(0));
        assert_eq!(view.offset(), (x + SCROLL_STEP, y));
    }

    #[test]
    fn left_right_do_nothing_when_page_fits_width() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 1000.0);
        let before = view.offset();
        view.apply(Action::ScrollRight);
        view.apply(Action::ScrollLeft);
        assert_eq!(view.offset(), before);
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> noido_text::Rect {
        noido_text::Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The centre of `r` on `page`, in viewport points.
    fn centre_on_screen(view: &View, page: usize, r: noido_text::Rect) -> (f32, f32) {
        on_screen(
            view,
            DocPoint {
                page,
                x: r.x + r.width / 2.0,
                y: r.y + r.height / 2.0,
            },
        )
    }

    #[test]
    fn reveal_visible_hit_does_not_move() {
        let mut view = letter_view();
        view.reveal(0, &[rect(72.0, 100.0, 50.0, 12.0)]);
        assert_eq!(view.offset(), (0.0, 0.0));
    }

    #[test]
    fn reveal_centres_hit_vertically() {
        let mut view = letter_view();
        let hit = rect(72.0, 300.0, 50.0, 12.0);
        view.reveal(5, &[hit]);
        let (_, y) = centre_on_screen(&view, 5, hit);
        assert!((y - 400.0).abs() < 1.0, "{y}");
    }

    #[test]
    fn reveal_keeps_zoom() {
        let mut view = letter_view();
        view.apply(Action::ZoomIn);
        let scale = view.scale();
        view.reveal(5, &[rect(72.0, 300.0, 50.0, 12.0)]);
        assert_eq!(view.scale(), scale);
        assert_eq!(view.mode(), ZoomMode::Custom);
    }

    #[test]
    fn reveal_centres_horizontally_when_zoomed() {
        let mut view = letter_view();
        while view.scale() < 2.0 {
            view.apply(Action::ZoomIn);
        }
        view.scrolled_to(0.0, 0.0);
        // Off screen to the right, but far enough from the edge to centre.
        let hit = rect(380.0, 300.0, 40.0, 12.0);
        view.reveal(2, &[hit]);
        let (x, y) = centre_on_screen(&view, 2, hit);
        assert!((x - 318.0).abs() < 1.0, "{x}");
        assert!((y - 400.0).abs() < 1.0, "{y}");
    }

    #[test]
    fn reveal_two_rects_uses_their_union() {
        let mut view = letter_view();
        let (a, b) = (
            rect(500.0, 300.0, 40.0, 12.0),
            rect(72.0, 320.0, 30.0, 12.0),
        );
        view.reveal(6, &[a, b]);
        let (_, top) = on_screen(
            &view,
            DocPoint {
                page: 6,
                x: 0.0,
                y: 300.0,
            },
        );
        let (_, bottom) = on_screen(
            &view,
            DocPoint {
                page: 6,
                x: 0.0,
                y: 332.0,
            },
        );
        assert!(((top + bottom) / 2.0 - 400.0).abs() < 1.0, "{top} {bottom}");
    }

    #[test]
    fn reveal_in_paged_mode_shows_spread() {
        let mut view = letter_view();
        view.apply(Action::ToggleContinuous);
        view.reveal(7, &[rect(72.0, 300.0, 50.0, 12.0)]);
        assert_eq!(view.current_spread(), Some(7));
        assert!(!view.continuous());
    }

    #[test]
    fn reveal_in_paged_zoomed_scrolls_within_page() {
        let mut view = letter_view();
        view.apply(Action::ToggleContinuous);
        for _ in 0..4 {
            view.apply(Action::ZoomIn);
        }
        // Below the view, but far enough from the bottom to centre.
        let hit = rect(72.0, 400.0, 50.0, 12.0);
        view.reveal(3, &[hit]);
        assert_eq!(view.current_spread(), Some(3));
        let (_, y) = centre_on_screen(&view, 3, hit);
        assert!((y - 400.0).abs() < 1.0, "{y}");
    }

    #[test]
    fn reveal_hit_larger_than_view_centres() {
        let mut view = letter_view();
        let hit = rect(10.0, 0.0, 590.0, 790.0);
        view.reveal(4, &[hit]);
        let (_, y) = centre_on_screen(&view, 4, hit);
        assert!((y - 400.0).abs() < 1.0, "{y}");
    }

    #[test]
    fn reveal_bad_page_is_ignored() {
        let mut view = letter_view();
        view.reveal(99, &[rect(72.0, 300.0, 50.0, 12.0)]);
        view.reveal(3, &[]);
        assert_eq!(view.offset(), (0.0, 0.0));
    }

    #[test]
    fn go_to_page_continuous_puts_top_at_top() {
        let mut view = letter_view();
        view.go_to_page(4);
        let rect = view.layout().page_rect(4).unwrap();
        assert!((view.offset().1 - (rect.y - MARGIN)).abs() < 0.01);
        assert_eq!(view.mode(), ZoomMode::FitWidth);
        assert_eq!(view.scale(), 1.0);
        assert!(view.continuous());
    }

    #[test]
    fn go_to_page_keeps_custom_zoom() {
        let mut view = letter_view();
        view.apply(Action::ZoomIn);
        view.apply(Action::ZoomIn);
        let scale = view.scale();
        view.go_to_page(3);
        assert_eq!(view.mode(), ZoomMode::Custom);
        assert_eq!(view.scale(), scale);
        let rect = view.layout().page_rect(3).unwrap();
        assert!((rect.y - view.offset().1 - MARGIN).abs() < 0.01);
    }

    #[test]
    fn go_to_page_paged_shows_spread() {
        let mut view = paged_view();
        view.apply(Action::ScrollScreenDown);
        view.go_to_page(6);
        assert!(!view.continuous());
        assert_eq!(view.current_spread(), Some(6));
        assert_eq!(view.offset().1, 0.0);
    }

    #[test]
    fn go_to_page_paged_dual_shows_spread_of_page() {
        let mut view = paged_view();
        view.apply(Action::ToggleDual);
        view.go_to_page(4);
        assert!(view.dual());
        assert_eq!(view.current_spread(), Some(view.layout().spread_of(4)));
        assert_eq!(view.offset().1, 0.0);
    }

    #[test]
    fn go_to_last_page_clamps() {
        let mut view = letter_view();
        view.go_to_page(99);
        let rect = view.layout().page_rect(9).unwrap();
        assert_eq!(view.offset().1, rect.y - MARGIN);
        // Zoomed out, the last page can't reach the top: scrolled to the end.
        for _ in 0..3 {
            view.apply(Action::ZoomOut);
        }
        view.go_to_page(9);
        assert_eq!(view.offset().1, view.max_offset().1);
        let mut view = paged_view();
        view.go_to_page(99);
        assert_eq!(
            view.current_spread(),
            Some(view.layout().spread_count() - 1)
        );
    }

    #[test]
    fn go_to_page_with_zero_pages_is_safe() {
        let mut view = View::new(vec![]);
        view.set_viewport_size(636.0, 800.0);
        view.go_to_page(0);
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
        view.wheel_flip(true);
        view.scrolled_to(100.0, 100.0);
        assert_eq!(view.offset(), (0.0, 0.0));
    }

    fn saved(zoom: ZoomMode, scale: f32, continuous: bool, dual: bool, page: usize) -> ViewState {
        ViewState {
            zoom,
            scale,
            continuous,
            dual,
            centre: DocPoint {
                page,
                // The horizontal centre of a fit-width page: reachable.
                x: 306.0,
                y: 400.0,
            },
        }
    }

    fn assert_same_state(a: ViewState, b: ViewState) {
        assert_eq!(
            (a.zoom, a.continuous, a.dual, a.centre.page),
            (b.zoom, b.continuous, b.dual, b.centre.page)
        );
        assert!((a.scale - b.scale).abs() < 1e-4, "{a:?} vs {b:?}");
        assert_near((a.centre.x, a.centre.y), (b.centre.x, b.centre.y), 0.5);
    }

    #[test]
    fn state_round_trips_in_every_mode() {
        for (continuous, dual, zoom_in) in [
            (true, false, false),
            (true, true, false),
            (false, false, false),
            (false, true, false),
            (true, false, true),
            (false, true, true),
        ] {
            let mut view = letter_view();
            if zoom_in {
                view.apply(Action::ZoomIn);
            }
            view.scrolled_to(0.0, 3000.0);
            if dual {
                view.apply(Action::ToggleDual);
            }
            if !continuous {
                view.apply(Action::ToggleContinuous);
            }
            view.scrolled_to(10.0, 40.0);
            let state = view.state().unwrap();

            let mut restored = letter_view();
            restored.restore(&state);
            assert_same_state(restored.state().unwrap(), state);
        }
    }

    #[test]
    fn restore_before_viewport_lands_on_centre() {
        let state = saved(ZoomMode::FitWidth, 1.0, true, false, 6);
        let mut view = View::new(vec![size(612.0, 792.0); 10]);
        view.restore(&state);
        view.set_viewport_size(636.0, 800.0);
        let centre = doc_at(&view, (318.0, 400.0));
        assert_eq!(centre.page, 6);
        assert_near((centre.x, centre.y), (306.0, 400.0), 0.5);
    }

    #[test]
    fn restore_fit_width_refits_new_window() {
        let state = letter_view().state().unwrap();
        let mut view = View::new(vec![size(612.0, 792.0); 10]);
        view.restore(&state);
        view.set_viewport_size(1248.0, 800.0);
        assert_eq!(view.mode(), ZoomMode::FitWidth);
        assert_eq!(view.scale(), 2.0);
    }

    #[test]
    fn restore_clamps_stale_state() {
        let mut view = letter_view();
        view.restore(&saved(ZoomMode::Custom, 50.0, true, false, 99));
        assert_eq!(view.scale(), MAX_ZOOM);
        assert_eq!(doc_at(&view, (318.0, 400.0)).page, 9);
    }

    #[test]
    fn restore_paged_shows_saved_spread() {
        let mut view = letter_view();
        view.restore(&saved(ZoomMode::FitWidth, 1.0, false, true, 5));
        assert!(view.dual());
        assert_eq!(view.current_spread(), Some(2));
    }

    #[test]
    fn restore_ignores_non_finite_values() {
        let mut state = saved(ZoomMode::Custom, f32::NAN, true, false, 4);
        state.centre.x = f32::NAN;
        state.centre.y = f32::INFINITY;
        let mut view = View::new(vec![size(612.0, 792.0); 10]);
        view.restore(&state);
        view.set_viewport_size(636.0, 800.0);
        let restored = view.state().unwrap();
        assert!(restored.scale.is_finite());
        assert!(restored.centre.x.is_finite() && restored.centre.y.is_finite());
        let (x, y) = view.offset();
        assert!(x.is_finite() && y.is_finite());
        assert_eq!(restored.centre.page, 4);
    }

    #[test]
    fn restore_keeps_point_on_its_page() {
        let mut state = saved(ZoomMode::FitWidth, 1.0, true, false, 4);
        state.centre.y = 5000.0;
        let mut view = letter_view();
        view.restore(&state);
        assert_eq!(doc_at(&view, (318.0, 400.0)).page, 4);
    }

    #[test]
    fn centre_page_follows_scroll() {
        let mut view = letter_view();
        view.scrolled_to(0.0, 3000.0);
        assert_eq!(view.centre_page(), Some(doc_at(&view, (318.0, 400.0)).page));
        assert_eq!(view.centre_page(), Some(4));
        assert_eq!(View::new(vec![]).centre_page(), None);
    }

    #[test]
    fn restore_with_zero_pages_does_nothing() {
        let mut view = View::new(vec![]);
        view.restore(&saved(ZoomMode::Custom, 2.0, false, true, 3));
        view.set_viewport_size(636.0, 800.0);
        assert!(view.state().is_none());
        assert!(view.continuous());
        assert_eq!(view.offset(), (0.0, 0.0));
    }
}
