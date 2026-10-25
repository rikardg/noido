//! Text extraction: an interpreter device that records glyphs instead of
//! drawing them.

use hayro::hayro_interpret::font::{Glyph as HayroGlyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, BezPath, Point as KurboPoint, Rect};
use noido_engine::{Glyph, Point, Quad};

/// Glyph space units per em.
const EM: f64 = 1000.0;
/// How far a glyph's box reaches below and above the baseline, in glyph
/// units. hayro doesn't give fonts' ascent and descent.
const DESCENT: f64 = 200.0;
const ASCENT: f64 = 800.0;
/// The advance assumed when the font gives none (Type 3).
const DEFAULT_ADVANCE: f64 = 500.0;

/// The glyphs drawn on `page`, in page space.
pub fn page_glyphs(page: &Page<'_>) -> Vec<Glyph> {
    let cache = InterpreterCache::new();
    let (width, height) = page.render_dimensions();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = TextDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.glyphs
}

#[derive(Default)]
struct TextDevice {
    glyphs: Vec<Glyph>,
}

impl<'a> Device<'a> for TextDevice {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        for glyph in run.glyphs() {
            let t: Affine = props.transform * glyph.transform();
            let at = |x: f64, y: f64| {
                let p = t * KurboPoint::new(x, y);
                Point {
                    x: p.x as f32,
                    y: p.y as f32,
                }
            };
            let origin = at(0.0, 0.0);
            let size = (t * KurboPoint::new(0.0, EM) - t * KurboPoint::ORIGIN).hypot();
            let advance = match &**glyph {
                HayroGlyph::Outline(outline) => outline.advance_width().map(f64::from),
                HayroGlyph::Type3(_) => None,
            }
            .unwrap_or(DEFAULT_ADVANCE);
            let text = match glyph.as_unicode() {
                Some(BfString::Char(c)) => c.to_string(),
                Some(BfString::String(s)) => s,
                None => String::new(),
            };
            self.glyphs.push(Glyph {
                text,
                origin,
                size: size as f32,
                quad: Quad {
                    corners: [
                        at(0.0, -DESCENT),
                        at(advance, -DESCENT),
                        at(advance, ASCENT),
                        at(0.0, ASCENT),
                    ],
                },
                invisible: matches!(mode, DrawMode::Invisible),
            });
        }
    }
}
