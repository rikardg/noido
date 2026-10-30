use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{
    Destination, Document, EngineError, Link, LinkTarget, PdfBytes, PdfEngine, Rect,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn open(path: &str) -> Box<dyn Document> {
    let bytes: PdfBytes = Arc::new(std::fs::read(path).unwrap());
    HayroEngine.open(bytes).unwrap()
}

/// The links on page 0 of the fixture: A, B, C, D, G and H, in that order
/// (E, F, I, J and K are not links noido follows).
fn fixture_links() -> Vec<Link> {
    open(&format!("{FIXTURES}/links.pdf")).links(0).unwrap()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn assert_rect(actual: Rect, expected: Rect) {
    assert!(
        close(actual.x, expected.x)
            && close(actual.y, expected.y)
            && close(actual.width, expected.width)
            && close(actual.height, expected.height),
        "{actual:?} != {expected:?}"
    );
}

fn assert_option(actual: Option<f32>, expected: Option<f32>) {
    match (actual, expected) {
        (Some(a), Some(b)) => assert!(close(a, b), "{a} != {b}"),
        (a, b) => assert_eq!(a, b),
    }
}

fn assert_internal(link: &Link, expected: Destination) {
    let LinkTarget::Internal(dest) = link.target else {
        panic!("not internal: {link:?}");
    };
    assert_eq!(dest.page, expected.page);
    assert_option(dest.x, expected.x);
    assert_option(dest.y, expected.y);
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn explicit_xyz_destination() {
    let links = fixture_links();
    assert_eq!(links[0].areas.len(), 1);
    assert_rect(links[0].areas[0], rect(72.0, 82.0, 100.0, 20.0));
    assert_internal(
        &links[0],
        Destination {
            page: 1,
            x: Some(72.0),
            y: Some(192.0),
        },
    );
}

#[test]
fn named_destination_in_names_tree() {
    assert_internal(
        &fixture_links()[1],
        Destination {
            page: 1,
            x: None,
            y: Some(292.0),
        },
    );
}

#[test]
fn named_destination_in_old_dests() {
    assert_internal(
        &fixture_links()[2],
        Destination {
            page: 1,
            x: None,
            y: None,
        },
    );
}

#[test]
fn uri_link_areas_from_quadpoints() {
    let link = &fixture_links()[3];
    assert_eq!(link.target, LinkTarget::Web("https://example.org/".into()));
    assert_eq!(link.areas.len(), 2);
    assert_rect(link.areas[0], rect(72.0, 172.0, 228.0, 10.0));
    assert_rect(link.areas[1], rect(72.0, 202.0, 78.0, 10.0));
}

#[test]
fn unresolvable_and_other_actions_are_skipped() {
    let links = fixture_links();
    let pages: Vec<Option<usize>> = links
        .iter()
        .map(|link| match link.target {
            LinkTarget::Internal(dest) => Some(dest.page),
            LinkTarget::Web(_) => None,
        })
        .collect();
    // A, B, C, D, G, H, L, M, N.
    assert_eq!(
        pages,
        vec![
            Some(1),
            Some(1),
            Some(1),
            None,
            Some(2),
            Some(1),
            Some(1),
            None,
            None
        ]
    );
}

#[test]
fn target_on_rotated_page() {
    assert_internal(
        &fixture_links()[4],
        Destination {
            page: 2,
            x: None,
            y: Some(100.0),
        },
    );
}

#[test]
fn fitr_destination_and_clipped_area() {
    let link = &fixture_links()[5];
    assert_eq!(link.areas.len(), 1);
    assert_rect(link.areas[0], rect(500.0, 0.0, 112.0, 32.0));
    assert_internal(
        link,
        Destination {
            page: 1,
            x: Some(100.0),
            y: Some(392.0),
        },
    );
}

#[test]
fn page_without_annots_has_no_links() {
    assert!(
        open(&format!("{FIXTURES}/links.pdf"))
            .links(1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn links_page_out_of_range() {
    assert!(matches!(
        open(&format!("{FIXTURES}/links.pdf")).links(3),
        Err(EngineError::PageOutOfRange { page: 3, count: 3 })
    ));
}

/// hyperref's internal links: GoTo a named destination that is
/// `[page /XYZ left top null]`.
#[test]
fn named_xyz_destination() {
    assert_internal(
        &fixture_links()[6],
        Destination {
            page: 1,
            x: Some(90.0),
            y: Some(92.0),
        },
    );
}

/// A web address wrapped over two lines is two links with the same address.
#[test]
fn wrapped_web_address_is_two_links() {
    let links = fixture_links();
    let wrapped = LinkTarget::Web("https://example.org/a/long/address".into());
    assert_eq!(links[7].target, wrapped);
    assert_eq!(links[8].target, wrapped);
    assert_rect(links[7].areas[0], rect(72.0, 432.0, 228.0, 20.0));
    assert_rect(links[8].areas[0], rect(72.0, 462.0, 78.0, 20.0));
}

#[test]
fn name_tree_with_shared_kids_ends() {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let links = open(&format!("{FIXTURES}/shared-names.pdf")).links(0);
        let _ = done.send(links.map(|l| l.len()));
    });
    let count = finished
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("looking a name up in a tree of shared kids didn't end");
    assert_eq!(count.unwrap(), 0);
}
