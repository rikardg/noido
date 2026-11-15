//! Drawing glyph outlines with skrifa into a tiny-skia pixmap: the same
//! rasteriser typst-render uses, so the comparison is about layout.

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

pub struct Canvas {
    pub pixmap: Pixmap,
    scale: f32,
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

impl Canvas {
    pub fn new(width_pt: f32, height_pt: f32, scale: f32) -> Self {
        let mut pixmap = Pixmap::new(
            (width_pt * scale).ceil() as u32,
            (height_pt * scale).ceil() as u32,
        )
        .unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        Self { pixmap, scale }
    }

    /// One glyph with its baseline origin at (x, y), in points.
    pub fn glyph(&mut self, font: &[u8], index: u32, glyph: u32, size: f32, x: f32, y: f32) {
        let Ok(font) = FontRef::from_index(font, index) else {
            return;
        };
        let Some(outline) = font.outline_glyphs().get(GlyphId::new(glyph)) else {
            return;
        };
        let mut pen = Pen(PathBuilder::new());
        let settings = DrawSettings::unhinted(Size::new(size), LocationRef::default());
        if outline.draw(settings, &mut pen).is_err() {
            return;
        }
        let Some(path) = pen.0.finish() else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        paint.anti_alias = true;
        let t = Transform::from_translate(x, y).post_scale(self.scale, self.scale);
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
    }
}

/// The glyph and advance (in font units scaled to `size`) for `c`.
pub fn glyph_for(font: &[u8], index: u32, c: char, size: f32) -> Option<(u32, f32)> {
    let font = FontRef::from_index(font, index).ok()?;
    let gid = font.charmap().map(c)?;
    let adv = font
        .glyph_metrics(Size::new(size), LocationRef::default())
        .advance_width(gid)?;
    Some((gid.to_u32(), adv))
}
