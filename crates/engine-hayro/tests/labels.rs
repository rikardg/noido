use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{Document, PdfBytes, PdfEngine};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn open(name: &str) -> Box<dyn Document> {
    let bytes: PdfBytes = Arc::new(std::fs::read(format!("{FIXTURES}/{name}")).unwrap());
    HayroEngine.open(bytes).unwrap()
}

fn s(label: &str) -> Option<String> {
    Some(label.to_owned())
}

#[test]
fn labels_of_every_range() {
    let expected = [
        None,
        s("i"),
        s("ii"),
        s("A 5"),
        s("A 6"),
        s("Cover"),
        s("Cover"),
        None,
        s("IV"),
        s("aa"),
        s("A"),
        s("1"),
    ];
    assert_eq!(open("page-labels.pdf").page_labels(), expected);
}

#[test]
fn no_labels_gives_none() {
    assert!(open("two-pages.pdf").page_labels().is_empty());
}
