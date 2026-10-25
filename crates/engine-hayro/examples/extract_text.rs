//! Prototype: text extraction with hayro, to judge its quality against
//! `pdftotext`. Usage: `extract_text <file.pdf> [first-page] [last-page]`
//! (1-based). Prints the text, pages separated by form feeds; timing and
//! glyph statistics go to stderr.

use std::time::Instant;

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, TransformExt, interpret_page,
};
use hayro::hayro_syntax::Pdf;
use hayro::kurbo::{Affine, BezPath, Point, Rect};

/// One glyph in page space: points from the top-left, y down.
struct PlacedGlyph {
    text: Option<String>,
    origin: Point,
    /// Font size (em height) in points.
    size: f64,
    /// Advance in points, if known.
    advance: Option<f64>,
    invisible: bool,
}

#[derive(Default)]
struct TextDevice {
    glyphs: Vec<PlacedGlyph>,
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
            // Glyph space is 1000 units per em.
            let t: Affine = props.transform * glyph.transform();
            let origin = t * Point::ORIGIN;
            let size = (t * Point::new(0.0, 1000.0) - origin).hypot();
            let advance = match &**glyph {
                Glyph::Outline(outline) => outline
                    .advance_width()
                    .map(|w| (t * Point::new(f64::from(w), 0.0) - origin).hypot()),
                Glyph::Type3(_) => None,
            };
            let text = glyph.as_unicode().map(|u| match u {
                BfString::Char(c) => c.to_string(),
                BfString::String(s) => s,
            });
            self.glyphs.push(PlacedGlyph {
                text,
                origin,
                size,
                advance,
                invisible: matches!(mode, DrawMode::Invisible),
            });
        }
    }
}

/// Joins glyphs in content-stream order into lines and words.
fn assemble(glyphs: &[PlacedGlyph]) -> String {
    let mut out = String::new();
    let mut prev: Option<&PlacedGlyph> = None;
    for g in glyphs {
        let Some(text) = &g.text else { continue };
        if let Some(p) = prev {
            let size = p.size.max(g.size).max(1.0);
            let expected_x = p.origin.x + p.advance.unwrap_or(0.5 * p.size);
            let new_line = (g.origin.y - p.origin.y).abs() > 0.5 * size
                || g.origin.x < p.origin.x - 0.5 * size;
            if new_line {
                out.push('\n');
            } else if g.origin.x - expected_x > 0.1 * size && !out.ends_with(' ') && text != " " {
                out.push(' ');
            }
        }
        out.push_str(text);
        prev = Some(g);
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: extract_text <file.pdf> [first] [last]");
    let bytes = std::fs::read(path).expect("read file");
    let start = Instant::now();
    let pdf = Pdf::new(bytes).expect("open pdf");
    let pages = pdf.pages();
    let first = args.get(2).map_or(1, |s| s.parse().unwrap());
    let last = args.get(3).map_or(pages.len(), |s| s.parse().unwrap());

    let cache = InterpreterCache::new();
    let (mut total, mut unmapped, mut invisible) = (0usize, 0usize, 0usize);
    for index in first - 1..last.min(pages.len()) {
        let page = &pages[index];
        let transform: Affine = page.initial_transform(true).to_kurbo();
        let (w, h) = page.render_dimensions();
        let mut context = Context::new(
            transform,
            Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
            &cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        let mut device = TextDevice::default();
        interpret_page(page, &mut context, &mut device);
        total += device.glyphs.len();
        unmapped += device.glyphs.iter().filter(|g| g.text.is_none()).count();
        invisible += device.glyphs.iter().filter(|g| g.invisible).count();
        if std::env::var_os("DUMP").is_some() {
            for g in &device.glyphs {
                eprintln!(
                    "{:?} at ({:.1}, {:.1}) size {:.1} adv {:?}",
                    g.text,
                    g.origin.x,
                    g.origin.y,
                    g.size,
                    g.advance.map(|a| (a * 10.0).round() / 10.0)
                );
            }
        }
        print!("{}\n\x0c", assemble(&device.glyphs));
    }
    eprintln!(
        "{} pages in {:.0} ms; {total} glyphs, {unmapped} without Unicode, {invisible} invisible",
        last.min(pages.len()) + 1 - first,
        start.elapsed().as_secs_f64() * 1000.0
    );
}
