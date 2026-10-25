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

/// A point on a page in PDF points from the page's top-left. Points in the
/// margins or gaps around pages are relative to the nearest page above, so
/// `x` and `y` may lie outside the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DocPoint {
    pub page: usize,
    pub x: f32,
    pub y: f32,
}

/// Continuous vertical layout: pages stacked top to bottom at a given scale,
/// centred horizontally.
pub struct Layout {
    page_sizes: Vec<Size>,
    widest: f32,
    scale: f32,
    content_width: f32,
    rects: Vec<PageRect>,
}

impl Layout {
    pub fn new(page_sizes: Vec<Size>) -> Self {
        let widest = page_sizes.iter().map(|s| s.width).fold(0.0, f32::max);
        let mut layout = Self {
            page_sizes,
            widest,
            scale: 0.0,
            content_width: 0.0,
            rects: Vec::new(),
        };
        layout.arrange(0.0, 0.0);
        layout
    }

    /// Lays the pages out at `scale` UI points per PDF point. The content is at
    /// least `available_width` wide.
    pub fn arrange(&mut self, scale: f32, available_width: f32) {
        self.scale = scale;
        self.content_width = available_width.max(self.widest * scale + 2.0 * MARGIN);
        let mut y = MARGIN;
        self.rects = self
            .page_sizes
            .iter()
            .map(|size| {
                let (w, h) = (size.width * scale, size.height * scale);
                let rect = PageRect {
                    x: (self.content_width - w) / 2.0,
                    y,
                    width: w,
                    height: h,
                };
                y += h + PAGE_GAP;
                rect
            })
            .collect();
    }

    /// UI points per PDF point, as last arranged.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes.len()
    }

    pub fn page_size(&self, page: usize) -> Option<Size> {
        self.page_sizes.get(page).copied()
    }

    /// Width of the widest page in PDF points.
    pub fn widest_page(&self) -> f32 {
        self.widest
    }

    pub fn page_rect(&self, page: usize) -> Option<PageRect> {
        self.rects.get(page).copied()
    }

    pub fn content_width(&self) -> f32 {
        self.content_width
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

    /// The document point at content point (`x`, `y`).
    pub fn doc_point(&self, x: f32, y: f32) -> Option<DocPoint> {
        if self.scale <= 0.0 || self.rects.is_empty() {
            return None;
        }
        let page = self.rects.partition_point(|r| r.y <= y).saturating_sub(1);
        let rect = self.rects[page];
        Some(DocPoint {
            page,
            x: (x - rect.x) / self.scale,
            y: (y - rect.y) / self.scale,
        })
    }

    /// The content point of document point `p`.
    pub fn content_point(&self, p: DocPoint) -> Option<(f32, f32)> {
        let rect = self.rects.get(p.page)?;
        Some((rect.x + p.x * self.scale, rect.y + p.y * self.scale))
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
        layout.arrange(1.0, 636.0);
        layout
    }

    #[test]
    fn arranges_at_scale() {
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
        assert_eq!(layout.content_width(), 636.0);
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
        layout.arrange(1.0, 500.0);
        assert_eq!(layout.content_height(), 0.0);
        assert_eq!(layout.page_rect(0), None);
        assert!(layout.visible_pages(0.0, 1000.0).is_empty());
        assert_eq!(layout.doc_point(10.0, 10.0), None);
    }

    #[test]
    fn zero_width_page_is_safe() {
        let mut layout = Layout::new(vec![size(0.0, 100.0)]);
        layout.arrange(1.0, 500.0);
        assert!(layout.page_rect(0).is_some());
        assert_eq!(layout.widest_page(), 0.0);
    }

    #[test]
    fn wide_pages_extend_content() {
        let mut layout = Layout::new(vec![size(612.0, 792.0)]);
        layout.arrange(2.0, 500.0);
        assert_eq!(layout.content_width(), 1248.0);
        assert_eq!(layout.page_rect(0).unwrap().x, 12.0);
    }

    #[test]
    fn narrow_pages_centre_in_window() {
        let mut layout = Layout::new(vec![size(612.0, 792.0)]);
        layout.arrange(0.5, 1000.0);
        assert_eq!(layout.page_rect(0).unwrap().x, (1000.0 - 306.0) / 2.0);
    }

    #[test]
    fn doc_point_round_trip() {
        let layout = two_pages();
        let p = layout.doc_point(300.0, 900.0).unwrap();
        assert_eq!(p.page, 1);
        let (x, y) = layout.content_point(p).unwrap();
        assert!((x - 300.0).abs() < 1e-3 && (y - 900.0).abs() < 1e-3);
    }

    #[test]
    fn doc_point_in_gap_refers_to_page_above() {
        let layout = two_pages();
        let p = layout.doc_point(100.0, 12.0 + 792.0 + 6.0).unwrap();
        assert_eq!(p.page, 0);
        assert!((p.y - 798.0).abs() < 1e-3);
    }

    #[test]
    fn render_scale_caps_longest_side() {
        let s = render_scale(size(100.0, 20_000.0), 2.0, 8192);
        assert!(20_000.0 * s <= 8192.0 && 20_000.0 * s > 8191.0, "scale {s}");
        assert_eq!(render_scale(size(612.0, 792.0), 2.0, 8192), 2.0);
    }
}
