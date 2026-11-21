use std::io::{Cursor, Write};

use noido_epub::{EpubError, PartKind, TocEntry, image_data, image_size, is_epub, open};
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
        PartKind::Cover { .. } => "Cover",
        PartKind::Image => "Image",
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
    assert_eq!(kinds, ["Cover", "Image", "Text", "Text"]);
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

fn outline(entries: &[TocEntry]) -> String {
    let parts: Vec<String> = entries
        .iter()
        .map(|e| {
            let mut s = format!("{} -> {}", e.title, e.href);
            if !e.children.is_empty() {
                s += &format!(" [{}]", outline(&e.children));
            }
            s
        })
        .collect();
    parts.join(", ")
}

#[test]
fn book_toc_comes_from_the_nav() {
    let book = open(&fixture("book.epub"), 8 << 20).ok().unwrap();
    assert_eq!(
        outline(&book.toc),
        "Chapter One -> OEBPS/text/ch1.xhtml, \
         Chapter Two -> OEBPS/text/ch 2.xhtml#top [Later -> OEBPS/text/ch 2.xhtml#later], \
         Missing -> OEBPS/nowhere.xhtml"
    );
}

#[test]
fn book2_toc_comes_from_the_ncx() {
    let book = open(&fixture("book2.epub"), 8 << 20).ok().unwrap();
    assert_eq!(
        outline(&book.toc),
        "One -> one.html [Notes -> notes.html#n1]"
    );
}

const OPF_PLAIN: &str = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#;
const A: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>x</p></body></html>"#;

#[test]
fn no_toc_is_an_empty_outline() {
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", OPF_PLAIN),
        ("a.xhtml", A),
    ]);
    assert!(open(&bytes, 8 << 20).ok().unwrap().toc.is_empty());
}

#[test]
fn a_span_heading_is_kept_with_an_empty_href() {
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="n" href="d/nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="a"/></spine></package>"#;
    let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="landmarks"><ol><li><a href="x.xhtml">Skip</a></li></ol></nav><nav epub:type="toc"><ol><li><span>Part I</span><ol><li><a href="../a.xhtml#s">A</a></li></ol></li></ol></nav></body></html>"#;
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", opf),
        ("a.xhtml", A),
        ("d/nav.xhtml", nav),
    ]);
    let book = open(&bytes, 8 << 20).ok().unwrap();
    assert_eq!(outline(&book.toc), "Part I ->  [A -> a.xhtml#s]");
}

#[test]
fn a_malformed_nav_gives_an_empty_toc() {
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="n" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="a"/></spine></package>"#;
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", opf),
        ("a.xhtml", A),
        ("nav.xhtml", "<nav><ol><li>"),
    ]);
    assert!(open(&bytes, 8 << 20).ok().unwrap().toc.is_empty());
}

#[test]
fn toc_titles_are_trimmed_and_collapsed() {
    let book = open(&fixture("book.epub"), 8 << 20).ok().unwrap();
    assert_eq!(book.toc[1].title, "Chapter Two");
    let raw = fixture("book.epub");
    let mut zip = zip::ZipArchive::new(Cursor::new(raw)).unwrap();
    let mut nav = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("OEBPS/nav.xhtml").unwrap(), &mut nav).unwrap();
    assert!(nav.contains(">\n  Chapter\n  Two <"), "{nav}");
}

#[test]
fn an_href_resolves_against_the_file_it_is_in() {
    use noido_epub::resolve;
    let base = "OEBPS/text/ch 2.xhtml";
    assert_eq!(resolve(base, "ch1.xhtml"), "OEBPS/text/ch1.xhtml");
    assert_eq!(
        resolve(base, "ch%201.xhtml#a%20b"),
        "OEBPS/text/ch 1.xhtml#a%20b"
    );
    assert_eq!(resolve(base, "../cover.png"), "OEBPS/cover.png");
    assert_eq!(resolve(base, "#top"), "OEBPS/text/ch 2.xhtml#top");
    assert_eq!(resolve("a.xhtml", "b.xhtml#x"), "b.xhtml#x");
}

const NAV_DOCTYPE: &str = "<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n<body>\n<nav epub:type=\"toc\">\n<ol>\n<li><a href=\"a.xhtml\">Chapter&nbsp;One&mdash;Start</a></li>\n</ol>\n</nav>\n</body>\n</html>\n";

const NCX_DOCTYPE: &str = "<?xml version=\"1.0\"?>\n<!DOCTYPE ncx PUBLIC \"-//NISO//DTD ncx 2005-1//EN\" \"http://www.daisy.org/z3986/2005/ncx-2005-1.dtd\">\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\"><navMap><navPoint id=\"p\"><navLabel><text>From NCX</text></navLabel><content src=\"a.xhtml\"/></navPoint></navMap></ncx>";

