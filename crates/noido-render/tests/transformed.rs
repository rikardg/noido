//! The document as the reader turned it: sizes, renders, glyphs, links and
//! destinations all come out turned.

use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_core::{PageTransform, PageTransforms, Rotation};
use noido_engine::{
    Destination, Document, LinkTarget, OutlineEntry, PdfBytes, PdfEngine, Point, Size,
};
use noido_render::{Transformed, Transforms, open_transformed};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn bytes(name: &str) -> PdfBytes {
    Arc::new(std::fs::read(format!("{FIXTURES}/{name}")).unwrap())
}

fn plain(name: &str) -> Box<dyn Document> {
    HayroEngine.open(bytes(name)).unwrap()
}

fn turned(name: &str, pages: &[(usize, Rotation)]) -> Transformed {
    let transforms = Transforms::new(PageTransforms::from_records(pages.iter().map(
        |&(page, rotation)| {
            (
                page,
                PageTransform {
                    rotation,
                    ..PageTransform::default()
                },
            )
        },
    )));
    open_transformed(&HayroEngine, bytes(name), &transforms).unwrap()
}

fn size(width: f32, height: f32) -> Size {
    Size { width, height }
}

/// `target` turned by `r` on a page of unturned size `s`, coordinate by
/// coordinate.
fn turn_destination(target: Destination, r: Rotation, s: Size) -> Destination {
    let x = target.x.unwrap_or(f32::NAN);
    let y = target.y.unwrap_or(f32::NAN);
    let p = r.point(Point { x, y }, s);
    let known = |v: f32| (!v.is_nan()).then_some(v);
    Destination {
        page: target.page,
        x: known(p.x),
        y: known(p.y),
    }
}

#[test]
fn sizes_swap_on_quarter_turns() {
    let doc = turned("two-pages.pdf", &[(0, Rotation::R90)]);
    assert_eq!(doc.page_size(0).unwrap(), size(792.0, 612.0));
    assert_eq!(doc.page_size(1).unwrap(), size(842.0, 595.0));
    assert_eq!(doc.transform(0).rotation, Rotation::R90);
    assert_eq!(doc.transform(1).rotation, Rotation::R0);
}

#[test]
fn render_turns_the_square() {
    // The square's middle is at (150, 642) from the top-left, unturned.
    let doc = turned("two-pages.pdf", &[(0, Rotation::R90)]);
    let image = doc.render_page(0, 792, 612).unwrap();
    assert_eq!((image.width(), image.height()), (792, 612));
    let black = |x, y| image.pixel(x, y)[..3] == [0, 0, 0];
    assert!(black(150, 150), "where a clockwise turn puts it");
    assert!(!black(642, 462), "where an anticlockwise turn would");
    assert!(!black(150, 400));
}

#[test]
fn render_tile_turns_its_part() {
    let doc = turned("two-pages.pdf", &[(0, Rotation::R90)]);
    let whole = doc.render_page(0, 792, 612).unwrap();
    let rect = noido_engine::PixelRect {
        x: 100,
        y: 120,
        width: 100,
        height: 60,
    };
    let tile = doc.render_tile(0, 1.0, rect).unwrap();
    assert_eq!((tile.width(), tile.height()), (100, 60));
    for (x, y) in [(50, 30), (5, 5), (95, 55)] {
        assert_eq!(
            tile.pixel(x, y)[..3],
            whole.pixel(100 + x, 120 + y)[..3],
            "at ({x}, {y})"
        );
    }
}

#[test]
fn glyphs_turn_with_the_page() {
    let doc = turned("text.pdf", &[(0, Rotation::R90)]);
    let glyphs = doc.text(0).unwrap();
    let page = doc.page_size(0).unwrap();
    let visible: Vec<_> = glyphs
        .iter()
        .filter(|g| !g.text.trim().is_empty())
        .collect();
    assert!(!visible.is_empty());
    for g in &visible {
        let (min, max) = g.quad.bounds();
        assert!(
            min.x >= 0.0 && min.y >= 0.0 && max.x <= page.width && max.y <= page.height,
            "{:?} outside {page:?}",
            g.quad
        );
    }
    for pair in visible[..5].windows(2) {
        assert!((pair[1].origin.x - pair[0].origin.x).abs() < 0.5);
        assert!(pair[1].origin.y > pair[0].origin.y, "runs down the page");
    }
    let unturned = plain("text.pdf").text(0).unwrap();
    let s = size(612.0, 792.0);
    assert_eq!(glyphs[0].origin, Rotation::R90.point(unturned[0].origin, s));
    assert_eq!(glyphs[0].quad, Rotation::R90.quad(unturned[0].quad, s));
}

