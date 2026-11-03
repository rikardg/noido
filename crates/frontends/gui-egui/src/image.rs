//! Rendered pages on the GPU.

use std::collections::HashMap;

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

/// How long a page's render takes to fade in over its stand-in.
pub(crate) const FADE_SECONDS: f64 = 0.15;

/// Pages whose render is fading in over their stand-in: when each began.
#[derive(Default)]
pub(crate) struct Fades {
    started: HashMap<usize, f64>,
}

impl Fades {
    /// `page`'s render arrived at `now`.
    pub(crate) fn start(&mut self, page: usize, now: f64) {
        self.started.insert(page, now);
    }

    /// How far `page`'s fade has come at `now`, from 0 to 1; `None` once
    /// done, or if it never began.
    pub(crate) fn alpha(&self, page: usize, now: f64) -> Option<f32> {
        let t = (now - self.started.get(&page)?) / FADE_SECONDS;
        (t < 1.0).then_some(t.max(0.0) as f32)
    }

    /// Drops the fades done by `now`.
    pub(crate) fn forget_finished(&mut self, now: f64) {
        self.started.retain(|_, start| now - *start < FADE_SECONDS);
    }
}

/// Paints a page over `rect` with `tint`: its `sharp` image, fading in over
/// its `stand_in` while `fade` (see [`Fades::alpha`]) runs; else the stand-in;
/// else blank paper. Returns whether it is fading.
pub(crate) fn paint_page(
    painter: &egui::Painter,
    rect: Rect,
    sharp: Option<&PageImage>,
    stand_in: Option<&PageImage>,
    fade: Option<f32>,
    tint: Color32,
) -> bool {
    let draw = |image: &PageImage, tint| painter.image(image.texture.id(), rect, image.uv, tint);
    match (sharp, stand_in, fade) {
        (Some(sharp), Some(stand_in), Some(alpha)) => {
            draw(stand_in, tint);
            draw(sharp, tint.gamma_multiply(alpha));
            return true;
        }
        (Some(image), _, _) | (None, Some(image), _) => {
            draw(image, tint);
        }
        (None, None, _) => {
            painter.rect_filled(rect, 0.0, tint);
        }
    }
    false
}

/// The blur of [`soften`] in each direction: away from the edges, two box
/// passes of radius 1.
const SOFTEN_WEIGHTS: [u32; 5] = [1, 2, 3, 2, 1];
/// The sum of the weights in both directions.
const SOFTEN_TOTAL: u32 = 81;

