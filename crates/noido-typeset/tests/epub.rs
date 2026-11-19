//! An EPUB set: its title page, parts, links, outline and properties.

use std::path::PathBuf;

use noido_engine::{Destination, Format, Link, LinkTarget, Location, OutlineEntry};
use noido_text::{PageText, on_page};
use noido_typeset::{Book, typeset_epub};

/// B-format, as the window sees it.
const PAGE: (f32, f32) = (365.67, 561.26);

fn book(name: &str) -> Book {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(path).unwrap();
    typeset_epub(&noido_epub::open(&bytes, 8 << 20).unwrap())
}

fn page_text(book: &Book, page: usize) -> String {
    PageText::new(on_page(book.glyphs(page), PAGE.0, PAGE.1))
        .text()
        .to_string()
}

fn internal(link: &Link) -> Destination {
    match &link.target {
        LinkTarget::Internal(d) => *d,
        other => panic!("{other:?}"),
    }
}

/// The page and line top of the first line of `page` holding `word`.
fn line_of(book: &Book, word: &str) -> (usize, f32) {
    let map = book.locations();
    for page in 0..book.page_count() {
        let text = page_text(book, page);
        let Some(at) = text.find(word) else { continue };
        let char = text[..at].chars().count();
        let loc = map.location(page, char).unwrap();
        let (_, run) = map.line(loc).unwrap();
        return (page, run.top);
    }
    panic!("{word} is on no page");
}

fn link_over(links: &[Link], dest: impl Fn(&LinkTarget) -> bool) -> Vec<&Link> {
    links.iter().filter(|l| dest(&l.target)).collect()
}

fn lands(d: Destination, (page, top): (usize, f32)) {
    assert_eq!((d.page, d.x, d.y), (page, None, Some(top)));
}

#[test]
fn the_cover_is_a_title_page() {
    let book = book("book.epub");
    assert_eq!(page_text(&book, 0), "A Small Book\nAnn Author, Bo Writer");
    let map = book.locations();
    let chars = page_text(&book, 0).chars().count();
    for c in 0..=chars {
        let at = map.location(0, c);
        assert!(at.is_none() || at == Some(Location { part: 0, char: 0 }));
    }
}

#[test]
fn each_spine_item_starts_a_page() {
    let book = book("book.epub");
    assert_eq!(book.page_count(), 3);
    let map = book.locations();
    for part in 0..3 {
        // The chapters' files start with a newline, which isn't drawn.
        let char = usize::from(part > 0);
        assert_eq!(map.page_start(part), Some(Location { part, char }));
    }
    assert!(page_text(&book, 1).starts_with("Chapter One"));
    assert!(page_text(&book, 2).starts_with("Chapter Two"));
}

#[test]
fn a_cross_part_link_lands_on_its_id() {
    let book = book("book.epub");
    let links = book.links(1);
    let later = link_over(
        &links,
        |t| matches!(t, LinkTarget::Internal(d) if d.page == 2),
    );
    assert_eq!(later.len(), 1, "{links:?}");
    lands(internal(later[0]), line_of(&book, "Later text."));
}

#[test]
fn a_link_to_a_part_lands_on_its_start() {
    let book = book("book.epub");
    let links = book.links(2);
    let start = link_over(&links, |t| matches!(t, LinkTarget::Internal(_)));
    assert_eq!(start.len(), 1, "{links:?}");
    lands(internal(start[0]), line_of(&book, "Chapter One"));
}

#[test]
fn a_web_link_is_kept() {
    let book = book("book.epub");
    let web = book
        .links(2)
        .into_iter()
        .filter(|l| l.target == LinkTarget::Web("https://example.com/".to_string()))
        .count();
    assert_eq!(web, 1);
}

#[test]
fn a_link_outside_the_spine_is_not_clickable() {
    // Of ch 2's three links, only the part link and the web link remain.
    assert_eq!(book("book.epub").links(2).len(), 2);
}

