use crate::{
    Destination, Glyph, Link, LocationMap, OutlineEntry, PdfBytes, PixelRect, Properties,
    Rgba8Image, Size,
};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Why the document could not be opened. The shared error names no
    /// format; the engine that tried names the one it reads.
    #[error("{0}")]
    Open(String),
    #[error("page {page} out of range (document has {count})")]
    PageOutOfRange { page: usize, count: usize },
    #[error("render of {width}×{height} px is too large")]
    TooLarge { width: u32, height: u32 },
    #[error("render failed: {0}")]
    Render(String),
}

/// A PDF backend. Opening is cheap to call from any thread; the returned
/// document stays on the thread that opened it.
pub trait PdfEngine: Send + Sync {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError>;
}

/// An open document. Not `Send`: it lives on the render thread.
pub trait Document {
    fn page_count(&self) -> usize;

    /// Page size in PDF points, after applying the page's rotation.
    fn page_size(&self, page: usize) -> Result<Size, EngineError>;

    /// Renders `rect` of `page` at `scale` pixels per point on a white background.
    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError>;

    /// Renders the whole of `page` stretched to fill up to `width` ×
    /// `height` pixels, on a white background, so no edge pixel is partly
    /// background. A raster engine may return fewer pixels rather than
    /// upscale; the renderer treats that as the page's render. By default,
    /// `render_tile` at the width's scale, which may leave the last row
    /// partly background.
    fn render_page(&self, page: usize, width: u32, height: u32) -> Result<Rgba8Image, EngineError> {
        let size = self.page_size(page)?;
        let rect = PixelRect {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.render_tile(page, width as f32 / size.width, rect)
    }

    /// The glyphs drawn on `page`, in content-stream order. Engines that
    /// can't read text give none.
    fn text(&self, page: usize) -> Result<Vec<Glyph>, EngineError> {
        let _ = page;
        Ok(Vec::new())
    }

    /// The links on `page`, in annotation order: where they overlap, the
    /// later one is on top. Engines that can't read links give none.
    fn links(&self, page: usize) -> Result<Vec<Link>, EngineError> {
        let _ = page;
        Ok(Vec::new())
    }

    /// Where the destination named `name` goes. Engines that can't read
    /// named destinations give none.
    fn named_destination(&self, name: &str) -> Option<Destination> {
        let _ = name;
        None
    }

    /// The document's outline (bookmarks) as a tree; empty when it has none
    /// or for engines that can't read it.
    fn outline(&self) -> Vec<OutlineEntry> {
        Vec::new()
    }

    /// The label each page declares for itself, usually the number printed
    /// on it, `None` for a page without one. Empty when no page has one, or
    /// for engines that can't read them.
    fn page_labels(&self) -> Vec<Option<String>> {
        Vec::new()
    }

    /// What the document says about itself, and the fonts it uses. Slow on
    /// a long document: every page's resources are visited. Engines that
    /// can't read it give none.
    fn properties(&self) -> Properties {
        Properties::default()
    }

    /// Where each page's characters sit in the parts. By default each
    /// page is its own part, as for a PDF.
    fn locations(&self) -> LocationMap {
        LocationMap::identity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OnePage;

    impl Document for OnePage {
        fn page_count(&self) -> usize {
            1
        }

        fn page_size(&self, _: usize) -> Result<Size, EngineError> {
            Ok(Size {
                width: 612.0,
                height: 792.0,
            })
        }

        fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
            Err(EngineError::Render("not drawn".into()))
        }
    }

    #[test]
    fn an_open_error_shows_only_its_message() {
        assert_eq!(EngineError::Open("nope".into()).to_string(), "nope");
    }

    #[test]
    fn document_without_properties_has_none() {
        assert_eq!(OnePage.properties(), crate::Properties::default());
    }

    #[test]
    fn a_document_is_its_own_pages_by_default() {
        assert!(!OnePage.locations().reflows());
    }

    #[test]
    fn document_without_links_has_none() {
        assert_eq!(OnePage.links(0).unwrap(), Vec::<crate::Link>::new());
    }
}
