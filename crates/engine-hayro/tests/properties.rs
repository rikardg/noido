use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{Date, Document, Font, PdfBytes, PdfEngine, Properties};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn open(name: &str) -> Box<dyn Document> {
    let bytes: PdfBytes = Arc::new(std::fs::read(format!("{FIXTURES}/{name}")).unwrap());
    HayroEngine.open(bytes).unwrap()
}

fn properties() -> Properties {
    open("properties.pdf").properties()
}

fn font(name: &str, embedded: bool) -> Font {
    Font {
        name: name.into(),
        embedded,
    }
}

#[test]
fn info_dictionary_strings_are_decoded() {
    let p = properties();
    assert_eq!(p.title.as_deref(), Some("Info Title"));
    assert_eq!(p.author.as_deref(), Some("Åsa Olson"));
    assert_eq!(p.keywords.as_deref(), Some("café — menu"));
    assert_eq!(p.producer.as_deref(), Some("make_fixtures.py"));
}

#[test]
fn info_dictionary_dates_keep_their_offset() {
    assert_eq!(
        properties().created,
        Some(Date {
            year: 2024,
            month: 3,
            day: 5,
            hour: 14,
            minute: 22,
            second: 1,
            utc_offset: Some(60),
        })
    );
}

#[test]
fn xmp_fills_what_the_info_dictionary_lacks() {
    let p = properties();
    assert_eq!(p.subject.as_deref(), Some("XMP subject"));
    assert_eq!(p.creator.as_deref(), Some("XMP Writer 1.0"));
    assert_eq!(
        p.modified,
        Some(Date {
            year: 2024,
            month: 6,
            day: 1,
            hour: 9,
            minute: 30,
            second: 0,
            utc_offset: Some(-240),
        })
    );
}

#[test]
fn catalog_flags_and_version() {
    let p = properties();
    assert_eq!(p.version.as_deref(), Some("1.7"));
    assert!(p.tagged);
    assert!(p.has_outline);
}

#[test]
fn fonts_from_pages_and_forms_once_each() {
    assert_eq!(
        properties().fonts,
        vec![
            font("ABCDEF+MinionPro-It", true),
            font("GHIJKL+NotoSans-Bold", true),
            font("Helvetica", false),
            font("TimesNewRomanPSMT", false),
        ]
    );
}

#[test]
fn a_bare_document_has_no_properties() {
    let p = open("two-pages.pdf").properties();
    assert_eq!(
        p,
        Properties {
            version: Some("1.7".into()),
            ..Properties::default()
        }
    );
}
