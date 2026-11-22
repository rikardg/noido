//! An EPUB set: its title page, parts, links, outline and properties.

use std::path::PathBuf;

use noido_engine::{
    Destination, Format, Link, LinkTarget, Location, OutlineEntry, PdfBytes, PixelRect,
};
use noido_text::{PageText, on_page};
use noido_typeset::{Book, typeset_epub};

/// B-format, as the window sees it.
const PAGE: (f32, f32) = (365.67, 561.26);

fn book(name: &str) -> Book {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(path).unwrap();
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    typeset_epub(&epub, PdfBytes::from(bytes))
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
fn the_cover_is_its_image() {
    let book = book("book.epub");
    let glyphs = book.glyphs(0);
    assert_eq!(glyphs.len(), 1);
    // 30 x 40 px is 22.5 x 30 pt; the width binds on the paper.
    let (x, y, w, h) = rect(&glyphs[0]);
    near(w, 365.67);
    near(h, 487.56);
    near(x, 0.0);
    near(y, (561.26 - 487.56) / 2.0);
    let map = book.locations();
    assert_eq!(map.page_start(0), Some(Location { part: 0, char: 0 }));
    let chars = page_text(&book, 0).chars().count();
    for c in 0..=chars {
        let at = map.location(0, c);
        assert_eq!(at, Some(Location { part: 0, char: 0 }), "char {c}");
    }
    let tile = PixelRect {
        x: 0,
        y: 0,
        width: PAGE.0.ceil() as u32,
        height: PAGE.1.ceil() as u32,
    };
    let image = book.render_tile(0, 1.0, tile).unwrap();
    let at = ((PAGE.1 / 2.0) as usize * image.width() as usize + (PAGE.0 / 2.0) as usize) * 4;
    let px = &image.pixels()[at..at + 4];
    assert!(
        px[0].abs_diff(200) <= 2 && px[1] <= 2 && px[2] <= 2 && px[3] == 255,
        "{px:?}"
    );
}

#[test]
fn a_cover_without_an_image_is_a_title_page() {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>title</dc:title><dc:creator>author</dc:creator></metadata><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine><guide><reference type="cover" href="c.xhtml"/></guide></package>"#;
    let page = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div/></body></html>"#;
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("META-INF/container.xml", container),
        ("p.opf", opf),
        ("c.xhtml", page),
    ] {
        w.start_file(name, SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    assert!(matches!(
        &epub.parts[0].kind,
        noido_epub::PartKind::Cover { images } if images.is_empty()
    ));
    let book = typeset_epub(&epub, PdfBytes::from(bytes));
    assert_eq!(page_text(&book, 0), "title\nauthor");
}

#[test]
fn an_image_part_is_a_figure() {
    let book = book("images.epub");
    let last = book.page_count() - 1;
    let glyphs = book.glyphs(last);
    assert_eq!(glyphs.len(), 1);
    let (x, _, w, h) = rect(&glyphs[0]);
    near(w, 45.0);
    near(h, 45.0);
    near(x, LEFT + (270.0 - 45.0) / 2.0);
    assert_eq!(
        book.locations().page_start(last),
        Some(Location { part: 1, char: 0 })
    );
    assert_eq!(
        book.locations().location(last, 0),
        Some(Location { part: 1, char: 0 })
    );
    // The plate is 60 x 60 px of (120, 60, 0).
    let (x, y, w, h) = rect(&glyphs[0]);
    let px = pixel(&book, last, x + w / 2.0, y + h / 2.0);
    assert!(
        px[0].abs_diff(120) <= 2 && px[1].abs_diff(60) <= 2 && px[2] <= 2 && px[3] == 255,
        "{px:?}"
    );
}

/// The RGBA pixel of `page` rendered at scale 1, at (x, y) in points.
fn pixel(book: &Book, page: usize, x: f32, y: f32) -> [u8; 4] {
    let tile = PixelRect {
        x: 0,
        y: 0,
        width: PAGE.0.ceil() as u32,
        height: PAGE.1.ceil() as u32,
    };
    let image = book.render_tile(page, 1.0, tile).unwrap();
    let at = (y as usize * image.width() as usize + x as usize) * 4;
    image.pixels()[at..at + 4].try_into().unwrap()
}

#[test]
fn a_tall_cover_is_fitted_by_height() {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>t</dc:title></metadata><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/><item id="i" href="c.png" media-type="image/png" properties="cover-image"/></manifest><spine><itemref idref="c"/></spine><guide><reference type="cover" href="c.xhtml"/></guide></package>"#;
    let page = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="c.png" alt="Cover"/></div></body></html>"#;
    let mut png = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(20, 60, image::Rgb([200, 0, 0]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("META-INF/container.xml", container.as_bytes()),
        ("p.opf", opf.as_bytes()),
        ("c.xhtml", page.as_bytes()),
        ("c.png", png.get_ref().as_slice()),
    ] {
        w.start_file(name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    let book = typeset_epub(&epub, PdfBytes::from(bytes));
    let glyphs = book.glyphs(0);
    assert_eq!(glyphs.len(), 1);
    let (x, _, w, h) = rect(&glyphs[0]);
    near(h, 561.26);
    near(w, 561.26 / 3.0);
    near(x + w / 2.0, 365.67 / 2.0);
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
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    let book = typeset_epub(&epub, PdfBytes::from(bytes));
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

// Images: `images.epub`, see fixtures/make_fixtures.py.

const LEFT: f32 = (PAGE.0 - 270.0) / 2.0;
const TOP: f32 = 56.0;

/// Every glyph with its page.
fn all_glyphs(book: &Book) -> Vec<(usize, noido_engine::Glyph)> {
    (0..book.page_count())
        .flat_map(|p| book.glyphs(p).into_iter().map(move |g| (p, g)))
        .collect()
}

/// A glyph's box: left, top, width, height.
fn rect(g: &noido_engine::Glyph) -> (f32, f32, f32, f32) {
    let c = &g.quad.corners;
    (c[0].x, c[3].y, c[1].x - c[0].x, c[0].y - c[3].y)
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} is not {b}");
}

/// The glyphs with this text whose height is `height`.
fn figures(book: &Book, text: &str, height: f32) -> Vec<(usize, noido_engine::Glyph)> {
    all_glyphs(book)
        .into_iter()
        .filter(|(_, g)| g.text == text && (rect(g).3 - height).abs() < 0.01)
        .collect()
}

fn above(a: (usize, f32), b: (usize, f32)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 < b.1)
}

/// The text set as text (body size), in reading order: figures are not
/// in it. The fixture's alt texts are the file names, so every image
/// that is not drawn reads "[image]".
fn body_text(book: &Book) -> String {
    all_glyphs(book)
        .iter()
        .filter(|(_, g)| (g.size - 11.0).abs() < 0.01)
        .map(|(_, g)| g.text.as_str())
        .collect()
}

#[test]
fn a_lone_image_is_a_centred_figure() {
    let book = book("images.epub");
    let found = figures(&book, "[image]", 434.0);
    assert_eq!(found.len(), 1);
    let (x, _, w, _) = rect(&found[0].1);
    near(w, 43.4);
    near(x, LEFT + (270.0 - 43.4) / 2.0);
}

#[test]
fn a_figure_that_does_not_fit_starts_the_next_page() {
    let book = book("images.epub");
    let (page, g) = figures(&book, "[image]", 434.0).remove(0);
    assert!(page > line_of(&book, "Before the figure.").0);
    near(rect(&g).1, TOP);
}

#[test]
fn a_heading_image_is_a_figure_that_starts_a_page() {
    let book = book("images.epub");
    let first = &book.glyphs(0)[0];
    assert_eq!(first.text, "Title Art");
    let (_, top, w, _) = rect(first);
    near(w, 270.0);
    near(top, TOP);
}

#[test]
fn a_large_image_in_text_splits_its_paragraph() {
    let book = book("images.epub");
    let (page, g) = all_glyphs(&book)
        .into_iter()
        .find(|(_, g)| g.text == "chart")
        .unwrap();
    near(rect(&g).2, 270.0);
    let at = (page, rect(&g).1);
    assert!(above(line_of(&book, "Chart:"), at));
    assert!(above(at, line_of(&book, "and its caption.")));
}

#[test]
fn side_by_side_images_stack() {
    let book = book("images.epub");
    let pair = figures(&book, "[image]", 30.0);
    assert_eq!(pair.len(), 2);
    let tops: Vec<(usize, f32)> = pair.iter().map(|(p, g)| (*p, rect(g).1)).collect();
    assert!(above(tops[0], tops[1]), "{tops:?}");
}

#[test]
fn unreadable_images_keep_their_stand_in() {
    let book = book("images.epub");
    // Broken, huge and missing (the dot in the text is drawn).
    assert_eq!(body_text(&book).matches("[image]").count(), 3);
}

#[test]
fn a_cut_image_is_a_figure_as_its_header_reads() {
    let book = book("images.epub");
    let cut = figures(&book, "[image]", 7.5);
    assert_eq!(cut.len(), 1);
    near(rect(&cut[0].1).2, 7.5);
}

/// The small inline image next to "bullet": its place in `all_glyphs`.
fn dot_index(glyphs: &[(usize, noido_engine::Glyph)]) -> usize {
    glyphs
        .iter()
        .position(|(_, g)| g.text == "[image]" && (rect(g).3 - 6.0).abs() < 0.01)
        .expect("the dot")
}

#[test]
fn an_inline_image_sits_on_the_baseline() {
    let book = book("images.epub");
    let glyphs = all_glyphs(&book);
    let i = dot_index(&glyphs);
    // " ", then "b" of "bullet": the same line.
    let (_, b) = &glyphs[i + 2];
    assert_eq!(b.text, "b");
    let (_, dot) = &glyphs[i];
    let (_, _, w, h) = rect(dot);
    near(dot.quad.corners[0].y, b.origin.y);
    near(h, 6.0);
    near(w, 6.0);
    assert!(h <= b.origin.y - b.quad.corners[3].y);
}

#[test]
fn an_inline_image_takes_its_width_in_the_line() {
    let book = book("images.epub");
    let glyphs = all_glyphs(&book);
    let i = dot_index(&glyphs);
    assert_eq!(glyphs[i + 1].1.text, " ");
    let after = rect(&glyphs[i + 2].1).0;
    let dot = rect(&glyphs[i].1).0;
    assert!(after > dot + 6.0, "{after} {dot}");
}

#[test]
fn locations_do_not_move() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/images.epub");
    let bytes = std::fs::read(path).unwrap();
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    let x = match &epub.parts[0].kind {
        noido_epub::PartKind::Text(x) => x,
        _ => panic!("part 0 is not text"),
    };
    // The text ends with "The end.", then whitespace nothing draws.
    let end = x.ids.iter().find(|(id, _)| id == "end").unwrap().1 + "The end.".len();
    let book = typeset_epub(&epub, PdfBytes::from(bytes));
    let (_, run) = book.locations().last_line_of(0).unwrap();
    assert_eq!(run.start.char + run.chars, end);
    // The link from the small figure goes to "The end." and the svg
    // figure just above it, which has the same place (it has no characters).
    let links: Vec<Link> = (0..book.page_count()).flat_map(|p| book.links(p)).collect();
    assert_eq!(links.len(), 1, "{links:?}");
    let (page, svg) = figures(&book, "[image]", 15.0).pop().unwrap();
    lands(internal(&links[0]), (page, rect(&svg).1));
}

#[test]
fn a_linked_figure_is_a_link_over_the_image() {
    let book = book("images.epub");
    // The linked small.png and the svg's one are 15 pt; one has a link.
    let linked = figures(&book, "[image]", 15.0)
        .into_iter()
        .filter(|(page, g)| {
            let (x, y, w, h) = rect(g);
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            book.links(*page).iter().any(|l| {
                l.areas
                    .iter()
                    .any(|a| a.x <= cx && cx <= a.x + a.width && a.y <= cy && cy <= a.y + a.height)
            })
        })
        .count();
    assert_eq!(linked, 1);
}

/// A PNG whose header reads as `w` by `h` (it is not drawable; layout
/// reads only the header).
fn png_header(w: u32, h: u32) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(13u32.to_be_bytes());
    out.extend(&ihdr);
    out.extend(crc(&ihdr).to_be_bytes());
    // The reader goes on to the first image data: an empty chunk will do.
    out.extend(0u32.to_be_bytes());
    out.extend(b"IDAT");
    out.extend(crc(b"IDAT").to_be_bytes());
    out
}

/// A book of XHTML bodies (`p0.xhtml`, `p1.xhtml`, ...) and one 100 by
/// 100 image, `i.png`.
fn book_of(bodies: &[&str]) -> Book {
    book_of_image(bodies, (100, 100))
}

/// As [`book_of`], with `i.png` reading as `size` pixels.
fn book_of_image(bodies: &[&str], size: (u32, u32)) -> Book {
    use std::io::Write;
    let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let items: String = (0..bodies.len())
        .map(|i| {
            format!(r#"<item id="p{i}" href="p{i}.xhtml" media-type="application/xhtml+xml"/>"#)
        })
        .collect();
    let spine: String = (0..bodies.len())
        .map(|i| format!(r#"<itemref idref="p{i}"/>"#))
        .collect();
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>{items}<item id="i" href="i.png" media-type="image/png"/></manifest><spine>{spine}</spine></package>"#
    );
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, data: &[u8]| {
        w.start_file(name, opts).unwrap();
        w.write_all(data).unwrap();
    };
    add("META-INF/container.xml", container.as_bytes());
    add("p.opf", opf.as_bytes());
    add("i.png", &png_header(size.0, size.1));
    for (i, body) in bodies.iter().enumerate() {
        let xhtml =
            format!("<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>{body}</body></html>");
        add(&format!("p{i}.xhtml"), xhtml.as_bytes());
    }
    let bytes = w.finish().unwrap().into_inner();
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    typeset_epub(&epub, PdfBytes::from(bytes))
}

#[test]
fn an_id_on_a_heading_image_lands_on_the_figure() {
    let book = book_of(&[
        r##"<p><a href="#x">go</a></p><h1 id="x"><img src="i.png" alt="Art"/></h1><p>Text.</p>"##,
    ]);
    let links = book.links(0);
    assert_eq!(links.len(), 1);
    // The figure is first on page 1, 75 pt tall.
    lands(internal(&links[0]), (1, TOP));
}

#[test]
fn a_part_holding_only_a_figure_has_a_place() {
    let book = book_of(&[
        r#"<p><a href="p1.xhtml">go</a></p>"#,
        r#"<p><img src="i.png" alt="Art"/></p>"#,
    ]);
    let links = book.links(0);
    assert_eq!(links.len(), 1);
    lands(internal(&links[0]), (1, TOP));
    assert_eq!(
        book.locations().page_start(1),
        Some(Location { part: 1, char: 0 })
    );
}

#[test]
fn a_list_item_that_is_a_figure_keeps_its_marker() {
    let book = book_of(&[
        "<p>x</p>",
        r#"<ul><li><img src="i.png" alt="Art"/></li></ul>"#,
    ]);
    let glyphs = all_glyphs(&book);
    let figure = &glyphs.iter().find(|(_, g)| g.text == "Art").unwrap().1;
    let (x, top, w, _) = rect(figure);
    near(w, 75.0);
    near(x, LEFT + (270.0 - 75.0) / 2.0);
    let bullet = &glyphs.iter().find(|(_, g)| g.text == "\u{2022}").unwrap().1;
    near(rect(bullet).0, LEFT);
    assert!(bullet.origin.y > top, "{} {top}", bullet.origin.y);
}

#[test]
fn an_id_on_a_heading_image_lands_on_the_figure_with_newlines_between_blocks() {
    let book = book_of(&[
        "<p><a href=\"#x\">go</a></p>\n<h1 id=\"x\"><img src=\"i.png\" alt=\"Art\"/></h1>\n<p>Text.</p>\n",
    ]);
    let links = book.links(0);
    assert_eq!(links.len(), 1);
    lands(internal(&links[0]), (1, TOP));
}

#[test]
fn a_full_page_figure_is_its_pages_place() {
    let book = book_of_image(
        &["<p><img src=\"i.png\" alt=\"Map\"/></p>\n<p>Text after.</p>\n"],
        (100, 2000),
    );
    let map = book.locations();
    let (page, _) = map.line(Location { part: 0, char: 0 }).unwrap();
    // The figure fills its page; the text is on the next.
    assert!(page_text(&book, page + 1).contains("Text after."), "{page}");
    let start = map.page_start(page).unwrap();
    assert_eq!(map.page_char(start).map(|(p, _)| p), Some(page));
    assert_eq!(map.line(start).map(|(p, _)| p), Some(page));
}

/// A book whose first spine item is a cover file `t/c.xhtml` holding
/// `body`, with the package's `cover-image` `art/c.png` when `package`.
/// `files` are the images in the archive, 30 by 40 pixels.
fn cover_book(body: &str, package: bool, files: &[&str]) -> Book {
    use std::io::Write;
    let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let art = if package {
        r#"<item id="a" href="art/c.png" media-type="image/png" properties="cover-image"/>"#
    } else {
        ""
    };
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>title</dc:title><dc:creator>author</dc:creator></metadata><manifest><item id="c" href="t/c.xhtml" media-type="application/xhtml+xml"/>{art}</manifest><spine><itemref idref="c"/></spine></package>"#
    );
    let page = format!("<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>{body}</body></html>");
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, data: &[u8]| {
        w.start_file(name, opts).unwrap();
        w.write_all(data).unwrap();
    };
    add("META-INF/container.xml", container.as_bytes());
    add("p.opf", opf.as_bytes());
    add("t/c.xhtml", page.as_bytes());
    for file in files {
        add(file, &png_header(30, 40));
    }
    let bytes = w.finish().unwrap().into_inner();
    let epub = noido_epub::open(&bytes, 8 << 20).unwrap();
    typeset_epub(&epub, PdfBytes::from(bytes))
}

fn is_cover_image(book: &Book) -> bool {
    book.glyphs(0).len() == 1 && page_text(book, 0) != "title\nauthor"
}

#[test]
fn a_cover_image_with_a_fragment_resolves() {
    let book = cover_book(r#"<img src="../img/x.png#top"/>"#, false, &["img/x.png"]);
    assert!(is_cover_image(&book));
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="../img/x.png#v"/></svg>"#;
    assert!(is_cover_image(&cover_book(svg, false, &["img/x.png"])));
}

#[test]
fn a_cover_whose_first_image_is_missing_uses_the_packages() {
    let book = cover_book(r#"<img src="../img/gone.png"/>"#, true, &["art/c.png"]);
    assert!(is_cover_image(&book));
}

#[test]
fn an_unresolvable_cover_is_a_title_page() {
    let book = cover_book(r#"<img src="../img/gone.png"/>"#, false, &[]);
    assert_eq!(page_text(&book, 0), "title\nauthor");
    let book = cover_book(r#"<img src="../img/gone.png"/>"#, true, &[]);
    assert_eq!(page_text(&book, 0), "title\nauthor");
}

#[test]
fn the_first_image_wins_over_the_packages() {
    // Both exist; the first image is the one drawn (30 x 40 either
    // way, so tell them apart by the one that is not in the archive).
    let book = cover_book(r#"<img src="../img/x.png"/>"#, true, &["img/x.png"]);
    assert!(is_cover_image(&book));
}