#[test]
fn an_empty_anchor_lands_on_the_text_after_it() {
    let book = book("book2.epub");
    // Part 0 is one.html, part 1 notes.html; the note's page follows.
    let links = book.links(0);
    assert_eq!(links.len(), 1, "{links:?}");
    lands(internal(&links[0]), line_of(&book, "The note."));
    let back = book.links(1);
    assert_eq!(back.len(), 1, "{back:?}");
    let d = internal(&back[0]);
    assert_eq!(d.page, 0);
    // The end of part 0: its last line.
    let (_, last) = line_of(&book, "here.");
    assert_eq!(d.y, Some(last));
}

#[test]
fn a_link_to_a_missing_id_lands_on_the_start_of_its_part() {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>
        <item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>
        <item id="b" href="b.xhtml" media-type="application/xhtml+xml"/>
        </manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#;
    let xhtml = |body: &str| {
        format!("<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\n{body}\n</body></html>")
    };
    let a = xhtml(r#"<p><a href="b.xhtml#nosuchid">Go</a></p>"#);
    let b = xhtml("<h1>Second</h1><p>Text.</p>");
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("META-INF/container.xml", container),
        ("p.opf", opf),
        ("a.xhtml", a.as_str()),
        ("b.xhtml", b.as_str()),
    ] {
        w.start_file(name, SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();
    let book = typeset_epub(&noido_epub::open(&bytes, 8 << 20).unwrap());
    let links = book.links(0);
    assert_eq!(links.len(), 1, "{links:?}");
    lands(internal(&links[0]), line_of(&book, "Second"));
}

fn titles(entries: &[OutlineEntry]) -> Vec<(&str, Vec<&str>)> {
    entries
        .iter()
        .map(|e| {
            (
                e.title.as_str(),
                e.children.iter().map(|c| c.title.as_str()).collect(),
            )
        })
        .collect()
}

#[test]
fn the_outline_is_the_toc() {
    let book = book("book.epub");
    let outline = book.outline();
    assert_eq!(
        titles(&outline),
        [
            ("Chapter One", vec![]),
            ("Chapter Two", vec!["Later"]),
            ("Missing", vec![]),
        ]
    );
    assert!(outline.iter().all(|e| e.open));
    let target = |e: &OutlineEntry| match e.target {
        Some(LinkTarget::Internal(d)) => d,
        ref other => panic!("{other:?}"),
    };
    lands(target(&outline[0]), line_of(&book, "Chapter One"));
    lands(target(&outline[1]), line_of(&book, "Chapter Two"));
    lands(
        target(&outline[1].children[0]),
        line_of(&book, "Later text."),
    );
    assert_eq!(outline[2].target, None);
}

#[test]
fn properties_come_from_the_package() {
    let p = book("book.epub").properties();
    assert_eq!(p.format, Format::Epub);
    assert_eq!(p.title.as_deref(), Some("A Small Book"));
    assert_eq!(p.author.as_deref(), Some("Ann Author, Bo Writer"));
    assert_eq!(p.subject.as_deref(), Some("Tests"));
    assert_eq!(p.creator, None);
    assert!(p.has_outline);
}

#[test]
fn an_unreadable_part_is_a_notice() {
    let book = book("book2.epub");
    // one, notes, (missing), broken, two: parts 2 to 3 are unreadable,
    // part 4 is Latin-1.
    let map = book.locations();
    let mut notices = 0;
    for page in 0..book.page_count() {
        if page_text(&book, page) == "This chapter could not be read." {
            notices += 1;
            assert!(map.page_start(page).is_some());
        }
    }
    assert_eq!(notices, 3);
}

#[test]
fn a_file_name_alt_is_an_image_marker() {
    let book = book("book.epub");
    let text = page_text(&book, 1);
    assert!(text.contains("[image]"), "{text}");
    assert!(!text.contains("f0001-01"), "{text}");
}

#[test]
fn superscripts_and_subscripts_draw() {
    let book = book("book.epub");
    let glyphs = book.glyphs(1);
    // "H2O1": the 2 is below the line of its neighbours, the 1 above.
    let h = glyphs.iter().position(|g| g.text == "H").unwrap();
    let line = glyphs[h].origin.y;
    let (two, one) = (&glyphs[h + 1], &glyphs[h + 3]);
    assert_eq!((two.text.as_str(), one.text.as_str()), ("2", "1"));
    assert!(two.origin.y > line, "{} {line}", two.origin.y);
    assert!(one.origin.y < line, "{} {line}", one.origin.y);
}
