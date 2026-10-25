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

/// How pages are arranged: one per row or two-page spreads, and in paged
/// mode which spread is shown (`None` shows all, stacked).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LayoutMode {
    pub dual: bool,
    pub paged: Option<usize>,
}

/// One laid-out row: a page or a spread.
#[derive(Clone)]
struct Row {
    top: f32,
    height: f32,
    pages: Range<usize>,
}

/// Rows of pages (single pages or spreads) stacked top to bottom at a given
/// scale, each centred horizontally.
pub struct Layout {
    page_sizes: Vec<Size>,
    mode: LayoutMode,
    scale: f32,
    content_width: f32,
    content_height: f32,
    rects: Vec<Option<PageRect>>,
    rows: Vec<Row>,
}

impl Layout {
    pub fn new(page_sizes: Vec<Size>) -> Self {
        let mut layout = Self {
            page_sizes,
            mode: LayoutMode::default(),
            scale: 0.0,
            content_width: 0.0,
            content_height: 0.0,
            rects: Vec::new(),
            rows: Vec::new(),
        };
        layout.arrange(0.0, 0.0, 0.0);
        layout
    }

    /// Takes effect at the next [`Layout::arrange`].
    pub fn set_mode(&mut self, mode: LayoutMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> LayoutMode {
        self.mode
    }

    /// Lays the shown rows out at `scale` UI points per PDF point. The content
    /// is at least `available_width` × `available_height`; rows that are
    /// narrower or shorter are centred.
    pub fn arrange(&mut self, scale: f32, available_width: f32, available_height: f32) {
        self.scale = scale;
        let shown = match self.mode.paged {
            Some(spread) => spread.min(self.spread_count())..(spread + 1).min(self.spread_count()),
            None => 0..self.spread_count(),
        };
        let row_widths: Vec<(usize, f32)> = shown
            .map(|spread| {
                let width = self.spread_size(spread).map_or(0.0, |s| s.width);
                (spread, width * scale + self.spread_gap(spread))
            })
            .collect();
        let widest_row = row_widths.iter().map(|&(_, w)| w).fold(0.0, f32::max);
        self.content_width = available_width.max(widest_row + 2.0 * MARGIN);

        self.rects = vec![None; self.page_sizes.len()];
        self.rows.clear();
        let rows_height: f32 = row_widths
            .iter()
            .map(|&(spread, _)| self.spread_size(spread).map_or(0.0, |s| s.height) * scale)
            .sum::<f32>()
            + PAGE_GAP * row_widths.len().saturating_sub(1) as f32;
        let mut y = MARGIN + ((available_height - rows_height - 2.0 * MARGIN) / 2.0).max(0.0);
        for (spread, row_width) in row_widths {
            let pages = self.spread_pages(spread);
            let height = self.spread_size(spread).map_or(0.0, |s| s.height) * scale;
            let mut x = (self.content_width - row_width) / 2.0;
            for page in pages.clone() {
                let size = self.page_sizes[page];
                let (w, h) = (size.width * scale, size.height * scale);
                self.rects[page] = Some(PageRect {
                    x,
                    y: y + (height - h) / 2.0,
                    width: w,
                    height: h,
                });
                x += w + self.spread_gap(spread);
            }
            self.rows.push(Row {
                top: y,
                height,
                pages,
            });
            y += height + PAGE_GAP;
        }
        self.content_height = match self.rows.last() {
            Some(last) => available_height.max(last.top + last.height + MARGIN),
            None => 0.0,
        };
    }

    /// UI points per PDF point, as last arranged.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes.len()
    }

    pub fn page_sizes(&self) -> &[Size] {
        &self.page_sizes
    }

    pub fn page_size(&self, page: usize) -> Option<Size> {
        self.page_sizes.get(page).copied()
    }

    pub fn spread_count(&self) -> usize {
        if self.mode.dual {
            self.page_sizes.len().div_ceil(2)
        } else {
            self.page_sizes.len()
        }
    }

    pub fn spread_of(&self, page: usize) -> usize {
        if self.mode.dual { page / 2 } else { page }
    }

    pub fn spread_pages(&self, spread: usize) -> Range<usize> {
        let per = if self.mode.dual { 2 } else { 1 };
        let n = self.page_sizes.len();
        (spread * per).min(n)..(spread * per + per).min(n)
    }