#[test]
fn link_areas_and_targets_turn() {
    let letter = size(612.0, 792.0);
    let unturned = plain("links.pdf").links(0).unwrap();
    let doc = turned("links.pdf", &[(0, Rotation::R90), (1, Rotation::R180)]);
    let links = doc.links(0).unwrap();
    assert_eq!(links.len(), unturned.len());
    for (link, plain) in links.iter().zip(&unturned) {
        let want: Vec<_> = plain
            .areas
            .iter()
            .map(|&a| Rotation::R90.rect(a, letter))
            .collect();
        assert_eq!(link.areas, want);
        match (&link.target, &plain.target) {
            (LinkTarget::Internal(got), LinkTarget::Internal(was)) => {
                let want = match was.page {
                    1 => turn_destination(*was, Rotation::R180, letter),
                    0 => turn_destination(*was, Rotation::R90, letter),
                    _ => *was,
                };
                assert_eq!(*got, want);
            }
            (got, was) => assert_eq!(got, was),
        }
    }
    // Link A goes to (72, 192) on page 1, which a half turn moves.
    let LinkTarget::Internal(a) = links[0].target else {
        panic!("link A is internal")
    };
    assert_eq!((a.page, a.x, a.y), (1, Some(540.0), Some(600.0)));
}

#[test]
fn a_missing_coordinate_stays_missing_on_its_own_axis() {
    // Link G goes to page 2 with an x and no y.
    let to_page_2 = |links: Vec<noido_engine::Link>| {
        links
            .into_iter()
            .find_map(|l| match l.target {
                LinkTarget::Internal(d) if d.page == 2 => Some(d),
                _ => None,
            })
            .unwrap()
    };
    // Page 2 has its own /Rotate 90, so the PDF's missing y shows as a
    // missing x; a quarter turn moves it back to y.
    let was = to_page_2(plain("links.pdf").links(0).unwrap());
    assert_eq!(was.x, None);
    let y = was.y.unwrap();
    let shown = plain("links.pdf").page_size(2).unwrap();
    let doc = turned("links.pdf", &[(2, Rotation::R90)]);
    let got = to_page_2(doc.links(0).unwrap());
    assert_eq!((got.page, got.x, got.y), (2, Some(shown.height - y), None));
}

#[test]
fn a_missing_coordinate_stays_missing_on_a_scaled_page() {
    // As above: link G reaches page 2 with no x and a y.
    let to_page_2 = |links: Vec<noido_engine::Link>| {
        links
            .into_iter()
            .find_map(|l| match l.target {
                LinkTarget::Internal(d) if d.page == 2 => Some(d),
                _ => None,
            })
            .unwrap()
    };
    let was = to_page_2(plain("links.pdf").links(0).unwrap());
    assert_eq!(was.x, None);
    let y = was.y.unwrap();
    let shown = plain("links.pdf").page_size(2).unwrap();

    let doc = scaled("links.pdf", &[(2, Rotation::R0, 0.5)]);
    let got = to_page_2(doc.links(0).unwrap());
    assert_eq!(got.page, 2);
    assert_eq!(got.x, None);
    assert!(close(got.y.unwrap(), y * 0.5));

    // Turned a quarter, the missing x becomes a missing y, and the
    // known y becomes an x.
    let doc = scaled("links.pdf", &[(2, Rotation::R90, 0.5)]);
    let got = to_page_2(doc.links(0).unwrap());
    assert_eq!(got.page, 2);
    assert_eq!(got.y, None);
    assert!(close(got.x.unwrap(), (shown.height - y) * 0.5));
}

fn first_internal(entries: &[OutlineEntry]) -> Option<Destination> {
    entries.iter().find_map(|e| match e.target {
        Some(LinkTarget::Internal(d)) => Some(d),
        _ => first_internal(&e.children),
    })
}

#[test]
fn outline_targets_turn() {
    let plain_doc = plain("outline.pdf");
    let was = first_internal(&plain_doc.outline()).unwrap();
    let unturned = plain_doc.page_size(was.page).unwrap();
    let doc = turned("outline.pdf", &[(was.page, Rotation::R270)]);
    let got = first_internal(&doc.outline()).unwrap();
    assert_eq!(got, turn_destination(was, Rotation::R270, unturned));
}

#[test]
fn changes_apply_to_the_next_call() {
    let transforms = Transforms::default();
    let doc = open_transformed(&HayroEngine, bytes("two-pages.pdf"), &transforms).unwrap();
    assert_eq!(doc.page_size(0).unwrap(), size(612.0, 792.0));
    transforms.set(
        0,
        PageTransform {
            rotation: Rotation::R270,
            ..PageTransform::default()
        },
    );
    assert_eq!(doc.page_size(0).unwrap(), size(792.0, 612.0));
    assert_eq!(transforms.get(0).rotation, Rotation::R270);
    transforms.retain_below(0);
    assert_eq!(doc.page_size(0).unwrap(), size(612.0, 792.0));
}

