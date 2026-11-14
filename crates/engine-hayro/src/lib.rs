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
use self_cell::self_cell;
use std::cell::{OnceCell, RefCell};

use noido_engine::{
    Destination, Document, EngineError, Glyph, Link, OutlineEntry, PdfBytes, PdfEngine, PixelRect,
    Properties, Rgba8Image, Size,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct HayroEngine;

impl PdfEngine for HayroEngine {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        let pdf = Pdf::new(bytes).map_err(|e| EngineError::Open(format!("{e:?}")))?;
        Ok(Box::new(HayroDocument::new(pdf)))
    }
}

/// The render cache and the scale it was started at.
struct CacheSlot<'a> {
    cache: RenderCache<'a>,
    scale: (f32, f32),
    /// How many renders have used it.
    renders: usize,
}

// hayro's `RenderCache<'a>` borrows from the `Pdf`, so the two live in one
// self-referential cell: the `Pdf` is the owner, a `RefCell` holding the
// current cache is the dependent. hayro cannot empty or bound a cache, so
// `render` replaces the slot's contents with a fresh cache when the scale
// changes by more than 1% and after every `CACHE_RENDERS` renders; the
// `Pdf` is never re-parsed. A `RenderCache` is not `Send`,
// which is fine: a `Document` is not either.
self_cell!(
    struct PdfAndCache {
        owner: Pdf,
        #[not_covariant]
        dependent: CacheCell,
    }
);

type CacheCell<'a> = RefCell<Option<CacheSlot<'a>>>;

/// Renders one cache serves before a new one is started. hayro's cache has
/// no limit, and at one zoom it still grows about 0.4 MB per page on
/// art-heavy books (84 MB after 200 pages). 64 renders keep most of the
/// reuse, at one cold render in 64, and bound it to roughly 25 MB.
const CACHE_RENDERS: usize = 64;

/// Renders whose scale, on each axis, is within this relative margin of the
/// cache's start scale share the cache.
const SCALE_MARGIN: f32 = 0.01;

fn same_scale(start: (f32, f32), new: (f32, f32)) -> bool {
    let close = |start: f32, new: f32| (new - start).abs() <= SCALE_MARGIN * start.abs();
    close(start.0, new.0) && close(start.1, new.1)
}

struct HayroDocument {
    cell: PdfAndCache,
    /// Page indices by object, built on the first request for links.
    pages: OnceCell<links::PageIndex>,
    #[cfg(test)]
    caches_started: std::cell::Cell<usize>,
    /// Test only: the next render panics while it holds the cache.
    #[cfg(test)]
    panic_next: std::cell::Cell<bool>,
}

impl HayroDocument {
    fn new(pdf: Pdf) -> Self {
        HayroDocument {
            cell: PdfAndCache::new(pdf, |_| RefCell::new(None)),
            pages: OnceCell::new(),
            #[cfg(test)]
            caches_started: std::cell::Cell::new(0),
            #[cfg(test)]
            panic_next: std::cell::Cell::new(false),
        }
    }

    fn pdf(&self) -> &Pdf {
        self.cell.borrow_owner()
    }

