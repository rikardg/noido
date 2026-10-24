use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{Document, EngineError, PdfEngine, PixelRect, Size};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/two-pages.pdf");

fn open_fixture() -> Box<dyn Document> {
    let bytes: Arc<[u8]> = std::fs::read(FIXTURE).unwrap().into();
    HayroEngine.open(bytes).unwrap()
}

#[test]
fn opens_and_counts_pages() {
    assert_eq!(open_fixture().page_count(), 2);
}

#[test]
fn page_sizes_respect_rotation() {
    let doc = open_fixture();
    assert_eq!(
        doc.page_size(0).unwrap(),
        Size {
            width: 612.0,
            height: 792.0
        }
    );
    assert_eq!(
        doc.page_size(1).unwrap(),
        Size {
            width: 842.0,
            height: 595.0
        }
    );
}

#[test]
fn renders_full_page_at_scale() {
    let doc = open_fixture();
    let rect = PixelRect {
        x: 0,
        y: 0,
        width: 306,
        height: 396,
    };
    let image = doc.render_tile(0, 0.5, rect).unwrap();
    assert_eq!((image.width(), image.height()), (306, 396));
    // The square spans x 50-100 and y 296-346 (top-down) at this scale.
    let [r, g, b, _] = image.pixel(75, 396 - 75);
    assert!(
        r < 32 && g < 32 && b < 32,
        "expected black, got {r},{g},{b}"
    );
    assert_eq!(image.pixel(10, 10), [255, 255, 255, 255]);
}

#[test]
fn renders_offset_tile() {
    let doc = open_fixture();
    // Wholly inside the square, which spans x 100-200 and y 592-692 (top-down) at scale 1.
    let rect = PixelRect {
        x: 110,
        y: 602,
        width: 80,
        height: 80,
    };
    let image = doc.render_tile(0, 1.0, rect).unwrap();
    assert_eq!((image.width(), image.height()), (80, 80));
    assert!(image.pixels().chunks(4).all(|p| p[0] < 32));
}

#[test]
fn page_out_of_range() {
    let doc = open_fixture();
    assert!(matches!(
        doc.page_size(2),
        Err(EngineError::PageOutOfRange { page: 2, count: 2 })
    ));
    let rect = PixelRect {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    assert!(matches!(
        doc.render_tile(2, 1.0, rect),
        Err(EngineError::PageOutOfRange { page: 2, count: 2 })
    ));
}

#[test]
fn rejects_too_large() {
    let doc = open_fixture();
    let wide = PixelRect {
        x: 0,
        y: 0,
        width: 70_000,
        height: 10,
    };
    assert!(matches!(
        doc.render_tile(0, 1.0, wide),
        Err(EngineError::TooLarge { .. })
    ));
    let empty = PixelRect {
        x: 0,
        y: 0,
        width: 0,
        height: 10,
    };
    assert!(matches!(
        doc.render_tile(0, 1.0, empty),
        Err(EngineError::TooLarge { .. })
    ));
}

#[test]
fn garbage_and_empty_fail_to_open() {
    assert!(matches!(
        HayroEngine.open(b"hello"[..].into()),
        Err(EngineError::Open(_))
    ));
    assert!(matches!(
        HayroEngine.open([][..].into()),
        Err(EngineError::Open(_))
    ));
}
