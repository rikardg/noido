//! The engine: lays a file out once and shares the book between the
//! threads of a window.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use noido_engine::{
    Destination, Document, EngineError, Glyph, Link, LocationMap, OutlineEntry, PdfBytes,
    PdfEngine, PixelRect, Properties, Rgba8Image, Size,
};

use crate::book::{Book, typeset, typeset_epub};
use crate::house::{LAYOUT_KEY, PAPER};

/// The largest source laid out: a bigger file takes seconds and gigabytes
/// on the thread that opens it. To go once layout is lean and off that
/// thread (#120).
const MAX_SOURCE: usize = 8 * 1024 * 1024;

/// The message for a source of `n` bytes over the limit.
fn too_large(n: usize) -> String {
    const MB: usize = 1024 * 1024;
    format!(
        "too large to typeset ({} MB; the limit is {} MB)",
        n.div_ceil(MB),
        MAX_SOURCE / MB
    )
}

/// What identifies a file's bytes: a hash and the length. Cheaper to keep
/// than the bytes (which a mapped file would have to be read for), and a
/// collision between two files is not a practical worry.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    hash: u64,
    len: usize,
}

fn fingerprint(bytes: &[u8]) -> Fingerprint {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Fingerprint {
        hash: hasher.finish(),
        len: bytes.len(),
    }
}

/// Opens markdown and plain text as typeset pages. The book of the last
/// file opened is kept, so the threads that open the same bytes share one
/// layout.
#[derive(Default)]
pub struct TypesetEngine {
    last: Mutex<Option<(Fingerprint, Arc<Book>)>>,
    /// How many EPUBs were parsed, for the tests.
    #[cfg(test)]
    parsed: std::sync::atomic::AtomicUsize,
}

impl TypesetEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PdfEngine for TypesetEngine {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        let epub = noido_epub::is_epub(&bytes);
        // Refused before it is hashed.
        if !epub && bytes.len() > MAX_SOURCE {
            return Err(EngineError::Open(too_large(bytes.len())));
        }
        let print = fingerprint(&bytes);
        // Held while laying out: a second thread with the same bytes waits
        // for the book instead of parsing and making its own.
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((kept, book)) = &*last
            && *kept == print
        {
            return Ok(Box::new(TypesetDocument(book.clone())));
        }
        let book = if epub {
            #[cfg(test)]
            self.parsed
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // The limit counts the spine's text, not the archive.
            let epub = noido_epub::open(&bytes, MAX_SOURCE).map_err(|error| match error {
                noido_epub::EpubError::TooLarge(n) => EngineError::Open(too_large(n)),
                other => EngineError::Open(other.to_string()),
            })?;
            typeset_epub(&epub, bytes)
        } else {
            let source = std::str::from_utf8(&bytes)
                .map_err(|_| EngineError::Open("not a readable document".into()))?;
            typeset(source)
        };
        let book = Arc::new(book);
        *last = Some((print, book.clone()));
        Ok(Box::new(TypesetDocument(book)))
    }
}

struct TypesetDocument(Arc<Book>);

impl Document for TypesetDocument {
    fn page_count(&self) -> usize {
        self.0.page_count()
    }

