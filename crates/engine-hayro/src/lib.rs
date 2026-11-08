//! noido PDF engine backed by hayro, a pure-Rust PDF rasterizer.

mod labels;
mod links;
mod outline;
mod pdf_string;
mod properties;
mod text;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Affine;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings};
use std::cell::OnceCell;

use noido_engine::{
    Destination, Document, EngineError, Glyph, Link, OutlineEntry, PdfBytes, PdfEngine, PixelRect,
    Properties, Rgba8Image, Size,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct HayroEngine;

impl PdfEngine for HayroEngine {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        let pdf = Pdf::new(bytes).map_err(|e| EngineError::Open(format!("{e:?}")))?;
        Ok(Box::new(HayroDocument {
            pdf,
            pages: OnceCell::new(),
        }))
    }
}

struct HayroDocument {
    pdf: Pdf,
    /// Page indices by object, built on the first request for links.
    pages: OnceCell<links::PageIndex>,
}

impl HayroDocument {
    fn page(&self, page: usize) -> Result<&Page<'_>, EngineError> {
        let pages = self.pdf.pages();
        pages.get(page).ok_or(EngineError::PageOutOfRange {
            page,
            count: pages.len(),
        })
    }

    /// Renders `rect` of `page` at `scale` pixels per point across and down.
    fn render(
        &self,
        page: usize,
        (scale_x, scale_y): (f32, f32),
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
            * Affine::scale_non_uniform(f64::from(scale_x), f64::from(scale_y))
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
        self.render(page, (scale, scale), rect)
    }

    fn render_page(&self, page: usize, width: u32, height: u32) -> Result<Rgba8Image, EngineError> {
        let size = self.page_size(page)?;
        let scale = (width as f32 / size.width, height as f32 / size.height);
        let rect = PixelRect {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.render(page, scale, rect)
    }

    fn text(&self, page: usize) -> Result<Vec<Glyph>, EngineError> {
        Ok(text::page_glyphs(self.page(page)?))
    }

    fn links(&self, page: usize) -> Result<Vec<Link>, EngineError> {
        self.page(page)?;
        let index = self.pages.get_or_init(|| links::page_index(&self.pdf));
        Ok(links::page_links(&self.pdf, index, page))
    }

    fn named_destination(&self, name: &str) -> Option<Destination> {
        let index = self.pages.get_or_init(|| links::page_index(&self.pdf));
        links::named_destination(&self.pdf, index, name.as_bytes())
    }

    fn outline(&self) -> Vec<OutlineEntry> {
        let index = self.pages.get_or_init(|| links::page_index(&self.pdf));
        outline::outline(&self.pdf, index)
    }

    fn page_labels(&self) -> Vec<Option<String>> {
        labels::page_labels(&self.pdf)
    }

    fn properties(&self) -> Properties {
        properties::properties(&self.pdf)
    }
}
