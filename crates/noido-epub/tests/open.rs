use std::io::{Cursor, Write};

use noido_epub::{EpubError, PartKind, is_epub, open};
use zip::write::SimpleFileOptions;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn kind(k: &PartKind) -> &'static str {
    match k {
        PartKind::Text(_) => "Text",
        PartKind::Cover => "Cover",
        PartKind::Unreadable => "Unreadable",
    }
}

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const CONTAINER: &str = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

#[test]
fn book_has_its_metadata() {
    let book = open(&fixture("book.epub"), 8 << 20).ok().unwrap();
    assert_eq!(book.meta.title.as_deref(), Some("A Small Book"));
    assert_eq!(book.meta.creators, ["Ann Author", "Bo Writer"]);
    assert_eq!(book.meta.language.as_deref(), Some("sv"));
    assert_eq!(book.meta.subject.as_deref(), Some("Tests"));
}

#[test]
fn book_parts_follow_the_spine() {
    let book = open(&fixture("book.epub"), 8 << 20).ok().unwrap();
    let paths: Vec<&str> = book.parts.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "OEBPS/cover.xhtml",
            "OEBPS/text/ch1.xhtml",
            "OEBPS/text/ch 2.xhtml"
        ]
    );
    let kinds: Vec<&str> = book.parts.iter().map(|p| kind(&p.kind)).collect();
    assert_eq!(kinds, ["Cover", "Text", "Text"]);
    assert!(book.toc.is_empty());
}

#[test]
fn book2_keeps_spine_places_whatever_happens() {
    let book = open(&fixture("book2.epub"), 8 << 20).ok().unwrap();
    assert_eq!(book.meta.title.as_deref(), Some("Second Book"));
    assert_eq!(book.meta.language, None);
    let kinds: Vec<&str> = book.parts.iter().map(|p| kind(&p.kind)).collect();
    assert_eq!(
        kinds,
        ["Text", "Text", "Unreadable", "Unreadable", "Unreadable"]
    );
    assert_eq!(book.parts[1].path, "notes.html");
    assert_eq!(book.parts[2].path, "");
}

#[test]
fn drm_is_refused() {
    let err = open(&fixture("drm.epub"), 8 << 20).err().unwrap();
    assert_eq!(err, EpubError::Drm);
    assert_eq!(err.to_string(), "this EPUB is DRM-protected");
}

#[test]
fn obfuscated_fonts_open() {
    assert!(open(&fixture("obfuscated.epub"), 8 << 20).is_ok());
}

#[test]
fn too_large_counts_the_spine_xhtml() {
    let bytes = fixture("book.epub");
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
    let n: usize = [
        "OEBPS/cover.xhtml",
        "OEBPS/text/ch1.xhtml",
        "OEBPS/text/ch 2.xhtml",
    ]
    .iter()
    .map(|name| zip.by_name(name).unwrap().size() as usize)
    .sum();
    assert_eq!(open(&bytes, 100).err(), Some(EpubError::TooLarge(n)));
    assert!(open(&bytes, n).is_ok());
}

#[test]
fn not_an_epub() {
    let comic = fixture("comic.cbz");
    assert!(!is_epub(&comic));
    assert_eq!(open(&comic, 8 << 20).err(), Some(EpubError::NoContainer));
    assert_eq!(open(b"hello", 8 << 20).err(), Some(EpubError::NotZip));
    assert!(!is_epub(b"hello"));
    for name in ["book.epub", "book2.epub", "drm.epub", "obfuscated.epub"] {
        assert!(is_epub(&fixture(name)), "{name}");
    }
}

#[test]
fn an_empty_spine_is_refused() {
    let opf =
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest/><spine/></package>"#;
    let bytes = zip_of(&[("META-INF/container.xml", CONTAINER), ("p.opf", opf)]);
    let err = open(&bytes, 8 << 20).err().unwrap();
    assert_eq!(err, EpubError::EmptySpine);
    assert_eq!(err.to_string(), "not a readable EPUB: the spine is empty");
}

#[test]
fn a_container_without_package_is_refused() {
    let bytes = zip_of(&[("META-INF/container.xml", CONTAINER)]);
    let err = open(&bytes, 8 << 20).err().unwrap();
    assert_eq!(err, EpubError::NoPackage);
    assert_eq!(err.to_string(), "not a readable EPUB: no package");
}

