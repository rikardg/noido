//! Drawing a typeset page: glyph outlines from skrifa, filled by tiny-skia.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use noido_engine::{EngineError, PixelRect, Rgba8Image};
use noido_image::MAX_PIXELS;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tiny_skia::{
    FillRule, Paint, Path, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Rect, Transform,
};

use crate::book::Book;
use crate::fonts::{FaceId, fonts};
use crate::house::LEFT;
use crate::images::ImageId;
use crate::lines::{Decoration, SetGlyph, SetLine};

/// The text colour, #1b1b1b.
const INK: u8 = 0x1b;
/// A decoration's line is this thick in points, and at least a pixel.
const HAIRLINE: f32 = 0.5;
/// An underline sits this far below the baseline, a strike-through this
/// far above it, in ems of the line's text.
const UNDERLINE: f32 = 0.1;
const STRIKE: f32 = 0.3;
/// The grey of the frame drawn for an image that cannot be, #999999.
const FRAME: u8 = 0x99;
/// The pixels drawn beyond a tile on every side, and cut off.
const PAD: u32 = 2;

/// A glyph placed on a page: its baseline origin in points.
struct Ink {
    face: FaceId,
    id: u16,
    size: f32,
    x: f32,
    y: f32,
}

/// An underline or strike-through: from `x0` to `x1`, hung from the
/// baseline `y`, for text of `em` points.
struct Bar {
    kind: Decoration,
    x0: f32,
    x1: f32,
    y: f32,
    em: f32,
}

/// An image's rectangle on the page, in points.
pub(crate) struct ImageInk {
    pub(crate) id: ImageId,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// What a page shows: the images, the glyphs and the lines over them.
#[derive(Default)]
pub(crate) struct PageInk {
    pub(crate) images: Vec<ImageInk>,
    glyphs: Vec<Ink>,
    bars: Vec<Bar>,
}

impl PageInk {
    /// A line whose top is at `top`; positions as the glyph quads of
    /// `Book::glyphs` have them.
    pub(crate) fn add_line(&mut self, line: &SetLine, top: f32) {
        let baseline = top + line.ascent;
        // An image is not a glyph; it is drawn from its own rectangle.
        for g in &line.glyphs {
            if let Some((id, height)) = g.image {
                self.images.push(ImageInk {
                    id,
                    x: LEFT + g.x,
                    y: baseline - g.rise - height,
                    width: g.advance,
                    height,
                });
            }
        }
        for g in line.glyphs.iter().filter(|g| g.image.is_none()) {
            self.glyphs.push(Ink {
                face: g.face,
                id: g.id,
                size: g.size,
                x: LEFT + g.x,
                y: baseline - g.rise,
            });
        }
        for &(kind, x0, x1) in &line.decorations {
            let under = |g: &&SetGlyph| g.x < x1 && g.x + g.advance > x0;
            let biggest = |glyphs: &mut dyn Iterator<Item = &SetGlyph>| {
                glyphs.map(|g| g.size).fold(0.0, f32::max)
            };
            let mut em = biggest(&mut line.glyphs.iter().filter(under));
            if em == 0.0 {
                // Nothing under it: the line's own size.
                em = biggest(&mut line.glyphs.iter());
            }
            self.bars.push(Bar {
                kind,
                x0: LEFT + x0,
                x1: LEFT + x1,
                y: baseline,
                em,
            });
        }
    }
}

/// The outlines drawn so far, at one em with y down; `None` for a glyph
/// without an outline. Shared by the threads that draw.
#[derive(Default)]
pub(crate) struct Outlines(Mutex<HashMap<(u16, u16), Cached>>);

type Cached = Option<Arc<Path>>;

impl Outlines {
    fn get(&self, face: FaceId, id: u16) -> Option<Arc<Path>> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.entry((face.0, id))
            .or_insert_with(|| outline(face, id).map(Arc::new))
            .clone()
    }
}

