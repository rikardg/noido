use std::ops::Range;

use noido_engine::Size;

/// Space between pages, in UI points.
pub const PAGE_GAP: f32 = 12.0;
/// Space around the column of pages, in UI points.
pub const MARGIN: f32 = 12.0;
/// Longest side of a rendered page image, in pixels.
pub const MAX_RENDER_SIDE: u32 = 8192;

/// A page's place in the scroll content, in UI points, origin top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Continuous vertical layout: pages stacked top to bottom, fitted to the available width.
pub struct Layout {
    page_sizes: Vec<Size>,
    widest: f32,
    scale: f32,
    rects: Vec<PageRect>,
}

impl Layout {
    pub fn new(page_sizes: Vec<Size>) -> Self {
        let widest = page_sizes.iter().map(|s| s.width).fold(0.0, f32::max);
        let mut layout = Self {
            page_sizes,
            widest,
            scale: 0.0,
            rects: Vec::new(),
        };
        layout.set_available_width(0.0);
        layout
    }

    pub fn set_available_width(&mut self, width: f32) {
        self.scale = if self.widest > 0.0 {
            ((width - 2.0 * MARGIN) / self.widest).max(0.0)
        } else {
            0.0
        };
        let column = self.widest * self.scale;
        let mut y = MARGIN;
        self.rects = self
            .page_sizes
            .iter()
            .map(|size| {
                let (w, h) = (size.width * self.scale, size.height * self.scale);
                let rect = PageRect {
                    x: MARGIN + (column - w) / 2.0,
                    y,
                    width: w,
                    height: h,
                };
                y += h + PAGE_GAP;
                rect
            })
            .collect();
    }

    /// UI points per PDF point.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn page_rect(&self, page: usize) -> Option<PageRect> {
        self.rects.get(page).copied()
    }

    pub fn content_height(&self) -> f32 {
        self.rects.last().map_or(0.0, |r| r.y + r.height + MARGIN)
    }

    /// Pages whose rectangle intersects the vertical span `[top, bottom)`.
    pub fn visible_pages(&self, top: f32, bottom: f32) -> Range<usize> {
        let start = self.rects.partition_point(|r| r.y + r.height <= top);
        let end = self.rects.partition_point(|r| r.y < bottom);
        start..end.max(start)
    }
}

/// The scale to render `page` at: `display_scale`, reduced if needed so the
/// longest side stays within `max_side` pixels.
pub fn render_scale(page: Size, display_scale: f32, max_side: u32) -> f32 {
    let longest = page.width.max(page.height) * display_scale;
    if longest > max_side as f32 {
        max_side as f32 / page.width.max(page.height)
    } else {
        display_scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    fn two_pages() -> Layout {
        let mut layout = Layout::new(vec![size(612.0, 792.0), size(306.0, 396.0)]);
        layout.set_available_width(636.0);
        layout
    }

    #[test]
    fn fit_width_scale() {
        let layout = two_pages();
        assert_eq!(layout.scale(), 1.0);
        assert_eq!(
            layout.page_rect(0),
            Some(PageRect {
                x: 12.0,
                y: 12.0,
                width: 612.0,
                height: 792.0
            })
        );
        assert_eq!(
            layout.page_rect(1),
            Some(PageRect {
                x: 12.0 + 153.0,
                y: 12.0 + 792.0 + 12.0,
                width: 306.0,
                height: 396.0
            })
        );
        assert_eq!(layout.content_height(), 12.0 + 792.0 + 12.0 + 396.0 + 12.0);
    }

    #[test]
    fn visible_pages_ranges() {
        let layout = two_pages();
        assert!(layout.visible_pages(0.0, 10.0).is_empty());
        assert_eq!(layout.visible_pages(0.0, 100.0), 0..1);
        assert_eq!(layout.visible_pages(800.0, 820.0), 0..2);
        assert!(layout.visible_pages(10_000.0, 10_100.0).is_empty());
    }

    #[test]
    fn no_pages_is_safe() {
        let mut layout = Layout::new(vec![]);
        layout.set_available_width(500.0);
        assert_eq!(layout.scale(), 0.0);
        assert_eq!(layout.content_height(), 0.0);
        assert_eq!(layout.page_rect(0), None);
        assert!(layout.visible_pages(0.0, 1000.0).is_empty());
    }

    #[test]
    fn zero_width_page_is_safe() {
        let mut layout = Layout::new(vec![size(0.0, 100.0)]);
        layout.set_available_width(500.0);
        assert_eq!(layout.scale(), 0.0);
        assert!(layout.page_rect(0).is_some());
    }

    #[test]
    fn narrow_window_never_negative() {
        let mut layout = two_pages();
        layout.set_available_width(10.0);
        assert!(layout.scale() >= 0.0);
    }

    #[test]
    fn render_scale_caps_longest_side() {
        let s = render_scale(size(100.0, 20_000.0), 2.0, 8192);
        assert!(20_000.0 * s <= 8192.0 && 20_000.0 * s > 8191.0, "scale {s}");
        assert_eq!(render_scale(size(612.0, 792.0), 2.0, 8192), 2.0);
    }
}
