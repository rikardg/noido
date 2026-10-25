use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{Document, EngineError, Glyph, PdfBytes, PdfEngine};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn open(name: &str) -> Box<dyn Document> {
    let bytes: PdfBytes = Arc::new(std::fs::read(format!("{FIXTURES}/{name}")).unwrap());
    HayroEngine.open(bytes).unwrap()
}

/// The glyphs' text with whitespace removed.
fn letters(glyphs: &[Glyph]) -> String {
    glyphs
        .iter()
        .flat_map(|g| g.text.chars())
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// The glyphs that aren't whitespace.
fn visible(glyphs: &[Glyph]) -> Vec<&Glyph> {
    glyphs
        .iter()
        .filter(|g| !g.text.trim().is_empty())
        .collect()
}

fn assert_inside_page(doc: &dyn Document, page: usize, glyphs: &[Glyph]) {
    let size = doc.page_size(page).unwrap();
    for g in glyphs {
        let (min, max) = g.quad.bounds();
        assert!(
            min.x >= 0.0 && min.y >= 0.0 && max.x <= size.width && max.y <= size.height,
            "{:?} outside {size:?}",
            g.quad
        );
    }
}

#[test]
fn text_glyphs_in_order() {
    let glyphs = open("text.pdf").text(0).unwrap();
    assert_eq!(letters(&glyphs), "HelloWorldnoido");
}

#[test]
fn text_glyph_positions() {
    let glyphs = open("text.pdf").text(0).unwrap();
    let glyphs = visible(&glyphs);
    let first = glyphs[0];
    assert!((first.origin.x - 72.0).abs() < 0.5, "{:?}", first.origin);
    assert!((first.origin.y - 92.0).abs() < 0.5, "{:?}", first.origin);
    assert!((first.size - 12.0).abs() < 0.01, "{}", first.size);
    let (min, max) = first.quad.bounds();
    assert!((min.y - (92.0 - 9.6)).abs() < 0.1, "{min:?}");
    assert!((max.y - (92.0 + 2.4)).abs() < 0.1, "{max:?}");
    // Top is above bottom on the page.
    assert!(first.quad.corners[3].y < first.quad.corners[0].y);
    for pair in glyphs[..5].windows(2) {
        assert!(pair[1].origin.x > pair[0].origin.x);
    }
}

#[test]
fn text_glyphs_inside_page() {
    let doc = open("text.pdf");
    assert_inside_page(doc.as_ref(), 0, &doc.text(0).unwrap());
}

#[test]
fn rotated_page_text() {
    let doc = open("text.pdf");
    let glyphs = doc.text(1).unwrap();
    assert_eq!(letters(&glyphs), "HelloWorldnoido");
    let hello = &visible(&glyphs)[..5];
    for pair in hello.windows(2) {
        assert!((pair[1].origin.x - pair[0].origin.x).abs() < 0.5);
        assert!(pair[1].origin.y > pair[0].origin.y);
    }
    assert_inside_page(doc.as_ref(), 1, &glyphs);
}

#[test]
fn text_on_blank_page_is_empty() {
    assert!(open("two-pages.pdf").text(1).unwrap().is_empty());
}

#[test]
fn text_page_out_of_range() {
    assert!(matches!(
        open("text.pdf").text(5),
        Err(EngineError::PageOutOfRange { page: 5, count: 2 })
    ));
}
