//! Low-res thumbnails: a large window of pages around the current one.

use noido_engine::Size;

use crate::render_scale;

/// Pages around the current one that get thumbnails.
pub const THUMBNAIL_BUDGET: usize = 100;
/// Thumbnail width in pixels; the height follows the page.
pub const THUMBNAIL_WIDTH: f32 = 120.0;
/// Longest thumbnail side in pixels, for very tall, narrow pages.
pub const THUMBNAIL_MAX_SIDE: u32 = 1024;

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