    fn page_size(&self, page: usize) -> Result<Size, EngineError> {
        if page < self.0.page_count() {
            Ok(PAPER)
        } else {
            Err(EngineError::PageOutOfRange {
                page,
                count: self.0.page_count(),
            })
        }
    }

    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        self.0.render_tile(page, scale, rect)
    }

    fn text(&self, page: usize) -> Result<Vec<Glyph>, EngineError> {
        if page < self.0.page_count() {
            Ok(self.0.glyphs(page))
        } else {
            Err(EngineError::PageOutOfRange {
                page,
                count: self.0.page_count(),
            })
        }
    }

    fn links(&self, page: usize) -> Result<Vec<Link>, EngineError> {
        if page < self.0.page_count() {
            Ok(self.0.links(page))
        } else {
            Err(EngineError::PageOutOfRange {
                page,
                count: self.0.page_count(),
            })
        }
    }

    fn named_destination(&self, name: &str) -> Option<Destination> {
        self.0.named_destination(name)
    }

    fn outline(&self) -> Vec<OutlineEntry> {
        self.0.outline()
    }

    fn properties(&self) -> Properties {
        self.0.properties()
    }

    fn locations(&self) -> LocationMap {
        self.0.locations()
    }

    fn layout_key(&self) -> Option<String> {
        Some(LAYOUT_KEY.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(text: &str) -> PdfBytes {
        PdfBytes::from(text.as_bytes().to_vec())
    }

    #[test]
    fn opening_the_same_bytes_twice_lays_out_once() {
        let engine = TypesetEngine::new();
        let source = "# Hi\n\nThere.\n";
        engine.open(bytes(source)).unwrap();
        let first = engine.last.lock().unwrap().as_ref().unwrap().1.clone();
        engine.open(bytes(source)).unwrap();
        let second = engine.last.lock().unwrap().as_ref().unwrap().1.clone();
        assert!(Arc::ptr_eq(&first, &second));
        engine.open(bytes("# Other\n")).unwrap();
        let third = engine.last.lock().unwrap().as_ref().unwrap().1.clone();
        assert!(!Arc::ptr_eq(&first, &third));
    }

    #[test]
    fn opening_the_same_epub_twice_parses_it_once() {
        use std::sync::atomic::Ordering;
        let engine = TypesetEngine::new();
        let first = engine.open(fixture("book.epub")).unwrap();
        let second = engine.open(fixture("book.epub")).unwrap();
        assert_eq!(engine.parsed.load(Ordering::SeqCst), 1);
        assert_eq!(first.page_count(), second.page_count());
    }

    fn open_error(len: usize, fill: u8) -> Option<String> {
        match TypesetEngine::new().open(PdfBytes::from(vec![fill; len])) {
            Err(error) => Some(error.to_string()),
            Ok(_) => None,
        }
    }

    #[test]
    fn a_source_over_the_limit_is_refused_at_the_limit_it_names() {
        let message = open_error(MAX_SOURCE + 1, b'\n').expect("opened too large a file");
        assert_eq!(message, "too large to typeset (9 MB; the limit is 8 MB)");
    }

    #[test]
    fn a_source_at_the_limit_opens() {
        assert_eq!(open_error(MAX_SOURCE, b'\n'), None);
    }

    #[test]
    fn invalid_utf8_is_not_a_readable_document() {
        let error = match TypesetEngine::new().open(PdfBytes::from(vec![0xff, 0xfe, 0x00])) {
            Err(error) => error,
            Ok(_) => panic!("opened bytes that are not text"),
        };
        assert_eq!(error.to_string(), "not a readable document");
    }

    #[test]
    fn a_document_answers_from_its_book() {
        let document = TypesetEngine::new()
            .open(bytes("# Hi\n\nThere.\n"))
            .unwrap();
        assert_eq!(document.page_count(), 1);
        assert_eq!(document.page_size(0).unwrap(), PAPER);
        assert!(document.page_size(1).is_err());
        assert_eq!(document.layout_key().as_deref(), Some(LAYOUT_KEY));
        assert!(document.page_labels().is_empty());
        assert!(document.locations().reflows());
        assert_eq!(document.outline().len(), 1);
        assert!(!document.text(0).unwrap().is_empty());
        assert!(document.text(1).is_err());
    }

    fn fixture(name: &str) -> PdfBytes {
        PdfBytes::from(
            std::fs::read(format!(
                "{}/../../fixtures/{name}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
    }

    #[test]
    fn an_epub_opens_as_its_book() {
        let document = TypesetEngine::new().open(fixture("book.epub")).unwrap();
        assert!(document.page_count() >= 3);
        assert_eq!(document.properties().format, noido_engine::Format::Epub);
    }

    #[test]
    fn the_limit_counts_an_epubs_text_not_its_zip() {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;
        let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#;
        let page =
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hello.</p></body></html>"#;
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in [
            ("META-INF/container.xml", container),
            ("p.opf", opf),
            ("a.xhtml", page),
        ] {
            w.start_file(name, stored).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.start_file("big.png", stored).unwrap();
        w.write_all(&vec![7u8; MAX_SOURCE + 1024 * 1024]).unwrap();
        let bytes = w.finish().unwrap().into_inner();
        assert!(bytes.len() > MAX_SOURCE);
        let document = TypesetEngine::new().open(PdfBytes::from(bytes)).unwrap();
        assert_eq!(document.properties().format, noido_engine::Format::Epub);
    }

    #[test]
    fn epub_errors_are_sentences() {
        let error = match TypesetEngine::new().open(fixture("drm.epub")) {
            Err(error) => error,
            Ok(_) => panic!("opened a DRM-protected book"),
        };
        assert_eq!(error.to_string(), "this EPUB is DRM-protected");
    }
}
