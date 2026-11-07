use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Size};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/two-pages.pdf");

fn open_fixture() -> Box<dyn Document> {
    let bytes: PdfBytes = Arc::new(std::fs::read(FIXTURE).unwrap());
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
fn a_tiling_pattern_with_a_step_bigger_than_the_page_renders() {
    // One 10 pt red tile, repeated every 99999 pt: a tile that cannot be
    // sized by its step. Sizing it by the step overflows a 16-bit pixel
    // dimension, which vello answers with a panic.
    let bytes: PdfBytes = Arc::new(
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/tiling.pdf"
        ))
        .unwrap(),
    );
    let doc = HayroEngine.open(bytes).unwrap();
    let image = doc.render_page(0, 1224, 1584).unwrap();
    let painted = image
        .pixels()
        .chunks(4)
        .filter(|p| p[..3] != [255, 255, 255])
        .count();
    assert!(painted > 0, "the tile was not drawn");
    assert!(
        painted < 1000,
        "{painted} px painted: the tile was repeated"
    );
    let red = image
        .pixels()
        .chunks(4)
        .filter(|p| p[0] > 200 && p[1] < 60 && p[2] < 60)
        .count();
    assert!(red > 0, "the tile is not painted red");
}

#[test]
fn garbage_and_empty_fail_to_open() {
    assert!(matches!(
        HayroEngine.open(Arc::new(b"hello".to_vec())),
        Err(EngineError::Open(_))
    ));
    assert!(matches!(
        HayroEngine.open(Arc::new(Vec::new())),
        Err(EngineError::Open(_))
    ));
}

#[test]
fn open_shares_bytes_without_copying() {
    let bytes: PdfBytes = Arc::new(std::fs::read(FIXTURE).unwrap());
    let doc = HayroEngine.open(bytes.clone()).unwrap();
    // The document holds references to the caller's buffer instead of a copy.
    assert!(Arc::strong_count(&bytes) > 1);
    drop(doc);
    assert_eq!(Arc::strong_count(&bytes), 1);
}

/// Where the fixture's black square (x 100–200, 100–200 pt from the bottom)
/// lands, in points from the top-left, from its darkness-weighted centre.
fn square_centre(doc: &dyn Document, scale: f32) -> (f64, f64) {
    let size = doc.page_size(0).unwrap();
    let (width, height) = (
        (size.width * scale).round() as u32,
        (size.height * scale).round() as u32,
    );
    let image = doc.render_page(0, width, height).unwrap();
    let (mut x_sum, mut y_sum, mut total) = (0.0, 0.0, 0.0);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let dark = 1.0 - f64::from(image.pixel(x, y)[0]) / 255.0;
            x_sum += dark * (f64::from(x) + 0.5);
            y_sum += dark * (f64::from(y) + 0.5);
            total += dark;
        }
    }
    // Each axis is stretched to whole pixels on its own.
    let scale_x = f64::from(width) / f64::from(size.width);
    let scale_y = f64::from(height) / f64::from(size.height);
    (x_sum / total / scale_x, y_sum / total / scale_y)
}

#[test]
fn content_lands_in_the_same_place_at_every_scale() {
    // Thumbnails and full renders are drawn over the same page rect; any
    // offset between scales would show as a jump when one replaces the other.
    let doc = open_fixture();
    for scale in [120.0 / 612.0, 0.5, 1.0, 2.37] {
        let (x, y) = square_centre(doc.as_ref(), scale);
        assert!(
            (x - 150.0).abs() < 0.05 && (y - 642.0).abs() < 0.05,
            "scale {scale}: ({x}, {y})"
        );
    }
}

#[test]
fn whole_page_render_has_no_light_edge() {
    // Letter at 1.37 is 838.44 × 1085.04 px: neither side is whole.
    let bytes: PdfBytes = Arc::new(
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/black.pdf"
        ))
        .unwrap(),
    );
    let doc = HayroEngine.open(bytes).unwrap();
    // Page 1 is turned a quarter: each axis takes its own scale after the
    // rotation, not before.
    for (page, (width, height)) in [(0, (838, 1085)), (1, (1085, 838))] {
        let image = doc.render_page(page, width, height).unwrap();
        assert_eq!((image.width(), image.height()), (width, height));
        for y in 0..height {
            for x in 0..width {
                let [r, g, b, _] = image.pixel(x, y);
                assert!(
                    r < 8 && g < 8 && b < 8,
                    "page {page} ({x}, {y}) is {r},{g},{b}"
                );
            }
        }
    }
}