    fn page(&self, page: usize) -> Result<&Page<'_>, EngineError> {
        let pages = self.pdf().pages();
        pages.get(page).ok_or(EngineError::PageOutOfRange {
            page,
            count: pages.len(),
        })
    }

    /// Renders `rect` of `page` at `scale` pixels per point across and down.
    fn render(
        &self,
        page_index: usize,
        (scale_x, scale_y): (f32, f32),
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        // Only a range check; the closure below indexes the page again.
        self.page(page_index)?;
        let max = u32::from(u16::MAX);
        if rect.width == 0 || rect.height == 0 || rect.width > max || rect.height > max {
            return Err(EngineError::TooLarge {
                width: rect.width,
                height: rect.height,
            });
        }
        let (width, height) = (rect.width as u16, rect.height as u16);

        let mut ctx = RenderContext::new(width, height);
        let transform_for = |page: &Page<'_>| {
            Affine::translate((-f64::from(rect.x), -f64::from(rect.y)))
                * Affine::scale_non_uniform(f64::from(scale_x), f64::from(scale_y))
                * page.initial_transform(true).to_kurbo()
        };
        self.cell.with_dependent(|pdf, slot| {
            let page = &pdf.pages()[page_index];
            let transform = transform_for(page);
            let scale = (scale_x, scale_y);
            // The cache is taken out for the render and put back only when
            // it ends normally: if the render panics, the cache (whose
            // mutexes the panic may have poisoned) is dropped with it, and
            // the next render starts a fresh one.
            let reusable = slot
                .borrow_mut()
                .take()
                .filter(|s| same_scale(s.scale, scale) && s.renders < CACHE_RENDERS);
            let mut held = reusable.unwrap_or_else(|| {
                #[cfg(test)]
                self.caches_started.set(self.caches_started.get() + 1);
                CacheSlot {
                    cache: RenderCache::new(),
                    scale,
                    renders: 0,
                }
            });
            #[cfg(test)]
            if self.panic_next.replace(false) {
                panic!("test: a render panicked while holding the cache");
            }
            hayro::render_into(
                page,
                &held.cache,
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &mut ctx,
                transform,
            );
            ctx.flush();
            held.renders += 1;
            *slot.borrow_mut() = Some(held);
        });

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
        self.pdf().pages().len()
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
        let index = self.pages.get_or_init(|| links::page_index(self.pdf()));
        Ok(links::page_links(self.pdf(), index, page))
    }

    fn named_destination(&self, name: &str) -> Option<Destination> {
        let index = self.pages.get_or_init(|| links::page_index(self.pdf()));
        links::named_destination(self.pdf(), index, name.as_bytes())
    }

    fn outline(&self) -> Vec<OutlineEntry> {
        let index = self.pages.get_or_init(|| links::page_index(self.pdf()));
        outline::outline(self.pdf(), index)
    }

    fn page_labels(&self) -> Vec<Option<String>> {
        labels::page_labels(self.pdf())
    }

    fn properties(&self) -> Properties {
        properties::properties(self.pdf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noido_engine::PixelRect;
    use std::sync::Arc;

    fn open(name: &str) -> HayroDocument {
        let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes: PdfBytes = Arc::new(std::fs::read(path).unwrap());
        HayroDocument::new(Pdf::new(bytes).unwrap())
    }

    /// Pixel bytes whose `Debug` is short: the length and a checksum, not
    /// the megabytes a failing comparison would otherwise print.
    #[derive(PartialEq)]
    struct Pixels(Vec<u8>);

    impl std::fmt::Debug for Pixels {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let sum: u64 = self.0.iter().map(|&b| u64::from(b)).sum();
            write!(f, "Pixels({} bytes, sum {sum})", self.0.len())
        }
    }

    fn tile(doc: &HayroDocument, page: usize, scale: f32) -> Pixels {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
        };
        let image = doc.render_tile(page, scale, rect).unwrap();
        Pixels(image.pixels().to_vec())
    }

    /// Renders a 50x50 corner of page 0 at a per-axis scale.
    fn at(doc: &HayroDocument, scale: (f32, f32)) {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 50,
            height: 50,
        };
        doc.render(0, scale, rect).unwrap();
    }

    #[test]
    fn each_axis_decides_the_cache() {
        let doc = open("two-pages.pdf");
        at(&doc, (1.0, 1.0));
        assert_eq!(doc.caches_started.get(), 1);
        // Within 1% of the start scale on both axes: shared.
        at(&doc, (1.009, 0.991));
        assert_eq!(doc.caches_started.get(), 1);
        // Only x changes by more than 1%.
        at(&doc, (1.02, 1.0));
        assert_eq!(doc.caches_started.get(), 2);
        // Only y changes by more than 1%.
        at(&doc, (1.02, 1.02));
        assert_eq!(doc.caches_started.get(), 3);
    }

    #[test]
    fn the_margin_is_relative_to_the_start_scale() {
        let doc = open("two-pages.pdf");
        at(&doc, (1.0, 1.0));
        // 1.0101 is just over 1% of 1.0, but just under 1% of itself.
        at(&doc, (1.0101, 1.0));
        assert_eq!(doc.caches_started.get(), 2);
        // Same on y, against the new cache's start (1.0101, 1.0).
        at(&doc, (1.0101, 1.0101));
        assert_eq!(doc.caches_started.get(), 3);
    }

    #[test]
    fn shared_cache_renders_like_a_fresh_one() {
        let shared = open("two-pages.pdf");
        for (page, scale) in [(0, 1.0), (0, 1.0), (1, 1.0), (0, 2.0), (1, 2.0)] {
            let fresh = open("two-pages.pdf");
            assert_eq!(
                tile(&shared, page, scale),
                tile(&fresh, page, scale),
                "page {page} at {scale}"
            );
        }
    }

    #[test]
    fn text_renders_like_with_a_fresh_cache() {
        let shared = open("text.pdf");
        for scale in [1.0, 1.0, 1.5] {
            let fresh = open("text.pdf");
            assert_eq!(tile(&shared, 0, scale), tile(&fresh, 0, scale));
        }
    }

    #[test]
    fn one_cache_per_scale() {
        let doc = open("two-pages.pdf");
        tile(&doc, 0, 1.0);
        tile(&doc, 1, 1.0);
        tile(&doc, 0, 1.0);
        tile(&doc, 0, 1.005);
        tile(&doc, 1, 0.995);
        assert_eq!(doc.caches_started.get(), 1);
        tile(&doc, 0, 1.25);
        tile(&doc, 1, 1.25);
        assert_eq!(doc.caches_started.get(), 2);
    }

    #[test]
    fn a_panicking_render_drops_the_cache() {
        let doc = open("text.pdf");
        tile(&doc, 0, 1.0);
        assert_eq!(doc.caches_started.get(), 1);
        doc.panic_next.set(true);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tile(&doc, 0, 1.0);
        }));
        assert!(panicked.is_err());
        let fresh = open("text.pdf");
        assert_eq!(tile(&doc, 0, 1.0), tile(&fresh, 0, 1.0));
        assert_eq!(doc.caches_started.get(), 2);
    }

    #[test]
    fn a_cache_serves_64_renders() {
        let doc = open("two-pages.pdf");
        for _ in 0..64 {
            tile(&doc, 0, 1.0);
        }
        assert_eq!(doc.caches_started.get(), 1);
        tile(&doc, 0, 1.0);
        assert_eq!(doc.caches_started.get(), 2);
    }
}
