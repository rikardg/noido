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

/// A PDF whose page has form X1, which has form X2, … down to X`depth`.
fn nested_forms(depth: usize) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /XObject << /X 4 0 R >> >> >>"
            .to_owned(),
    ];
    for i in 0..depth {
        let inner = if i + 1 < depth {
            format!("/XObject << /X {} 0 R >>", 5 + i)
        } else {
            "/Font << /F << /Type /Font /Subtype /Type1 /BaseFont /Deepest >> >>".to_owned()
        };
        objects.push(format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << {inner} >> \
             /Length 0 >>\nstream\n\nendstream"
        ));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn deeply_nested_forms_do_not_overflow_the_stack() {
    let bytes: PdfBytes = Arc::new(nested_forms(5_000));
    let fonts = HayroEngine.open(bytes).unwrap().properties().fonts;
    assert_eq!(fonts, vec![font("Deepest", false)]);
}
