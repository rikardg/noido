use std::io::{Cursor, Read, Write};

use engine_comic::ComicEngine;
use noido_engine::{Document, EngineError, Format, PdfBytes, PdfEngine, PixelRect, Size};

fn fixture_bytes(name: &str) -> PdfBytes {
    let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    PdfBytes::from(std::fs::read(path).unwrap())
}

fn open_named(name: &str) -> Box<dyn Document> {
    ComicEngine.open(fixture_bytes(name)).unwrap()
}

fn open_fixture() -> Box<dyn Document> {
    open_named("comic.cbz")
}

fn fixture_entry(archive: &str, name: &str) -> Vec<u8> {
    let archive_bytes = fixture_bytes(archive);
    let mut zip = zip::ZipArchive::new(Cursor::new(&archive_bytes[..])).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn lists_only_images_in_natural_order() {
    // page 10 is the 20x30 one; lexical order would put it second.
    let doc = open_fixture();
    assert_eq!(doc.page_count(), 4);
    let sizes: Vec<Size> = (0..4).map(|page| doc.page_size(page).unwrap()).collect();
    assert_eq!(
        sizes,
        [
            Size {
                width: 10.0,
                height: 15.0
            },
            Size {
                width: 10.0,
                height: 15.0
            },
            Size {
                width: 20.0,
                height: 30.0
            },
            Size {
                width: 10.0,
                height: 15.0
            },
        ]
    );
}

#[test]
fn renders_a_page_at_the_requested_size() {
    let doc = open_fixture();
    let image = doc.render_page(0, 10, 15).unwrap();
    assert_eq!((image.width(), image.height()), (10, 15));
    assert_eq!(image.pixel(0, 0), [255, 0, 0, 255]);
}

#[test]
fn caps_a_render_at_the_images_own_pixels() {
    let doc = open_fixture();
    let image = doc.render_page(0, 100, 150).unwrap();
    assert_eq!((image.width(), image.height()), (10, 15));
}

#[test]
fn renders_a_tile_from_a_scaled_page() {
    let doc = open_fixture();
    let tile = doc
        .render_tile(
            0,
            2.0,
            PixelRect {
                x: 2,
                y: 3,
                width: 4,
                height: 6,
            },
        )
        .unwrap();
    assert_eq!((tile.width(), tile.height()), (4, 6));
    assert_eq!(tile.pixel(0, 0), [255, 0, 0, 255]);
}

#[test]
fn reads_comic_info() {
    let properties = open_fixture().properties();
    assert_eq!(properties.format, Format::Comic);
    assert_eq!(properties.title.as_deref(), Some("The Test Comic"));
    assert_eq!(properties.author.as_deref(), Some("A. Writer"));
    assert_eq!(
        properties.subject.as_deref(),
        Some("Four pages, out of order.")
    );
    assert_eq!(properties.keywords.as_deref(), Some("Test"));
    let created = properties.created.expect("a date");
    assert_eq!((created.year, created.month, created.day), (2026, 11, 15));
}

#[test]
fn a_page_past_the_end_is_out_of_range() {
    let doc = open_fixture();
    assert!(matches!(
        doc.page_size(4),
        Err(EngineError::PageOutOfRange { page: 4, count: 4 })
    ));
}

#[test]
fn opens_and_renders_a_deflated_archive() {
    let doc = open_named("comic-deflated.cbz");
    assert_eq!(doc.page_count(), 4);
    let image = doc.render_page(0, 10, 15).unwrap();
    assert_eq!(image.pixel(0, 0), [255, 0, 0, 255]);
}

#[test]
fn alpha_is_premultiplied() {
    let image = open_named("comic-alpha.cbz").render_page(0, 2, 2).unwrap();
    assert_eq!(image.pixel(0, 0), [100, 50, 25, 128]);
}

#[test]
fn a_broken_page_names_the_comic_page() {
    let doc = open_named("comic-broken.cbz");
    assert_eq!(doc.page_count(), 2);
    let message = doc.render_page(1, 10, 15).unwrap_err().to_string();
    assert!(message.contains("page 2"), "{message}");
}

#[test]
fn a_damaged_page_keeps_its_place_and_names_itself() {
    let doc = open_named("comic-damaged.cbz");
    assert_eq!(doc.page_count(), 2);
    let message = doc.render_page(1, 10, 15).unwrap_err().to_string();
    assert!(message.contains("page 2"), "{message}");
}

#[test]
fn a_page_over_the_pixel_cap_is_refused_before_decoding() {
    let doc = open_named("comic-huge.cbz");
    assert_eq!(
        doc.page_size(0).unwrap(),
        Size {
            width: 8000.0,
            height: 8000.0
        }
    );
    assert!(matches!(
        doc.render_page(0, 1, 1),
        Err(EngineError::TooLarge {
            width: 8000,
            height: 8000
        })
    ));
}

#[test]
fn an_archive_without_images_is_refused() {
    let error = match ComicEngine.open(fixture_bytes("comic-empty.cbz")) {
        Err(error) => error,
        Ok(_) => panic!("opened an archive without images"),
    };
    let message = error.to_string();
    assert!(message.contains("no images"), "{message}");
    assert!(message.contains("CBZ"), "{message}");
    assert!(!message.contains("PDF"), "{message}");
}

#[test]
fn a_malformed_zip_is_an_open_error() {
    let bytes = PdfBytes::from(b"PK\x03\x04not a zip".to_vec());
    assert!(ComicEngine.open(bytes).is_err());
}

#[test]
fn malformed_comic_info_leaves_defaults() {
    let properties = open_named("comic-bad-info.cbz").properties();
    assert_eq!(properties.format, Format::Comic);
    assert_eq!(properties.title, None);
}

#[test]
fn too_many_entries_is_refused() {
    use std::io::Cursor;

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for index in 0..20_001 {
        writer.start_file(format!("{index}"), options).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    let error = match ComicEngine.open(PdfBytes::from(bytes)) {
        Err(error) => error,
        Ok(_) => panic!("opened an archive with too many entries"),
    };
    assert!(error.to_string().contains("more than a comic"), "{error}");
}

#[test]
fn a_comic_info_over_the_cap_is_ignored() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("ComicInfo.xml", options).unwrap();
    writer.write_all(b"<ComicInfo><Title>Huge</Title>").unwrap();
    writer.write_all(&vec![b' '; 1024 * 1024 + 1]).unwrap();
    writer.write_all(b"</ComicInfo>").unwrap();
    writer.start_file("1.png", options).unwrap();
    writer
        .write_all(&fixture_entry("comic.cbz", "pages/1.png"))
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let properties = ComicEngine
        .open(PdfBytes::from(bytes))
        .unwrap()
        .properties();
    assert_eq!(properties.format, Format::Comic);
    assert_eq!(properties.title, None);
}

#[test]
fn an_image_with_a_lying_extension_is_recognised() {
    let png = fixture_entry("comic.cbz", "pages/1.png");
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("pages/1.jpg", options).unwrap();
    writer.write_all(&png).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let doc = ComicEngine.open(PdfBytes::from(bytes)).unwrap();
    assert_eq!(doc.page_count(), 1);
    assert_eq!(
        doc.page_size(0).unwrap(),
        Size {
            width: 10.0,
            height: 15.0
        }
    );
}

#[test]
fn duplicate_names_are_not_two_pages() {
    // `zip`'s writer refuses duplicates; a malformed archive can still hold
    // them, and its reader folds them to one page.
    let doc = open_named("comic-duplicate.cbz");
    assert_eq!(doc.page_count(), 1);
    assert_eq!(
        doc.page_size(0).unwrap(),
        Size {
            width: 10.0,
            height: 15.0
        }
    );
}
