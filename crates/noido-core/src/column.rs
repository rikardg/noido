//! The sidebar's column of page thumbnails: one per page, all as wide as the
//! sidebar allows, each with its page number underneath.

use std::ops::Range;

use noido_engine::Size;

use crate::PageRect;

/// Space around the thumbnails and above the first, in UI points.
pub const COLUMN_MARGIN: f32 = 12.0;
/// Height of the page number row under each thumbnail, in UI points.
pub const LABEL_HEIGHT: f32 = 26.0;
/// Space between a page number and the next thumbnail, in UI points.
pub const COLUMN_GAP: f32 = 12.0;

/// Thumbnails laid out top to bottom for a sidebar of a given inner width.
pub struct Column {
    thumbs: Vec<PageRect>,
}

impl Column {
    pub fn new(page_sizes: &[Size], width: f32) -> Self {
        let thumb_width = (width - 2.0 * COLUMN_MARGIN).max(1.0);
        let mut y = COLUMN_MARGIN;
        let thumbs = page_sizes
            .iter()
            .map(|size| {
                let height = if size.width.is_finite() && size.width > 0.0 {
                    thumb_width * size.height / size.width
                } else {
                    thumb_width
                };
                let height = if height.is_finite() && height > 0.0 {
                    height
                } else {
                    thumb_width
                };
                let rect = PageRect {
                    x: COLUMN_MARGIN,
                    y,
                    width: thumb_width,
                    height,
                };
                y += height + LABEL_HEIGHT + COLUMN_GAP;
                rect
            })
            .collect();
        Self { thumbs }
    }

    pub fn thumb_width(&self) -> f32 {
        self.thumbs.first().map_or(1.0, |t| t.width)
    }

    pub fn thumb(&self, page: usize) -> Option<PageRect> {
        self.thumbs.get(page).copied()
    }

    /// The page number's row under the thumbnail.
    pub fn label(&self, page: usize) -> Option<PageRect> {
        self.thumb(page).map(|t| PageRect {
            y: t.y + t.height,
            height: LABEL_HEIGHT,
            ..t
        })
    }

    pub fn content_height(&self) -> f32 {
        self.thumbs
            .last()
            .map_or(0.0, |t| t.y + t.height + LABEL_HEIGHT + COLUMN_MARGIN)
    }

    /// Pages whose thumbnail or number overlaps the span `[top, bottom)`.
    pub fn visible(&self, top: f32, bottom: f32) -> Range<usize> {
        let start = self
            .thumbs
            .partition_point(|t| t.y + t.height + LABEL_HEIGHT <= top);
        let end = self.thumbs.partition_point(|t| t.y < bottom).max(start);
        start..end
    }

    /// The page whose slot, from its thumbnail's top to the next one's,
    /// holds `y`; the first or last page outside them.
    pub fn page_at(&self, y: f32) -> Option<usize> {
        if self.thumbs.is_empty() {
            return None;
        }
        Some(self.thumbs.partition_point(|t| t.y <= y).saturating_sub(1))
    }