    /// The spread's size in PDF points without the gap between its pages. An
    /// odd last page in dual mode counts as a full spread of that page's width.
    pub fn spread_size(&self, spread: usize) -> Option<Size> {
        let pages = self.spread_pages(spread);
        if pages.is_empty() {
            return None;
        }
        let sizes = &self.page_sizes[pages.clone()];
        let mut width: f32 = sizes.iter().map(|s| s.width).sum();
        if self.mode.dual && pages.len() == 1 {
            width *= 2.0;
        }
        let height = sizes.iter().map(|s| s.height).fold(0.0, f32::max);
        Some(Size { width, height })
    }

    /// The gap between the pages of a spread, in UI points.
    pub fn spread_gap(&self, _spread: usize) -> f32 {
        if self.mode.dual { PAGE_GAP } else { 0.0 }
    }

    /// Width of the widest spread in PDF points, gap excluded.
    pub fn widest_spread(&self) -> f32 {
        (0..self.spread_count())
            .filter_map(|s| self.spread_size(s))
            .map(|s| s.width)
            .fold(0.0, f32::max)
    }

    /// `None` for pages not shown (outside the current spread in paged mode).
    pub fn page_rect(&self, page: usize) -> Option<PageRect> {
        self.rects.get(page).copied().flatten()
    }

    pub fn content_width(&self) -> f32 {
        self.content_width
    }

    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// Shown pages in rows intersecting the vertical span `[top, bottom)`.
    pub fn visible_pages(&self, top: f32, bottom: f32) -> Range<usize> {
        let start = self.rows.partition_point(|r| r.top + r.height <= top);
        let end = self.rows.partition_point(|r| r.top < bottom).max(start);
        match (self.rows.get(start), end > start) {
            (Some(first), true) => first.pages.start..self.rows[end - 1].pages.end,
            (Some(first), false) => first.pages.start..first.pages.start,
            (None, _) => {
                let after = self.rows.last().map_or(0, |r| r.pages.end);
                after..after
            }
        }
    }

    /// The document point at content point (`x`, `y`).
    pub fn doc_point(&self, x: f32, y: f32) -> Option<DocPoint> {
        if self.scale <= 0.0 || self.rows.is_empty() {
            return None;
        }
        let row = &self.rows[self.rows.partition_point(|r| r.top <= y).saturating_sub(1)];
        let page = row
            .pages
            .clone()
            .rev()
            .find(|&p| self.page_rect(p).is_some_and(|r| r.x <= x))
            .unwrap_or(row.pages.start);
        let rect = self.page_rect(page)?;
        Some(DocPoint {
            page,
            x: (x - rect.x) / self.scale,
            y: (y - rect.y) / self.scale,
        })
    }

    /// The content point of document point `p`, if its page is shown.
    pub fn content_point(&self, p: DocPoint) -> Option<(f32, f32)> {
        let rect = self.page_rect(p.page)?;
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
        layout.arrange(1.0, 636.0, 0.0);
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
        layout.arrange(1.0, 500.0, 0.0);
        assert_eq!(layout.content_height(), 0.0);
        assert_eq!(layout.page_rect(0), None);
        assert!(layout.visible_pages(0.0, 1000.0).is_empty());
        assert_eq!(layout.doc_point(10.0, 10.0), None);
    }

    #[test]
    fn zero_width_page_is_safe() {
        let mut layout = Layout::new(vec![size(0.0, 100.0)]);
        layout.arrange(1.0, 500.0, 0.0);
        assert!(layout.page_rect(0).is_some());
        assert_eq!(layout.widest_spread(), 0.0);
    }

    #[test]
    fn wide_pages_extend_content() {
        let mut layout = Layout::new(vec![size(612.0, 792.0)]);
        layout.arrange(2.0, 500.0, 0.0);
        assert_eq!(layout.content_width(), 1248.0);
        assert_eq!(layout.page_rect(0).unwrap().x, 12.0);
    }

