use std::sync::Arc;
use std::time::{Duration, Instant};

use noido_engine::{
    Destination, Document, EngineError, Glyph, Link, LinkTarget, PdfBytes, PdfEngine, PixelRect,
    Point, Quad, Rect, Rgba8Image, Size,
};
use noido_render::{FLASH_BAR_HEIGHT, LinkReply, Linker};

/// Letter pages, each with lines of text and links. Lines start at x = 72
/// with baselines 20 pt apart from y = 100; glyphs are 6 pt wide, their
/// boxes from 10 pt above the baseline to 2 pt below.
#[derive(Clone)]
struct LinkEngine {
    lines: Vec<Vec<&'static str>>,
    links: Vec<Vec<Link>>,
    /// A page whose links fail.
    failing: Option<usize>,
}

impl PdfEngine for LinkEngine {
    fn open(&self, _: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        Ok(Box::new(self.clone()))
    }
}

impl Document for LinkEngine {
    fn page_count(&self) -> usize {
        self.lines.len()
    }

    fn page_size(&self, _: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 612.0,
            height: 792.0,
        })
    }

    fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
        Err(EngineError::Render("not drawn".into()))
    }

    fn text(&self, page: usize) -> Result<Vec<Glyph>, EngineError> {
        let mut glyphs = Vec::new();
        for (i, line) in self.lines[page].iter().enumerate() {
            let baseline = 100.0 + 20.0 * i as f32;
            for (j, c) in line.chars().enumerate() {
                let x = 72.0 + 6.0 * j as f32;
                let p = |x, y| Point { x, y };
                glyphs.push(Glyph {
                    text: c.to_string(),
                    origin: p(x, baseline),
                    size: 12.0,
                    quad: Quad {
                        corners: [
                            p(x, baseline + 2.0),
                            p(x + 6.0, baseline + 2.0),
                            p(x + 6.0, baseline - 10.0),
                            p(x, baseline - 10.0),
                        ],
                    },
                    invisible: false,
                });
            }
        }
        Ok(glyphs)
    }

    fn links(&self, page: usize) -> Result<Vec<Link>, EngineError> {
        if self.failing == Some(page) {
            return Err(EngineError::Render("broken".into()));
        }
        Ok(self.links[page].clone())
    }

    fn named_destination(&self, name: &str) -> Option<Destination> {
        (name == "a").then(|| target(1, Some(50.0)))
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn web_link() -> Link {
    Link {
        areas: vec![rect(72.0, 90.0, 30.0, 12.0)],
        target: LinkTarget::Web("https://example.org/".into()),
    }
}

/// Page 0: two lines and a link; page 1: links fail; page 2: blank.
fn linker() -> Linker {
    let engine = LinkEngine {
        lines: vec![vec!["first", "second"], vec![], vec![]],
        links: vec![vec![web_link()], vec![], vec![]],
        failing: Some(1),
    };
    Linker::start(Arc::new(engine), Arc::new(Vec::new()), Box::new(|| {}))
}

fn recv(linker: &Linker) -> LinkReply {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(reply) = linker.try_recv() {
            return reply;
        }
        assert!(Instant::now() < deadline, "no reply within 5 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn target(page: usize, y: Option<f32>) -> Destination {
    Destination {
        page,
        x: Some(72.0),
        y,
    }
}

fn flash_area(linker: &Linker, target: Destination) -> Option<Rect> {
    linker.request_flash(target);
    match recv(linker) {
        LinkReply::Flash { target: got, area } => {
            assert_eq!(got, target);
            area
        }
        other => panic!("{other:?}, not a flash"),
    }
}

#[test]
fn answers_links_of_a_page() {
    let linker = linker();
    linker.request_links(0);
    match recv(&linker) {
        LinkReply::Links { page, links } => {
            assert_eq!(page, 0);
            assert_eq!(links, vec![web_link()]);
        }
        other => panic!("{other:?}, not links"),
    }
}

#[test]
fn failed_links_answer_empty() {
    let linker = linker();
    linker.request_links(1);
    match recv(&linker) {
        LinkReply::Links { page, links } => {
            assert_eq!(page, 1);
            assert!(links.is_empty());
        }
        other => panic!("{other:?}, not links"),
    }
}

#[test]
fn flash_is_the_line_below_target() {
    // The second line's box starts at 110.
    assert_eq!(
        flash_area(&linker(), target(0, Some(108.0))),
        Some(rect(72.0, 110.0, 36.0, 12.0))
    );
}

#[test]
fn flash_without_line_is_a_bar_across_the_text() {
    assert_eq!(
        flash_area(&linker(), target(0, Some(500.0))),
        Some(rect(72.0, 499.0, 36.0, FLASH_BAR_HEIGHT))
    );
}

#[test]
fn flash_on_blank_page_is_a_bar_across_most_of_the_page() {
    let area = flash_area(&linker(), target(2, Some(300.0))).unwrap();
    assert!((area.x - 61.2).abs() < 0.01);
    assert!((area.width - 489.6).abs() < 0.01);
    assert_eq!((area.y, area.height), (299.0, FLASH_BAR_HEIGHT));
}

#[test]
fn flash_without_y_or_text_is_none() {
    assert_eq!(flash_area(&linker(), target(2, None)), None);
}

#[test]
fn replies_come_in_request_order() {
    let linker = linker();
    linker.request_flash(target(0, None));
    linker.request_links(0);
    linker.request_flash(target(2, None));
    assert!(matches!(recv(&linker), LinkReply::Flash { target: t, .. } if t.page == 0));
    assert!(matches!(recv(&linker), LinkReply::Links { page: 0, .. }));
    assert!(matches!(recv(&linker), LinkReply::Flash { target: t, .. } if t.page == 2));
}

#[test]
fn answers_a_named_place() {
    let linker = linker();
    linker.request_named("a".into());
    match recv(&linker) {
        LinkReply::Named { name, target: t } => {
            assert_eq!(name, "a");
            assert_eq!(t, Some(target(1, Some(50.0))));
        }
        other => panic!("{other:?}"),
    }
    linker.request_named("b".into());
    assert!(matches!(
        recv(&linker),
        LinkReply::Named { target: None, .. }
    ));
}