    /// The scroll offset that shows `page` with its number after scrolling
    /// as little as possible from `offset` (centred on the thumbnail if it
    /// doesn't fit); `None` when it's fully in view already.
    pub fn reveal(&self, page: usize, offset: f32, view_height: f32) -> Option<f32> {
        let thumb = self.thumb(page)?;
        let top = thumb.y;
        let bottom = thumb.y + thumb.height + LABEL_HEIGHT;
        if top >= offset && bottom <= offset + view_height {
            return None;
        }
        let wanted = if bottom - top + 2.0 * COLUMN_MARGIN > view_height {
            thumb.y + thumb.height / 2.0 - view_height / 2.0
        } else if top < offset {
            top - COLUMN_MARGIN
        } else {
            bottom + COLUMN_MARGIN - view_height
        };
        let max = (self.content_height() - view_height).max(0.0);
        Some(wanted.clamp(0.0, max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter() -> Size {
        Size {
            width: 612.0,
            height: 792.0,
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn thumbs_fill_width_and_stack() {
        let column = Column::new(&[letter(); 3], 260.0);
        assert_eq!(column.thumb_width(), 236.0);
        let h = 236.0 * 792.0 / 612.0;
        let first = column.thumb(0).unwrap();
        assert_eq!((first.x, first.y, first.width), (12.0, 12.0, 236.0));
        assert!(near(first.height, h));
        let label = column.label(0).unwrap();
        assert!(near(label.y, 12.0 + h));
        assert_eq!(
            (label.x, label.width, label.height),
            (12.0, 236.0, LABEL_HEIGHT)
        );
        assert!(near(
            column.thumb(1).unwrap().y,
            12.0 + h + LABEL_HEIGHT + COLUMN_GAP
        ));
        assert!(near(
            column.content_height(),
            3.0 * (h + LABEL_HEIGHT + COLUMN_GAP) - 12.0 + 24.0
        ));
        assert!(column.thumb(3).is_none());
    }

    #[test]
    fn mixed_page_sizes_share_width() {
        let landscape = Size {
            width: 842.0,
            height: 595.0,
        };
        let column = Column::new(&[letter(), landscape], 260.0);
        let (a, b) = (column.thumb(0).unwrap(), column.thumb(1).unwrap());
        assert_eq!(a.width, b.width);
        assert!(near(b.height, 236.0 * 595.0 / 842.0));
    }

    #[test]
    fn narrow_width_is_at_least_one_point() {
        let column = Column::new(&[letter()], 10.0);
        assert_eq!(column.thumb_width(), 1.0);
    }

    #[test]
    fn page_without_width_is_square() {
        let odd = Size {
            width: 0.0,
            height: 100.0,
        };
        let column = Column::new(&[odd], 260.0);
        assert_eq!(column.thumb(0).unwrap().height, 236.0);
    }

    #[test]
    fn visible_pages_overlap_range() {
        let column = Column::new(&[letter(); 10], 260.0);
        let second = column.thumb(1).unwrap();
        let fourth = column.thumb(3).unwrap();
        assert_eq!(column.visible(second.y + 1.0, fourth.y + 1.0), 1..4);
        assert_eq!(column.visible(0.0, 1.0), 0..0);
        assert_eq!(column.visible(0.0, 13.0), 0..1);
        assert_eq!(column.visible(1e6, 2e6), 10..10);
    }

    #[test]
    fn visible_includes_label_only_overlap() {
        let column = Column::new(&[letter(); 10], 260.0);
        let label = column.label(0).unwrap();
        let top = label.y + 1.0;
        assert_eq!(column.visible(top, top + 5.0), 0..1);
    }

    #[test]
    fn page_at_clamps_and_finds_slot() {
        let column = Column::new(&[letter(); 10], 260.0);
        let label = column.label(0).unwrap();
        assert_eq!(column.page_at(label.y + label.height + 5.0), Some(0));
        assert_eq!(column.page_at(column.thumb(1).unwrap().y), Some(1));
        assert_eq!(column.page_at(1e6), Some(9));
        assert_eq!(column.page_at(-5.0), Some(0));
    }

    #[test]
    fn reveal_none_when_visible() {
        let column = Column::new(&[letter(); 10], 260.0);
        assert_eq!(column.reveal(0, 0.0, 800.0), None);
    }

    #[test]
    fn reveal_scrolls_down_minimally() {
        let column = Column::new(&[letter(); 10], 260.0);
        let label = column.label(4).unwrap();
        let offset = column.reveal(4, 0.0, 800.0).unwrap();
        assert!(near(offset + 800.0, label.y + label.height + COLUMN_MARGIN));
    }

    #[test]
    fn reveal_scrolls_up_minimally() {
        let column = Column::new(&[letter(); 10], 260.0);
        let thumb = column.thumb(2).unwrap();
        let offset = column.reveal(2, thumb.y + 100.0, 800.0).unwrap();
        assert!(near(offset, thumb.y - COLUMN_MARGIN));
    }

    #[test]
    fn reveal_centres_when_too_tall() {
        let column = Column::new(&[letter(); 10], 260.0);
        let thumb = column.thumb(5).unwrap();
        let offset = column.reveal(5, 0.0, 100.0).unwrap();
        assert!(near(offset + 50.0, thumb.y + thumb.height / 2.0));
    }

    #[test]
    fn reveal_clamps_to_content() {
        let column = Column::new(&[letter(); 10], 260.0);
        let offset = column.reveal(9, 0.0, 800.0).unwrap();
        assert!(near(offset, column.content_height() - 800.0));
        // Everything fits: never scrolls.
        let short = Column::new(&[letter()], 260.0);
        assert_eq!(short.reveal(0, 50.0, 2000.0), Some(0.0));
    }

    #[test]
    fn empty_column_is_safe() {
        let column = Column::new(&[], 260.0);
        assert_eq!(column.content_height(), 0.0);
        assert_eq!(column.visible(0.0, 100.0), 0..0);
        assert_eq!(column.page_at(10.0), None);
        assert_eq!(column.reveal(0, 0.0, 100.0), None);
    }
}