#[test]
fn only_the_first_spine_item_can_be_a_cover() {
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>
        <item id="a" href="a.png" media-type="image/png"/>
        <item id="b" href="b.png" media-type="image/png"/>
        <item id="c" href="c.xhtml" media-type="application/xhtml+xml"/>
        <item id="d" href="d.xhtml" media-type="application/xhtml+xml"/>
        </manifest><spine><itemref idref="a"/><itemref idref="b"/><itemref idref="c"/><itemref idref="d"/></spine>
        <guide><reference type="cover" href="c.xhtml"/></guide></package>"#;
    let xhtml = |body: &str| {
        format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>{body}</body></html>"#)
    };
    let c = xhtml("<img src=\"a.png\"/>");
    let d = xhtml("<p>Text</p>");
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", opf),
        ("c.xhtml", &c),
        ("d.xhtml", &d),
    ]);
    let book = open(&bytes, 8 << 20).ok().unwrap();
    let kinds: Vec<&str> = book.parts.iter().map(|p| kind(&p.kind)).collect();
    assert_eq!(kinds, ["Cover", "Unreadable", "Text", "Text"]);
}

#[test]
fn a_guide_cover_without_characters_is_a_cover_and_with_characters_is_text() {
    let make = |first_body: &str| {
        let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>
            <item id="c" href="c.xhtml" media-type="application/xhtml+xml"/>
            </manifest><spine><itemref idref="c"/></spine>
            <guide><reference type="cover" href="c.xhtml"/></guide></package>"#;
        let c = format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>{first_body}</body></html>"#
        );
        let bytes = zip_of(&[
            ("META-INF/container.xml", CONTAINER),
            ("p.opf", opf),
            ("c.xhtml", &c),
        ]);
        let book = open(&bytes, 8 << 20).ok().unwrap();
        kind(&book.parts[0].kind)
    };
    assert_eq!(make("<div/>"), "Cover");
    assert_eq!(make("<p>Title</p>"), "Text");
}

/// The kind of a book's first part, whose file is `first_body` in an
/// indented real-looking file.
fn first_part(first_body: &str, marked: bool) -> &'static str {
    let guide = if marked {
        r#"<guide><reference type="cover" href="c.xhtml"/></guide>"#
    } else {
        ""
    };
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>
        <item id="c" href="c.xhtml" media-type="application/xhtml+xml"/>
        </manifest><spine><itemref idref="c"/></spine>{guide}</package>"#
    );
    let c = format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">\n  <head>\n    <title>Cover</title>\n  </head>\n  <body>\n{first_body}\n  </body>\n</html>\n"
    );
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", &opf),
        ("c.xhtml", &c),
    ]);
    kind(&open(&bytes, 8 << 20).ok().unwrap().parts[0].kind)
}

#[test]
fn an_indented_image_cover_is_a_cover() {
    assert_eq!(
        first_part(
            "    <div>\n      <img src=\"cover.jpg\" alt=\"\"/>\n    </div>",
            false
        ),
        "Cover"
    );
}

#[test]
fn an_indented_svg_cover_is_a_cover() {
    let svg = "    <div>\n      <svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 600 800\">\n        <image width=\"600\" height=\"800\" xlink:href=\"cover.jpg\"/>\n      </svg>\n    </div>";
    assert_eq!(first_part(svg, false), "Cover");
    assert_eq!(first_part(svg, true), "Cover");
}

#[test]
fn an_svg_cover_with_a_title_is_a_cover() {
    let svg = "    <div>\n      <svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 600 800\">\n        <title>Cover</title>\n        <image width=\"600\" height=\"800\" xlink:href=\"cover.jpg\"/>\n      </svg>\n    </div>";
    assert_eq!(first_part(svg, false), "Cover");
    assert_eq!(first_part(svg, true), "Cover");
}

#[test]
fn a_cover_file_with_text_stays_text() {
    assert_eq!(
        first_part(
            "    <h1>Title</h1>\n    <img src=\"cover.jpg\" alt=\"\"/>",
            true
        ),
        "Text"
    );
    assert_eq!(first_part("    <h1>Title</h1>", false), "Text");
}

#[test]
fn a_file_with_no_text_and_no_image_is_cover_only_when_marked() {
    assert_eq!(first_part("    <div>\n    </div>", false), "Text");
    assert_eq!(first_part("    <div>\n    </div>", true), "Cover");
}