struct Pen(PathBuilder);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, -y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, -y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(cx0, -cy0, x, -y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.cubic_to(cx0, -cy0, cx1, -cy1, x, -y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

fn outline(face: FaceId, id: u16) -> Option<Path> {
    let font = FontRef::new(fonts().data(face)).ok()?;
    let glyph = font.outline_glyphs().get(GlyphId::new(u32::from(id)))?;
    let mut pen = Pen(PathBuilder::new());
    let settings = DrawSettings::unhinted(Size::new(1.0), LocationRef::default());
    glyph.draw(settings, &mut pen).ok()?;
    pen.0.finish()
}

impl Book {
    /// `rect` of `page` at `scale` pixels per point, from the page's top
    /// left, on white.
    pub fn render_tile(
        &self,
        page: usize,
        scale: f32,
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        let ink = self.ink.get(page).ok_or(EngineError::PageOutOfRange {
            page,
            count: self.ink.len(),
        })?;
        let too_large = EngineError::TooLarge {
            width: rect.width,
            height: rect.height,
        };
        // The PDF engine's limit.
        let max = u32::from(u16::MAX);
        if rect.width == 0 || rect.height == 0 || rect.width > max || rect.height > max {
            return Err(too_large);
        }
        // Drawn on a pixmap a pixel larger all round: tiny-skia gives the
        // pixels on a clipped edge slightly different coverage, and a
        // tile must show what the page does.
        let (w, h) = (rect.width + 2 * PAD, rect.height + 2 * PAD);
        let mut pixmap = Pixmap::new(w, h).ok_or(too_large)?;
        pixmap.fill(tiny_skia::Color::WHITE);
        let mut paint = Paint::default();
        paint.set_color_rgba8(INK, INK, INK, 255);
        paint.anti_alias = true;
        let (dx, dy) = (rect.x as f32 - PAD as f32, rect.y as f32 - PAD as f32);
        let view = Rect::from_xywh(0.0, 0.0, w as f32, h as f32);

        let thick = (HAIRLINE * scale).max(1.0);
        for image in &ink.images {
            self.draw_image(&mut pixmap, image, scale, (dx, dy), thick);
        }

        for g in &ink.glyphs {
            let Some(path) = self.outlines.get(g.face, g.id) else {
                continue;
            };
            let k = g.size * scale;
            // Placed on the page first and moved into the tile after, so
            // that a tile's coordinates are the page's, less whole pixels.
            let on_page = Transform::from_row(k, 0.0, 0.0, k, g.x * scale, g.y * scale);
            let Some(placed) = Path::clone(&path)
                .transform(on_page)
                .and_then(|p| p.transform(Transform::from_translate(-dx, -dy)))
            else {
                continue;
            };
            // Skip what is wholly outside the tile.
            if view.is_none_or(|v| placed.bounds().intersect(&v).is_some()) {
                pixmap.fill_path(
                    &placed,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }

        for bar in &ink.bars {
            let y = match bar.kind {
                Decoration::Underline => bar.y + UNDERLINE * bar.em,
                Decoration::Strike => bar.y - STRIKE * bar.em,
            };
            let line = Rect::from_xywh(
                bar.x0 * scale - dx,
                y * scale - thick / 2.0 - dy,
                (bar.x1 - bar.x0) * scale,
                thick,
            );
            if let Some(line) = line {
                pixmap.fill_rect(line, &paint, Transform::identity(), None);
            }
        }

        let row = rect.width as usize * 4;
        let mut pixels = Vec::with_capacity(row * rect.height as usize);
        for y in PAD as usize..(PAD + rect.height) as usize {
            let from = (y * w as usize + PAD as usize) * 4;
            pixels.extend_from_slice(&pixmap.data()[from..from + row]);
        }
        Rgba8Image::from_premultiplied(rect.width, rect.height, pixels)
            .ok_or_else(|| EngineError::Render("the pixels do not fit the tile".into()))
    }
}

impl Book {
    /// `image`, at `scale`, on `pixmap` whose top left is the page's
    /// pixel `origin`: its pixels, or a frame when it has none.
    fn draw_image(
        &self,
        pixmap: &mut Pixmap,
        image: &ImageInk,
        scale: f32,
        origin: (f32, f32),
        thick: f32,
    ) {
        // Rounded on the page, so that a tile's image is the page's.
        let edge = |at: f32| (at * scale).round();
        let (x0, y0) = (edge(image.x), edge(image.y));
        let (x1, y1) = (edge(image.x + image.width), edge(image.y + image.height));
        let (w, h) = (x1 - x0, y1 - y0);
        if w < 1.0 || h < 1.0 {
            return;
        }
        let (left, top) = (x0 - origin.0, y0 - origin.1);
        if left >= pixmap.width() as f32
            || top >= pixmap.height() as f32
            || left + w <= 0.0
            || top + h <= 0.0
        {
            return;
        }
        let pixels = if (w as u64) * (h as u64) > MAX_PIXELS {
            None
        } else {
            self.images.pixels(image.id, w as u32, h as u32)
        };
        let drawn = pixels
            .as_ref()
            .and_then(|p| PixmapRef::from_bytes(p.as_raw(), p.width(), p.height()));
        if let Some(drawn) = drawn {
            pixmap.draw_pixmap(
                left as i32,
                top as i32,
                drawn,
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            return;
        }
        let mut paint = Paint::default();
        paint.set_color_rgba8(FRAME, FRAME, FRAME, 255);
        let t = thick.round();
        for (x, y, w, h) in [
            (left, top, w, t),
            (left, top + h - t, w, t),
            (left, top, t, h),
            (left + w - t, top, t, h),
        ] {
            if let Some(bar) = Rect::from_xywh(x, y, w, h) {
                pixmap.fill_rect(bar, &paint, Transform::identity(), None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::house::{LEFT, MEASURE, PAPER, TOP};
    use crate::typeset;

    const TEXT: &str = "Some words to set on a page, long enough to run over \
        several lines of the measure, so that there is ink to find. Some \
        words to set on a page, long enough to run over several lines of \
        the measure, so that there is ink to find.\n";

    fn whole(book: &Book, scale: f32) -> Rgba8Image {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: (PAPER.width * scale).ceil() as u32,
            height: (PAPER.height * scale).ceil() as u32,
        };
        book.render_tile(0, scale, rect).unwrap()
    }

    fn is_white(px: &[u8]) -> bool {
        px == [255, 255, 255, 255]
    }

    #[test]
    fn a_page_has_ink_inside_the_text_block_only() {
        let image = whole(&typeset(TEXT), 1.0);
        let (mut inside, mut outside) = (0, 0);
        for (i, px) in image.pixels().chunks(4).enumerate() {
            let (x, y) = (
                (i as u32 % image.width()) as f32,
                (i as u32 / image.width()) as f32,
            );
            if is_white(px) {
                continue;
            }
            let in_block = (LEFT.floor() - 1.0..LEFT + MEASURE + 1.0).contains(&x)
                && (TOP..TOP + 434.0).contains(&y);
            if in_block {
                inside += 1;
            } else {
                outside += 1;
            }
        }
        assert!(inside > 100, "ink inside: {inside}");
        assert_eq!(outside, 0);
    }

    #[test]
    fn drawing_twice_gives_the_same_pixels() {
        let book = typeset(TEXT);
        assert_eq!(whole(&book, 1.5).pixels(), whole(&book, 1.5).pixels());
    }

    #[test]
    fn a_tile_is_the_matching_part_of_the_page() {
        let book = typeset(TEXT);
        let page = whole(&book, 1.0);
        let rect = PixelRect {
            x: 100,
            y: 100,
            width: 50,
            height: 50,
        };
        let tile = book.render_tile(0, 1.0, rect).unwrap();
        assert_eq!((tile.width(), tile.height()), (50, 50));
        let mut crop = Vec::new();
        for row in 100..150 {
            let from = (row * page.width() as usize + 100) * 4;
            crop.extend_from_slice(&page.pixels()[from..from + 200]);
        }
        assert_eq!(tile.pixels(), crop.as_slice());
        assert!(crop.chunks(4).any(|p| !is_white(p)), "the tile has ink");
    }

    #[test]
    fn a_tile_beyond_the_limit_is_too_large() {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 70_000,
            height: 10,
        };
        let err = typeset(TEXT).render_tile(0, 1.0, rect).unwrap_err();
        assert!(matches!(err, EngineError::TooLarge { .. }), "{err:?}");
    }

    #[test]
    fn a_page_out_of_range_is_an_error() {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        let err = typeset(TEXT).render_tile(9, 1.0, rect).unwrap_err();
        assert!(matches!(err, EngineError::PageOutOfRange { .. }), "{err:?}");
    }

    #[test]
    fn a_link_is_underlined() {
        let plain = whole(&typeset("Some words here.\n"), 2.0);
        let linked = whole(&typeset("[Some words here.](#x)\n"), 2.0);
        let ink = |i: &Rgba8Image| i.pixels().chunks(4).filter(|p| !is_white(p)).count();
        assert!(ink(&linked) > ink(&plain));
    }

    /// The bounding box of the non-white pixels in the columns
    /// `cols` of `image`: (left, top, right, bottom) in pixels.
    fn ink_box(image: &Rgba8Image, cols: std::ops::Range<u32>) -> Option<(u32, u32, u32, u32)> {
        let mut found: Option<(u32, u32, u32, u32)> = None;
        for (i, px) in image.pixels().chunks(4).enumerate() {
            let (x, y) = (i as u32 % image.width(), i as u32 / image.width());
            if is_white(px) || !cols.contains(&x) {
                continue;
            }
            found = Some(match found {
                None => (x, y, x, y),
                Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
            });
        }
        found
    }

    #[test]
    fn ink_sits_under_the_glyph_quads() {
        let scale = 2.0;
        let book = typeset("Hxp\n");
        let image = whole(&book, scale);
        let glyphs = book.glyphs(0);
        let px = |v: f32| (v * scale) as i64;
        for g in &glyphs {
            let c = g.quad.corners;
            let (x0, x1) = (c[0].x.min(c[2].x), c[0].x.max(c[2].x));
            let cols = px(x0) as u32..px(x1) as u32 + 1;
            let (l, t, r, b) = ink_box(&image, cols).expect("ink");
            let (above, below) = (c[2].y.min(c[0].y), c[2].y.max(c[0].y));
            let (l, t, r, b) = (l as i64, t as i64, r as i64, b as i64);
            assert!(l >= px(x0) - 1 && r <= px(x1) + 1, "{} x {l}..{r}", g.text);
            assert!(
                t >= px(above) - 1 && b <= px(below) + 1,
                "{} y {t}..{b}",
                g.text
            );
            let baseline = px(g.origin.y);
            match g.text.as_str() {
                // The cap height reaches well above the baseline, and
                // nothing hangs below it.
                "H" => {
                    assert!(t <= baseline - px(0.6 * g.size), "H top {t} vs {baseline}");
                    assert!(b <= baseline + 1, "H bottom {b} vs {baseline}");
                }
                "p" => assert!(
                    b >= baseline + px(0.1 * g.size),
                    "p bottom {b} vs {baseline}"
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn a_tile_matches_the_page_at_a_fractional_scale() {
        let book = typeset(TEXT);
        let page = whole(&book, 1.5);
        let rect = PixelRect {
            x: 97,
            y: 103,
            width: 60,
            height: 60,
        };
        let tile = book.render_tile(0, 1.5, rect).unwrap();
        let mut crop = Vec::new();
        for row in 103..163 {
            let from = (row * page.width() as usize + 97) * 4;
            crop.extend_from_slice(&page.pixels()[from..from + 240]);
        }
        let diff: Vec<_> = tile
            .pixels()
            .iter()
            .zip(&crop)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i / 4 % 60, i / 240, *a, *b))
            .collect();
        assert!(
            diff.is_empty(),
            "{} differ: {:?}",
            diff.len(),
            &diff[..diff.len().min(8)]
        );
    }

    /// The rows where `decorated` differs from `plain`.
    fn changed_rows(plain: &Rgba8Image, decorated: &Rgba8Image) -> Vec<u32> {
        let w = plain.width() as usize * 4;
        let mut rows = Vec::new();
        for (y, (a, b)) in plain
            .pixels()
            .chunks(w)
            .zip(decorated.pixels().chunks(w))
            .enumerate()
        {
            if a != b {
                rows.push(y as u32);
            }
        }
        rows
    }

    #[test]
    fn a_strike_is_above_the_baseline_and_an_underline_below() {
        let scale = 2.0;
        let plain = typeset("Hxp Hxp\n");
        let baseline = (plain.glyphs(0)[0].origin.y * scale) as u32;
        let plain = whole(&plain, scale);
        let struck = changed_rows(&plain, &whole(&typeset("~~Hxp Hxp~~\n"), scale));
        let under = changed_rows(&plain, &whole(&typeset("[Hxp Hxp](#x)\n"), scale));
        assert!(
            !struck.is_empty() && struck.iter().all(|&y| y < baseline),
            "{struck:?}"
        );
        assert!(
            !under.is_empty() && under.iter().all(|&y| y > baseline),
            "{under:?}"
        );
    }

    // Images: `images.epub`, drawn.

    fn images_book() -> Book {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/images.epub");
        let bytes = std::fs::read(path).unwrap();
        let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
        crate::typeset_epub(&epub, noido_engine::PdfBytes::from(bytes))
    }

    /// The page and place of the first image `width` x `height` points.
    fn image_of(book: &Book, width: f32, height: f32) -> (usize, usize) {
        for (page, ink) in book.ink.iter().enumerate() {
            for (n, i) in ink.images.iter().enumerate() {
                if (i.width - width).abs() < 0.01 && (i.height - height).abs() < 0.01 {
                    return (page, n);
                }
            }
        }
        panic!("no image {width} x {height}");
    }

    fn pixel(image: &Rgba8Image, x: u32, y: u32) -> [u8; 4] {
        let at = (y * image.width() + x) as usize * 4;
        image.pixels()[at..at + 4].try_into().unwrap()
    }

    fn near_to(px: [u8; 4], want: [u8; 3]) {
        let ok = px[..3].iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 2) && px[3] == 255;
        assert!(ok, "{px:?} is not {want:?}");
    }

    fn page_at(book: &Book, page: usize, scale: f32) -> Rgba8Image {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: (PAPER.width * scale).ceil() as u32,
            height: (PAPER.height * scale).ceil() as u32,
        };
        book.render_tile(page, scale, rect).unwrap()
    }

    #[test]
    fn a_figure_is_drawn_in_its_rectangle() {
        let book = images_book();
        let (page, n) = image_of(&book, 15.0, 15.0);
        let i = &book.ink[page].images[n];
        let image = page_at(&book, page, 2.0);
        let (cx, cy) = ((i.x + i.width / 2.0) * 2.0, (i.y + i.height / 2.0) * 2.0);
        near_to(pixel(&image, cx as u32, cy as u32), [0, 120, 200]);
        let outside = ((i.x - 3.0) * 2.0) as u32;
        assert!(is_white(&pixel(&image, outside, cy as u32)));
    }

    #[test]
    fn an_inline_image_is_drawn_inside_its_quad() {
        let book = images_book();
        let (page, n) = image_of(&book, 6.0, 6.0);
        let i = &book.ink[page].images[n];
        let quad = book
            .glyphs(page)
            .into_iter()
            .find(|g| {
                g.text == "[image]"
                    && (g.quad.corners[0].y - g.quad.corners[3].y - 6.0).abs() < 0.01
            })
            .unwrap()
            .quad;
        // The ink's rectangle is the quad: the top is the baseline less 6.
        assert!((i.y - quad.corners[3].y).abs() < 0.01, "{} {:?}", i.y, quad);
        assert!((i.x - quad.corners[0].x).abs() < 0.01);
        let scale = 4.0;
        let image = page_at(&book, page, scale);
        let (cx, cy) = ((i.x + 3.0) * scale, (i.y + 3.0) * scale);
        near_to(pixel(&image, cx as u32, cy as u32), [0, 0, 0]);
        // Nothing of it above or below the quad.
        let above = ((i.y - 0.5) * scale) as u32;
        let below = ((i.y + 6.0 + 0.5) * scale) as u32;
        assert!(is_white(&pixel(&image, cx as u32, above)));
        assert!(is_white(&pixel(&image, cx as u32, below)));
    }

    #[test]
    fn a_tile_through_an_image_matches_the_page() {
        let book = images_book();
        let (page, n) = image_of(&book, 43.4, 434.0);
        let i = &book.ink[page].images[n];
        let scale = 1.7;
        let whole = page_at(&book, page, scale);
        let (x0, y0) = ((i.x * scale) as u32 - 20, (i.y * scale) as u32 + 100);
        let mut tiles = 0;
        for ty in (y0..y0 + 5 * 41).step_by(41) {
            for tx in (x0..x0 + 3 * 37).step_by(37) {
                let rect = PixelRect {
                    x: tx,
                    y: ty,
                    width: 37,
                    height: 41,
                };
                let tile = book.render_tile(page, scale, rect).unwrap();
                let mut crop = Vec::new();
                for row in ty..ty + 41 {
                    let from = (row * whole.width() + tx) as usize * 4;
                    crop.extend_from_slice(&whole.pixels()[from..from + 37 * 4]);
                }
                assert_eq!(tile.pixels(), crop.as_slice(), "tile {tx},{ty}");
                tiles += 1;
            }
        }
        assert_eq!(tiles, 15);
        // The tiles cross the image.
        let centre = pixel(&whole, ((i.x + i.width / 2.0) * scale) as u32, y0 + 100);
        near_to(centre, [0, 150, 0]);
    }

    #[test]
    fn a_transparent_image_draws_over_white() {
        let book = images_book();
        let (page, n) = image_of(&book, 150.0, 150.0);
        let i = &book.ink[page].images[n];
        let image = page_at(&book, page, 1.0);
        let px = pixel(
            &image,
            (i.x + i.width / 2.0) as u32,
            (i.y + i.height / 2.0) as u32,
        );
        near_to(px, [228, 177, 152]);
    }

    #[test]
    fn an_image_is_decoded_once() {
        use std::sync::atomic::Ordering;
        let book = images_book();
        let (page, _) = image_of(&book, 43.4, 434.0);
        assert_eq!(book.images.decodes.load(Ordering::SeqCst), 0);
        page_at(&book, page, 1.0);
        page_at(&book, page, 1.0);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| page_at(&book, page, 1.0));
            }
        });
        assert_eq!(book.images.decodes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_broken_image_draws_a_frame() {
        let book = images_book();
        let (page, n) = image_of(&book, 7.5, 7.5);
        let i = &book.ink[page].images[n];
        let scale = 4.0;
        let image = page_at(&book, page, scale);
        let x0 = (i.x * scale).round() as u32;
        let y0 = (i.y * scale).round() as u32;
        let x1 = ((i.x + i.width) * scale).round() as u32;
        let y1 = ((i.y + i.height) * scale).round() as u32;
        assert_eq!(pixel(&image, x0, (y0 + y1) / 2), [0x99, 0x99, 0x99, 255]);
        assert_eq!(pixel(&image, (x0 + x1) / 2, y0), [0x99, 0x99, 0x99, 255]);
        assert!(is_white(&pixel(&image, (x0 + x1) / 2, (y0 + y1) / 2)));
    }

    #[test]
    fn an_image_over_the_cap_at_this_scale_draws_a_frame() {
        use std::sync::atomic::Ordering;
        let book = images_book();
        let (page, n) = image_of(&book, 43.4, 434.0);
        let i = &book.ink[page].images[n];
        // A scale at which the rectangle has 10% more pixels per side than
        // the cap allows in all.
        let scale = (MAX_PIXELS as f32 / (i.width * i.height)).sqrt() * 1.1;
        let (x0, y0) = ((i.x * scale).round() as u32, (i.y * scale).round() as u32);
        let (x1, y1) = (
            ((i.x + i.width) * scale).round() as u32,
            ((i.y + i.height) * scale).round() as u32,
        );
        assert!(u64::from(x1 - x0) * u64::from(y1 - y0) > MAX_PIXELS);
        let tile = |x: u32, y: u32| {
            let rect = PixelRect {
                x,
                y,
                width: 16,
                height: 16,
            };
            book.render_tile(page, scale, rect).unwrap()
        };
        // The top left corner of the rectangle: its frame.
        let corner = tile(x0, y0);
        assert_eq!(pixel(&corner, 0, 8), [0x99, 0x99, 0x99, 255]);
        assert_eq!(pixel(&corner, 8, 0), [0x99, 0x99, 0x99, 255]);
        // Its centre: nothing, not the image.
        let centre = tile((x0 + x1) / 2, (y0 + y1) / 2);
        assert!(centre.pixels().chunks(4).all(is_white));
        // And the image was never decoded for it.
        assert_eq!(book.images.decodes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_book_is_shareable_between_threads() {
        fn needs<T: Send + Sync>() {}
        needs::<Book>();
    }
}
