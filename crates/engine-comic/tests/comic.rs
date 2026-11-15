use engine_comic::ComicEngine;
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Size};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/comic.cbz");

fn open_fixture() -> Box<dyn Document> {
    let bytes = PdfBytes::from(std::fs::read(FIXTURE).unwrap());
    ComicEngine.open(bytes).unwrap()
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