fn opf_with(nav: bool, ncx: bool) -> String {
    let mut items =
        String::from(r#"<item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>"#);
    if nav {
        items += r#"<item id="n" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#;
    }
    if ncx {
        items += r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#;
    }
    let toc = if ncx { r#" toc="ncx""# } else { "" };
    format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>{items}</manifest><spine{toc}><itemref idref="a"/></spine></package>"#
    )
}

#[test]
fn a_nav_with_a_doctype_and_html_entities_gives_its_toc() {
    let opf = opf_with(true, false);
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", &opf),
        ("a.xhtml", A),
        ("nav.xhtml", NAV_DOCTYPE),
    ]);
    let book = open(&bytes, 8 << 20).ok().unwrap();
    assert_eq!(outline(&book.toc), "Chapter One\u{2014}Start -> a.xhtml");
}

#[test]
fn a_ncx_with_a_doctype_gives_its_toc() {
    let opf = opf_with(false, true);
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", &opf),
        ("a.xhtml", A),
        ("toc.ncx", NCX_DOCTYPE),
    ]);
    let book = open(&bytes, 8 << 20).ok().unwrap();
    assert_eq!(outline(&book.toc), "From NCX -> a.xhtml");
}

#[test]
fn the_ncx_is_the_fallback_when_the_nav_has_no_entries() {
    let opf = opf_with(true, true);
    for nav in [Some("<nav><ol><li>"), Some("<html><body/></html>"), None] {
        let mut files = vec![
            ("META-INF/container.xml", CONTAINER),
            ("p.opf", opf.as_str()),
            ("a.xhtml", A),
            ("toc.ncx", NCX_DOCTYPE),
        ];
        if let Some(nav) = nav {
            files.push(("nav.xhtml", nav));
        }
        let book = open(&zip_of(&files), 8 << 20).ok().unwrap();
        assert_eq!(outline(&book.toc), "From NCX -> a.xhtml", "{nav:?}");
    }
}

#[test]
fn a_package_and_container_with_a_doctype_open() {
    let opf = format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE package PUBLIC \"+//ISBN 0-9673008-1-9//DTD OEB 1.2 Package//EN\" \"http://openebook.org/dtds/oeb-1.2/oebpk12.dtd\">\n{}",
        opf_with(false, false)
    );
    let container = format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE container SYSTEM \"container.dtd\">\n{CONTAINER}"
    );
    let bytes = zip_of(&[
        ("META-INF/container.xml", &container),
        ("p.opf", &opf),
        ("a.xhtml", A),
    ]);
    assert_eq!(open(&bytes, 8 << 20).ok().unwrap().parts.len(), 1);
}

fn cover_of(book: &noido_epub::Epub) -> Option<String> {
    match &book.parts[0].kind {
        PartKind::Cover { image } => image.clone(),
        _ => panic!("not a cover"),
    }
}

#[test]
fn the_cover_names_its_image() {
    let book = open(&fixture("book.epub"), 8 << 20).ok().unwrap();
    assert_eq!(cover_of(&book).as_deref(), Some("OEBPS/cover.png"));
}

/// A marked cover whose file holds no image; `manifest_extra` and
/// `metadata` may still name one.
fn marked_cover(manifest_extra: &str, metadata: &str) -> Option<String> {
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata>{metadata}</metadata><manifest>
        <item id="c" href="t/c.xhtml" media-type="application/xhtml+xml"/>{manifest_extra}
        </manifest><spine><itemref idref="c"/></spine>
        <guide><reference type="cover" href="t/c.xhtml"/></guide></package>"#
    );
    let c = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div/></body></html>"#;
    let bytes = zip_of(&[
        ("META-INF/container.xml", CONTAINER),
        ("p.opf", &opf),
        ("t/c.xhtml", c),
    ]);
    cover_of(&open(&bytes, 8 << 20).ok().unwrap())
}

#[test]
fn a_cover_with_no_image_names_none() {
    assert_eq!(marked_cover("", ""), None);
}

#[test]
fn a_cover_with_no_image_falls_back_to_the_manifest() {
    let property =
        r#"<item id="i" href="art/c.png" media-type="image/png" properties="cover-image"/>"#;
    assert_eq!(marked_cover(property, "").as_deref(), Some("art/c.png"));
    let plain = r#"<item id="i" href="art/m.png" media-type="image/png"/>"#;
    let meta = r#"<meta name="cover" content="i"/>"#;
    assert_eq!(marked_cover(plain, meta).as_deref(), Some("art/m.png"));
}

#[test]
fn an_image_in_the_spine_is_an_image_part() {
    let book = open(&fixture("images.epub"), 8 << 20).ok().unwrap();
    assert_eq!(kind(&book.parts[1].kind), "Image");
    assert_eq!(book.parts[1].path, "OEBPS/img/plate.png");
}

#[test]
fn image_size_reads_only_the_header() {
    let bytes = fixture("images.epub");
    assert_eq!(image_size(&bytes, "OEBPS/img/wide.png"), Some((800, 100)));
    assert_eq!(image_size(&bytes, "OEBPS/img/huge.png"), None);
    assert_eq!(image_size(&bytes, "OEBPS/img/broken.png"), None);
    assert_eq!(image_size(&bytes, "OEBPS/img/missing.png"), None);
}

#[test]
fn image_data_reads_the_entry() {
    let bytes = fixture("images.epub");
    let data = image_data(&bytes, "OEBPS/img/small.png").unwrap();
    assert!(data.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(image_data(&bytes, "OEBPS/img/missing.png"), None);
}
