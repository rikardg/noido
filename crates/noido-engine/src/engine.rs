use std::sync::Arc;

/// The bytes of a PDF file, shared without copying between loader, render
/// thread and engine.
pub type PdfBytes = Arc<Vec<u8>>;

use crate::{Glyph, Link, PixelRect, Rgba8Image, Size};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("not a readable PDF: {0}")]
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
    fn document_without_links_has_none() {
        assert_eq!(OnePage.links(0).unwrap(), Vec::<crate::Link>::new());
    }
}
