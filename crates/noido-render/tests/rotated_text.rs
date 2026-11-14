//! Text on a page the PDF itself turns (`/Rotate 90`) picks and draws as
//! the same text upright does.

use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{PdfBytes, PdfEngine};
use noido_text::PageText;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn text_pdf_page(page: usize) -> PageText {
    let bytes: PdfBytes = Arc::new(std::fs::read(format!("{FIXTURES}/text.pdf")).unwrap());
    let document = HayroEngine.open(bytes).unwrap();
    PageText::new(document.text(page).unwrap())
}

/// The char index where `word` starts.
fn start_of(page: &PageText, word: &str) -> usize {
    let byte = page.text().find(word).unwrap();
    page.text()[..byte].chars().count()
}

/// The middle of the glyph `offset` chars into `word`.
fn middle_of(page: &PageText, word: &str, offset: usize) -> (f32, f32) {
    let r = page.glyph_bounds(start_of(page, word) + offset).unwrap();
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn rotated_fixture_page_selects_like_the_upright_one() {
    let upright = text_pdf_page(0);
    let turned = text_pdf_page(1);
    assert_eq!(upright.text(), turned.text());
    let (x, y) = middle_of(&upright, "World", 2);
    let want = upright.word_at(x, y);
    assert!(want.is_some());
    let (x, y) = middle_of(&turned, "World", 2);
    assert_eq!(turned.word_at(x, y), want);
}

#[test]
fn rotated_fixture_page_places_the_caret_on_the_glyph_clicked() {
    let turned = text_pdf_page(1);
    let start = start_of(&turned, "World");
    for offset in 0..5 {
        let (x, y) = middle_of(&turned, "World", offset);
        let caret = turned.caret_at(x, y).unwrap();
        assert!(
            caret == start + offset || caret == start + offset + 1,
            "offset {offset}: caret {caret}"
        );
    }
}

#[test]
fn a_turned_word_is_highlighted_along_its_line() {
    let turned = text_pdf_page(1);
    // "ell": part of the line, so shorter along it than the whole line.
    let start = start_of(&turned, "Hello");
    let rects = turned.rects(start + 1..start + 4);
    let line = turned.line_rect_of(start).unwrap();
    assert_eq!(rects.len(), 1);
    assert!(
        rects[0].height < line.height / 2.0,
        "{:?} in {line:?}",
        rects[0]
    );
    assert!(
        (rects[0].width - line.width).abs() < 0.5,
        "{:?} in {line:?}",
        rects[0]
    );
}
