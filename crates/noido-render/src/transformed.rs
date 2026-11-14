//! The document as the reader shows it: a wrapper on the engine's document
//! that turns each page by the reader's rotation, so the threads, the
//! layout and the text layer above it all see ordinary pages of the new
//! shape. The engine draws pages upright; the wrapper turns the pixels.

use std::sync::{Arc, RwLock};

use noido_core::{PageTransform, PageTransforms};
use noido_engine::{
    Destination, Document, EngineError, Glyph, Link, LinkTarget, OutlineEntry, PdfBytes, PdfEngine,
    PixelRect, Point, Properties, Rect, Rgba8Image, Size,
};

/// The reader's page transforms, shared by every thread's document: a
/// change shows in each one's next call.
#[derive(Clone, Default)]
pub struct Transforms(Arc<RwLock<PageTransforms>>);

impl Transforms {
    pub fn new(transforms: PageTransforms) -> Self {
        Self(Arc::new(RwLock::new(transforms)))
    }

    pub fn get(&self, page: usize) -> PageTransform {
        self.0.read().unwrap().get(page)
    }

    pub fn set(&self, page: usize, transform: PageTransform) {
        self.0.write().unwrap().set(page, transform);
    }

    pub fn snapshot(&self) -> PageTransforms {
        self.0.read().unwrap().clone()
    }

    /// Whether the document is scaled to fit.
    pub fn scale_to_fit(&self) -> bool {
        self.0.read().unwrap().scale_to_fit()
    }

    pub fn set_scale_to_fit(&self, on: bool) {
        self.0.write().unwrap().set_scale_to_fit(on);
    }

    /// Replaces every page's transform and the toggle.
    pub fn replace(&self, transforms: PageTransforms) {
        *self.0.write().unwrap() = transforms;
    }

    /// Sets every page's scale from the engine's (unturned) page sizes.
    pub fn refit(&self, engine_sizes: &[Size]) {
        self.0.write().unwrap().refit(engine_sizes);
    }

    /// Drops transforms at or past `page_count`.
    pub fn retain_below(&self, page_count: usize) {
        self.0.write().unwrap().retain_below(page_count);
    }
}

/// An engine document shown with the reader's transforms.
pub struct Transformed {
    inner: Box<dyn Document>,
    transforms: Transforms,
}

/// Opens `bytes` with `engine`, shown with `transforms`.
pub fn open_transformed(
    engine: &dyn PdfEngine,
    bytes: PdfBytes,
    transforms: &Transforms,
) -> Result<Transformed, EngineError> {
    Ok(Transformed::new(engine.open(bytes)?, transforms.clone()))
}

impl Transformed {
    pub fn new(inner: Box<dyn Document>, transforms: Transforms) -> Self {
        Self { inner, transforms }
    }

    /// How `page` is shown now.
    pub fn transform(&self, page: usize) -> PageTransform {
        self.transforms.get(page)
    }

    /// The inner document's page sizes: unturned and unscaled.
    pub fn engine_sizes(&self) -> Result<Vec<Size>, EngineError> {
        (0..self.inner.page_count())
            .map(|page| self.inner.page_size(page))
            .collect()
    }

    /// A destination as shown: its point turned and scaled with the page
    /// it is on. Each shown coordinate comes from one engine one, so a
    /// missing coordinate leaves the one it becomes missing.
    fn destination(&self, target: Destination) -> Destination {
        let transform = self.transform(target.page);
        let Ok(size) = self.inner.page_size(target.page) else {
            return target;
        };
        let p = transform.point(
            Point {
                x: target.x.unwrap_or(f32::NAN),
                y: target.y.unwrap_or(f32::NAN),
            },
            size,
        );
        let known = |v: f32| (!v.is_nan()).then_some(v);
        Destination {
            page: target.page,
            x: known(p.x),
            y: known(p.y),
        }
    }

    fn link_target(&self, target: LinkTarget) -> LinkTarget {
        match target {
            LinkTarget::Internal(d) => LinkTarget::Internal(self.destination(d)),
            web => web,
        }
    }

    fn outline_entry(&self, entry: OutlineEntry) -> OutlineEntry {
        OutlineEntry {
            target: entry.target.map(|t| self.link_target(t)),
            children: entry
                .children
                .into_iter()
                .map(|c| self.outline_entry(c))
                .collect(),
            ..entry
        }
    }
}

impl Document for Transformed {
    fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    fn page_size(&self, page: usize) -> Result<Size, EngineError> {
        let size = self.inner.page_size(page)?;
        Ok(self.transform(page).size(size))
    }

    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        let transform = self.transform(page);
        let rotation = transform.rotation;
        // The shown page at `scale` is the turned page at `scale` times
        // the page's own scale.
        let scale = scale * transform.scale;
        if rotation == noido_core::Rotation::R0 {
            return self.inner.render_tile(page, scale, rect);
        }
        // The tile's place on the unturned page, in whole pixels of the
        // page at `scale`.
        let turned = rotation.size(self.inner.page_size(page)?);
        let pixels = Size {
            width: (turned.width * scale).round(),
            height: (turned.height * scale).round(),
        };
        let part = rotation.inverse().rect(
            Rect {
                x: rect.x as f32,
                y: rect.y as f32,
                width: rect.width as f32,
                height: rect.height as f32,
            },
            pixels,
        );
        let unturned = PixelRect {
            x: part.x.round().max(0.0) as u32,
            y: part.y.round().max(0.0) as u32,
            width: part.width.round() as u32,
            height: part.height.round() as u32,
        };
        Ok(self
            .inner
            .render_tile(page, scale, unturned)?
            .turned(rotation))
    }

    fn render_page(&self, page: usize, width: u32, height: u32) -> Result<Rgba8Image, EngineError> {
        let rotation = self.transform(page).rotation;
        let (w, h) = match rotation {
            noido_core::Rotation::R90 | noido_core::Rotation::R270 => (height, width),
            _ => (width, height),
        };
        Ok(self.inner.render_page(page, w, h)?.turned(rotation))
    }

    fn text(&self, page: usize) -> Result<Vec<Glyph>, EngineError> {
        let glyphs = self.inner.text(page)?;
        let transform = self.transform(page);
        if transform.is_identity() {
            return Ok(glyphs);
        }
        let size = self.inner.page_size(page)?;
        Ok(glyphs
            .into_iter()
            .map(|g| Glyph {
                origin: transform.point(g.origin, size),
                quad: transform.quad(g.quad, size),
                size: g.size * transform.scale,
                ..g
            })
            .collect())
    }

    fn links(&self, page: usize) -> Result<Vec<Link>, EngineError> {
        let links = self.inner.links(page)?;
        let transform = self.transform(page);
        let size = self.inner.page_size(page)?;
        Ok(links
            .into_iter()
            .map(|link| Link {
                areas: link
                    .areas
                    .into_iter()
                    .map(|a| transform.rect(a, size))
                    .collect(),
                target: self.link_target(link.target),
            })
            .collect())
    }

    fn named_destination(&self, name: &str) -> Option<Destination> {
        self.inner
            .named_destination(name)
            .map(|d| self.destination(d))
    }

    fn outline(&self) -> Vec<OutlineEntry> {
        self.inner
            .outline()
            .into_iter()
            .map(|e| self.outline_entry(e))
            .collect()
    }

    fn page_labels(&self) -> Vec<Option<String>> {
        self.inner.page_labels()
    }

    fn properties(&self) -> Properties {
        self.inner.properties()
    }
}