/// `image` blurred, so a thumbnail stretched over a page reads as a soft
/// preview rather than a blocky one. Edge pixels repeat outwards;
/// premultiplied pixels average as they are.
///
/// Written for the compiler to vectorise: a horizontal pass over padded
/// rows, then a vertical one over whole rows, rounded once at the end.
pub(crate) fn soften(image: &Rgba8Image) -> Rgba8Image {
    let (width, height) = (image.width() as usize, image.height() as usize);
    if width == 0 || height == 0 {
        return image.clone();
    }
    let stride = width * 4;
    let [w0, w1, w2, w3, w4] = SOFTEN_WEIGHTS.map(|w| w as u16);
    // A row with its edge pixel repeated twice on each side.
    let mut padded = vec![0u16; stride + 16];
    let mut across = vec![0u16; stride * height];
    for (row, out) in image
        .pixels()
        .chunks_exact(stride)
        .zip(across.chunks_exact_mut(stride))
    {
        for (p, &v) in padded[8..8 + stride].iter_mut().zip(row) {
            *p = v.into();
        }
        for c in 0..4 {
            let (first, last) = (row[c].into(), row[stride - 4 + c].into());
            padded[c] = first;
            padded[4 + c] = first;
            padded[8 + stride + c] = last;
            padded[12 + stride + c] = last;
        }
        for (i, o) in out.iter_mut().enumerate() {
            *o = w0 * padded[i]
                + w1 * padded[i + 4]
                + w2 * padded[i + 8]
                + w3 * padded[i + 12]
                + w4 * padded[i + 16];
        }
    }
    let row = |y: usize, k: usize| {
        let y = (y + k).saturating_sub(2).min(height - 1);
        &across[y * stride..(y + 1) * stride]
    };
    let [v0, v1, v2, v3, v4] = SOFTEN_WEIGHTS;
    let mut pixels = vec![0u8; stride * height];
    for (y, out) in pixels.chunks_exact_mut(stride).enumerate() {
        let (a, b, c, d, e) = (row(y, 0), row(y, 1), row(y, 2), row(y, 3), row(y, 4));
        for i in 0..stride {
            let sum = v0 * u32::from(a[i])
                + v1 * u32::from(b[i])
                + v2 * u32::from(c[i])
                + v3 * u32::from(d[i])
                + v4 * u32::from(e[i]);
            out[i] = ((sum + SOFTEN_TOTAL / 2) / SOFTEN_TOTAL) as u8;
        }
    }
    Rgba8Image::from_premultiplied(image.width(), image.height(), pixels).expect("same size")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, pixels: Vec<u8>) -> Rgba8Image {
        Rgba8Image::from_premultiplied(width, height, pixels).unwrap()
    }

    /// The 1-2-3-2-1 kernel applied directly in 2D, edges clamped,
    /// unrounded.
    fn reference(image: &Rgba8Image) -> Vec<f32> {
        let (w, h) = (image.width() as i32, image.height() as i32);
        let weights = [1.0, 2.0, 3.0, 2.0, 1.0];
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                for c in 0..4 {
                    let mut sum = 0.0;
                    for (j, wy) in weights.iter().enumerate() {
                        for (i, wx) in weights.iter().enumerate() {
                            let sx = (x + i as i32 - 2).clamp(0, w - 1) as u32;
                            let sy = (y + j as i32 - 2).clamp(0, h - 1) as u32;
                            sum += wx * wy * image.pixel(sx, sy)[c] as f32;
                        }
                    }
                    out.push(sum / 81.0);
                }
            }
        }
        out
    }

    #[test]
    fn fade_rises_and_ends() {
        let mut fades = Fades::default();
        assert_eq!(fades.alpha(2, 10.0), None);
        fades.start(2, 10.0);
        assert_eq!(fades.alpha(2, 10.0), Some(0.0));
        let half = fades.alpha(2, 10.0 + FADE_SECONDS / 2.0).unwrap();
        assert!((half - 0.5).abs() < 1e-6, "{half}");
        assert_eq!(fades.alpha(2, 10.0 + FADE_SECONDS), None);
        assert_eq!(fades.alpha(3, 10.0), None);
    }

    #[test]
    fn finished_fades_are_forgotten() {
        let mut fades = Fades::default();
        fades.start(1, 0.0);
        fades.start(2, 1.0);
        fades.forget_finished(1.0);
        assert_eq!(fades.started.keys().copied().collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn soften_leaves_a_uniform_image() {
        let flat = image(4, 3, [10, 20, 30, 40].repeat(12));
        assert_eq!(soften(&flat).pixels(), flat.pixels());
    }

    #[test]
    fn soften_spreads_a_pixel_by_one_two_three_two_one() {
        // A bright pixel in the middle of 7 x 7 black, far from the edges.
        let mut pixels = vec![0; 7 * 7 * 4];
        let centre = (3 * 7 + 3) * 4;
        pixels[centre..centre + 4].copy_from_slice(&[243, 243, 243, 243]);
        let soft = soften(&image(7, 7, pixels));
        // Weights (1 2 3 2 1) x (1 2 3 2 1) / 81, times 243.
        let weights = [1, 2, 3, 2, 1];
        for y in 0..7 {
            for x in 0..7 {
                let expected = if (1..6).contains(&x) && (1..6).contains(&y) {
                    weights[x as usize - 1] * weights[y as usize - 1] * 3
                } else {
                    0
                };
                assert_eq!(soft.pixel(x, y), [expected as u8; 4], "({x}, {y})");
            }
        }
    }

    #[test]
    fn soften_repeats_the_edges() {
        // A bright left column: the edge stays brightest, nothing wraps.
        let mut pixels = vec![0; 5 * 2 * 4];
        for y in 0..2 {
            pixels[y * 5 * 4..y * 5 * 4 + 4].copy_from_slice(&[81; 4]);
        }
        let soft = soften(&image(5, 2, pixels));
        // Column 0 sees itself three times over (1 + 2 + 3) / 9.
        let row: Vec<u8> = (0..5).map(|x| soft.pixel(x, 0)[0]).collect();
        assert_eq!(row, [54, 27, 9, 0, 0]);
        assert_eq!(soft.pixel(0, 1), soft.pixel(0, 0));
    }

    #[test]
    fn soften_matches_the_kernel_applied_directly() {
        let (w, h) = (9, 6);
        let pixels = (0..w * h * 4).map(|i| (i * 37 % 251) as u8).collect();
        let original = image(w, h, pixels);
        let soft = soften(&original);
        for (i, (&got, want)) in soft.pixels().iter().zip(reference(&original)).enumerate() {
            assert!(
                (got as f32 - want).abs() <= 0.5 + 1e-3,
                "byte {i}: {got} vs {want}"
            );
        }
    }

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
