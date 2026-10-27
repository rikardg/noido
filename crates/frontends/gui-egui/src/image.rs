//! Rendered pages on the GPU.

use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use noido_core::Paper;
use noido_engine::{Rgba8Image, Size};

/// A rendered page on the GPU, full resolution or thumbnail.
#[derive(Clone)]
pub(crate) struct PageImage {
    pub(crate) texture: TextureHandle,
    /// The part of the texture the page covers (see [`page_uv`]).
    pub(crate) uv: Rect,
}

impl PageImage {
    /// Uploads `image`, a render of a page of `size` at `scale`.
    pub(crate) fn load(
        ctx: &egui::Context,
        name: String,
        image: &Rgba8Image,
        size: Option<Size>,
        scale: f32,
    ) -> Self {
        let pixels = [image.width() as usize, image.height() as usize];
        let color_image = ColorImage::from_rgba_premultiplied(pixels, image.pixels());
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        Self {
            texture: ctx.load_texture(name, color_image, TextureOptions::LINEAR),
            uv: size.map_or(full, |size| page_uv(size, scale, image)),
        }
    }
}

/// The colour of `paper`: pages and their placeholders are drawn with it as
/// the tint, which colours white paper and leaves black text black.
pub(crate) fn paper_colour(paper: Paper) -> Color32 {
    let [r, g, b] = paper.rgb();
    Color32::from_rgb(r, g, b)
}

/// The part of `image` that the page covers. Renders are rounded up to whole
/// pixels, so the last row and column are partly padding; stretching them
/// onto the page would squeeze the content towards the top-left, by up to a
/// pixel of the image (several screen pixels for a thumbnail).
pub(crate) fn page_uv(size: Size, scale: f32, image: &Rgba8Image) -> Rect {
    let covered = |length: f32, pixels: u32| (length * scale / pixels as f32).min(1.0);
    Rect::from_min_max(
        pos2(0.0, 0.0),
        pos2(
            covered(size.width, image.width()),
            covered(size.height, image.height()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_uv_covers_only_the_page() {
        // Letter 120 px wide is 155.3 px tall, rendered into 156 rows; the
        // last 0.7 row is padding and must not be stretched onto the page.
        let letter = Size {
            width: 612.0,
            height: 792.0,
        };
        let scale = 120.0 / 612.0;
        let image = Rgba8Image::from_premultiplied(120, 156, vec![255; 120 * 156 * 4]).unwrap();
        let uv = page_uv(letter, scale, &image);
        assert_eq!(uv.min, pos2(0.0, 0.0));
        assert_eq!(uv.max.x, 1.0);
        assert!((uv.max.y - 792.0 * scale / 156.0).abs() < 1e-6, "{uv:?}");
    }
}
