//! noido PDF engine backed by hayro, a pure-Rust PDF rasterizer.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Affine;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings};
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};

#[derive(Clone, Copy, Debug, Default)]
pub struct HayroEngine;

impl PdfEngine for HayroEngine {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        let pdf = Pdf::new(bytes).map_err(|e| EngineError::Open(format!("{e:?}")))?;
        Ok(Box::new(HayroDocument { pdf }))
    }
}

struct HayroDocument {
    pdf: Pdf,
}

impl HayroDocument {
    fn page(&self, page: usize) -> Result<&Page<'_>, EngineError> {
        let pages = self.pdf.pages();
        pages.get(page).ok_or(EngineError::PageOutOfRange {
            page,
            count: pages.len(),
        })
    }
}

impl Document for HayroDocument {
    fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    fn page_size(&self, page: usize) -> Result<Size, EngineError> {
        let (width, height) = self.page(page)?.render_dimensions();
        Ok(Size { width, height })
    }

    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        let page = self.page(page)?;
        let max = u32::from(u16::MAX);
        if rect.width == 0 || rect.height == 0 || rect.width > max || rect.height > max {
            return Err(EngineError::TooLarge {
                width: rect.width,
                height: rect.height,
            });
        }
        let (width, height) = (rect.width as u16, rect.height as u16);

        // The cache borrows from the parsed document, so it cannot be stored
        // alongside it; build one per render for now.
        let cache = RenderCache::new();
        let mut ctx = RenderContext::new(width, height);
        let transform = Affine::translate((-f64::from(rect.x), -f64::from(rect.y)))
            * Affine::scale(f64::from(scale))
            * page.initial_transform(true).to_kurbo();
        hayro::render_into(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &mut ctx,
            transform,
        );
        ctx.flush();

        let mut pixmap = Pixmap::new(width, height);
        ctx.render_with(
            &mut pixmap,
            &mut Resources::default(),
            RasterizerSettings {
                target_init: TargetInit::Clear(WHITE),
                ..Default::default()
            },
        );

        let pixels = pixmap
            .data()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        Rgba8Image::from_premultiplied(rect.width, rect.height, pixels)
            .ok_or_else(|| EngineError::Render("pixmap size mismatch".into()))
    }
}
