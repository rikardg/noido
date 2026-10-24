use std::sync::Arc;

use crate::{PixelRect, Rgba8Image, Size};

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
    fn open(&self, bytes: Arc<[u8]>) -> Result<Box<dyn Document>, EngineError>;
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
}