    #[test]
    fn narrow_pages_centre_in_window() {
        let mut layout = Layout::new(vec![size(612.0, 792.0)]);
        layout.arrange(0.5, 1000.0, 0.0);
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

    fn letters(count: usize) -> Vec<Size> {
        vec![size(612.0, 792.0); count]
    }

    fn arranged(pages: Vec<Size>, mode: LayoutMode, width: f32) -> Layout {
        let mut layout = Layout::new(pages);
        layout.set_mode(mode);
        layout.arrange(1.0, width, 0.0);
        layout
    }

    const DUAL: LayoutMode = LayoutMode {
        dual: true,
        paged: None,
    };

    #[test]
    fn dual_rows_side_by_side() {
        let layout = arranged(letters(4), DUAL, 1260.0);
        assert_eq!(layout.page_rect(0).unwrap().x, 12.0);
        assert_eq!(layout.page_rect(1).unwrap().x, 12.0 + 612.0 + 12.0);
        assert_eq!(layout.page_rect(1).unwrap().y, 12.0);
        assert_eq!(layout.page_rect(2).unwrap().y, 12.0 + 792.0 + 12.0);
        assert_eq!(layout.content_width(), 1260.0);
    }

    #[test]
    fn odd_last_page_takes_left_slot() {
        let layout = arranged(letters(3), DUAL, 1260.0);
        assert_eq!(
            layout.page_rect(2).unwrap().x,
            layout.page_rect(0).unwrap().x
        );
        assert_eq!(layout.spread_count(), 2);
        assert_eq!(layout.spread_pages(1), 2..3);
    }

    #[test]
    fn spread_of_mixed_sizes() {
        let layout = arranged(vec![size(612.0, 792.0), size(306.0, 396.0)], DUAL, 1260.0);
        assert_eq!(layout.spread_size(0), Some(size(918.0, 792.0)));
        assert_eq!(layout.page_rect(1).unwrap().y, 12.0 + (792.0 - 396.0) / 2.0);
    }

    #[test]
    fn paged_shows_one_spread() {
        let paged = LayoutMode {
            dual: false,
            paged: Some(2),
        };
        let layout = arranged(letters(4), paged, 636.0);
        assert_eq!(layout.page_rect(1), None);
        assert_eq!(layout.page_rect(2).unwrap().y, 12.0);
        assert_eq!(layout.page_rect(3), None);
        assert_eq!(layout.content_height(), 12.0 + 792.0 + 12.0);
        assert_eq!(layout.visible_pages(0.0, 1e6), 2..3);
    }

    #[test]
    fn doc_point_picks_page_in_row() {
        let layout = arranged(letters(2), DUAL, 1260.0);
        let p = layout.doc_point(12.0 + 612.0 + 12.0 + 100.0, 50.0).unwrap();
        assert_eq!(p.page, 1);
        assert!((p.x - 100.0).abs() < 1e-3);
    }

    #[test]
    fn spread_mapping() {
        let dual = arranged(letters(4), DUAL, 1260.0);
        assert_eq!(dual.spread_of(3), 1);
        let single = arranged(letters(4), LayoutMode::default(), 636.0);
        assert_eq!(single.spread_of(3), 3);
        assert_eq!(Layout::new(vec![]).spread_count(), 0);
    }

    #[test]
    fn short_content_is_centred_vertically() {
        let paged = LayoutMode {
            dual: true,
            paged: Some(0),
        };
        let mut layout = Layout::new(letters(2));
        layout.set_mode(paged);
        layout.arrange(1.0, 1260.0, 1000.0);
        // One 792-high row plus margins is 816; the remaining 184 is split.
        assert_eq!(layout.page_rect(0).unwrap().y, 12.0 + 92.0);
        assert_eq!(layout.content_height(), 1000.0);
    }

    #[test]
    fn tall_content_is_not_shifted() {
        let mut layout = Layout::new(letters(3));
        layout.arrange(1.0, 636.0, 1000.0);
        assert_eq!(layout.page_rect(0).unwrap().y, 12.0);
        assert_eq!(
            layout.content_height(),
            12.0 + 3.0 * 792.0 + 2.0 * 12.0 + 12.0
        );
    }

    #[test]
    fn render_scale_caps_longest_side() {
        let s = render_scale(size(100.0, 20_000.0), 2.0, 8192);
        assert!(20_000.0 * s <= 8192.0 && 20_000.0 * s > 8191.0, "scale {s}");
        assert_eq!(render_scale(size(612.0, 792.0), 2.0, 8192), 2.0);
    }
}