fn scaled(name: &str, pages: &[(usize, Rotation, f32)]) -> Transformed {
    let transforms =
        Transforms::new(PageTransforms::from_records(pages.iter().map(
            |&(page, rotation, scale)| (page, PageTransform { rotation, scale }),
        )));
    open_transformed(&HayroEngine, bytes(name), &transforms).unwrap()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn scaled_page_size() {
    let doc = scaled("two-pages.pdf", &[(0, Rotation::R0, 0.5)]);
    assert_eq!(doc.page_size(0).unwrap(), size(306.0, 396.0));
    let doc = scaled("two-pages.pdf", &[(0, Rotation::R90, 0.5)]);
    assert_eq!(doc.page_size(0).unwrap(), size(396.0, 306.0));
}

#[test]
fn scaled_glyphs() {
    let letter = size(612.0, 792.0);
    let unturned = plain("text.pdf").text(0).unwrap();
    for rotation in [Rotation::R0, Rotation::R90] {
        let t = PageTransform {
            rotation,
            scale: 0.5,
        };
        let glyphs = scaled("text.pdf", &[(0, rotation, 0.5)]).text(0).unwrap();
        assert_eq!(glyphs.len(), unturned.len());
        for (g, u) in glyphs.iter().zip(&unturned) {
            assert_eq!(g.origin, t.point(u.origin, letter));
            assert_eq!(g.quad, t.quad(u.quad, letter));
            assert!(close(g.size, u.size * 0.5), "{} vs {}", g.size, u.size);
        }
    }
}

#[test]
fn scaled_link_areas_and_targets() {
    let letter = size(612.0, 792.0);
    let unturned = plain("links.pdf").links(0).unwrap();
    for rotation in [Rotation::R0, Rotation::R90] {
        let t = PageTransform {
            rotation,
            scale: 0.5,
        };
        let doc = scaled(
            "links.pdf",
            &[(0, rotation, 0.5), (1, Rotation::R180, 0.25)],
        );
        let links = doc.links(0).unwrap();
        assert_eq!(links.len(), unturned.len());
        let t1 = PageTransform {
            rotation: Rotation::R180,
            scale: 0.25,
        };
        for (link, was) in links.iter().zip(&unturned) {
            let want: Vec<_> = was.areas.iter().map(|&a| t.rect(a, letter)).collect();
            assert_eq!(link.areas, want);
            if let (LinkTarget::Internal(got), LinkTarget::Internal(was)) =
                (&link.target, &was.target)
                && was.page == 1
            {
                let p = t1.point(
                    Point {
                        x: was.x.unwrap_or(f32::NAN),
                        y: was.y.unwrap_or(f32::NAN),
                    },
                    letter,
                );
                let known = |v: f32| (!v.is_nan()).then_some(v);
                assert_eq!((got.x, got.y), (known(p.x), known(p.y)));
            }
        }
        // Link A: (72, 192) on page 1, half-turned, then a quarter.
        let LinkTarget::Internal(a) = links[0].target else {
            panic!("link A is internal")
        };
        assert_eq!((a.page, a.x, a.y), (1, Some(135.0), Some(150.0)));
    }
}

#[test]
fn scaled_outline() {
    let plain_doc = plain("outline.pdf");
    let was = first_internal(&plain_doc.outline()).unwrap();
    let doc = scaled("outline.pdf", &[(was.page, Rotation::R0, 0.5)]);
    let got = first_internal(&doc.outline()).unwrap();
    assert_eq!(got.page, was.page);
    assert_eq!(got.x, was.x.map(|x| x * 0.5));
    assert_eq!(got.y, was.y.map(|y| y * 0.5));
}

#[test]
fn scaled_tile_matches_the_page_at_the_product_scale() {
    for rotation in [Rotation::R0, Rotation::R90] {
        let doc = scaled("two-pages.pdf", &[(0, rotation, 0.5)]);
        let reference = turned("two-pages.pdf", &[(0, rotation)]);
        let rect = noido_engine::PixelRect {
            x: 10,
            y: 20,
            width: 120,
            height: 80,
        };
        let tile = doc.render_tile(0, 1.0, rect).unwrap();
        assert_eq!((tile.width(), tile.height()), (120, 80));
        let want = reference.render_tile(0, 0.5, rect).unwrap();
        assert_eq!((want.width(), want.height()), (120, 80));
        for y in 0..80 {
            for x in 0..120 {
                assert_eq!(
                    tile.pixel(x, y),
                    want.pixel(x, y),
                    "{rotation:?} ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn scaled_render_page_has_the_size_asked_for() {
    let doc = scaled("two-pages.pdf", &[(0, Rotation::R90, 0.5)]);
    let image = doc.render_page(0, 396, 306).unwrap();
    assert_eq!((image.width(), image.height()), (396, 306));
}

#[test]
fn engine_sizes_are_the_inner_sizes() {
    let doc = scaled("two-pages.pdf", &[(0, Rotation::R90, 0.5)]);
    assert_eq!(
        doc.engine_sizes().unwrap(),
        vec![size(612.0, 792.0), size(842.0, 595.0)]
    );
}

#[test]
fn transforms_forward_scale_to_fit() {
    let transforms = Transforms::default();
    assert!(!transforms.scale_to_fit());
    transforms.set_scale_to_fit(true);
    assert!(transforms.scale_to_fit());
    transforms.refit(&[size(100.0, 100.0), size(100.0, 100.0), size(200.0, 200.0)]);
    assert_eq!(transforms.get(2).scale, 0.5);
    assert_eq!(transforms.get(0).scale, 1.0);
}
