use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use engine_hayro::HayroEngine;
use noido_core::Keep;
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};
use noido_render::{RenderRequest, RenderedPage, Renderer};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/two-pages.pdf");

fn fixture() -> PdfBytes {
    PdfBytes::from(std::fs::read(FIXTURE).unwrap())
}

fn no_waker() -> Box<dyn Fn() + Send> {
    Box::new(|| {})
}

fn recv(renderer: &Renderer) -> RenderedPage {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(page) = renderer.try_recv() {
            return page;
        }
        assert!(Instant::now() < deadline, "no render result within 5 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn start_reports_page_sizes() {
    let (_renderer, info) = Renderer::start(
        Arc::new(HayroEngine),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();
    assert_eq!(
        info.page_sizes,
        vec![
            Size {
                width: 612.0,
                height: 792.0
            },
            Size {
                width: 842.0,
                height: 595.0
            }
        ]
    );
}

#[test]
fn start_fails_on_garbage() {
    let result = Renderer::start(
        Arc::new(HayroEngine),
        PdfBytes::from(b"nope".to_vec()),
        noido_render::Transforms::default(),
        no_waker(),
    );
    assert!(matches!(result, Err(EngineError::Open(_))));
}

#[test]
fn renders_requested_page_and_wakes() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let waker = Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let (renderer, _) = Renderer::start(
        Arc::new(HayroEngine),
        fixture(),
        noido_render::Transforms::default(),
        waker,
    )
    .unwrap();
    renderer.request(RenderRequest {
        page: 0,
        scale: 0.5,
    });
    let rendered = recv(&renderer);
    assert_eq!(rendered.page, 0);
    let image = rendered.result.unwrap();
    assert_eq!((image.width(), image.height()), (306, 396));
    let deadline = Instant::now() + Duration::from_secs(5);
    while wakes.load(Ordering::SeqCst) < 1 {
        assert!(Instant::now() < deadline, "the render did not wake the app");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn out_of_range_page_reports_error() {
    let (renderer, _) = Renderer::start(
        Arc::new(HayroEngine),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();
    renderer.request(RenderRequest {
        page: 9,
        scale: 1.0,
    });
    let rendered = recv(&renderer);
    assert_eq!(rendered.page, 9);
    assert!(matches!(
        rendered.result,
        Err(EngineError::PageOutOfRange { .. })
    ));
}

/// Engine whose page 1 cannot be drawn: rendering it panics, as hayro does
/// on a page with a tiling pattern it cannot size.
struct PanicEngine;

struct PanicDocument;

impl PdfEngine for PanicEngine {
    fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        Ok(Box::new(PanicDocument))
    }
}

impl Document for PanicDocument {
    fn page_count(&self) -> usize {
        3
    }

    fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 100.0,
            height: 100.0,
        })
    }

    fn render_tile(
        &self,
        page: usize,
        _scale: f32,
        _rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        assert_ne!(page, 1, "the page that cannot be drawn");
        Ok(Rgba8Image::from_premultiplied(1, 1, vec![0; 4]).unwrap())
    }
}

#[test]
fn a_panicking_page_does_not_end_the_render_thread() {
    let (renderer, _) = Renderer::start(
        Arc::new(PanicEngine),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();
    renderer.request(RenderRequest {
        page: 1,
        scale: 1.0,
    });
    let rendered = recv(&renderer);
    assert_eq!(rendered.page, 1);
    match rendered.result {
        Err(EngineError::Render(message)) => {
            assert!(message.contains("the engine panicked"), "{message}");
            assert!(
                message.contains("the page that cannot be drawn"),
                "{message}"
            );
        }
        Ok(_) => panic!("the panicking page was drawn"),
        Err(other) => panic!("{other}, not a panic"),
    }

    // The thread lived: the page after the one that panicked still renders.
    renderer.request(RenderRequest {
        page: 2,
        scale: 1.0,
    });
    let rendered = recv(&renderer);
    assert_eq!(rendered.page, 2);
    assert!(rendered.result.is_ok(), "page 2 failed");
}

/// Engine whose first render blocks until the test releases it, and which
/// records the order of renders.
#[derive(Clone)]
struct GateEngine {
    entered: Arc<AtomicBool>,
    gate: Arc<Barrier>,
    rendered: Arc<Mutex<Vec<(usize, f32)>>>,
}

struct GateDocument(GateEngine);

impl PdfEngine for GateEngine {
    fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        Ok(Box::new(GateDocument(self.clone())))
    }
}

impl Document for GateDocument {
    fn page_count(&self) -> usize {
        6
    }

    fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 100.0,
            height: 100.0,
        })
    }

    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        _rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        if !self.0.entered.swap(true, Ordering::SeqCst) {
            self.0.gate.wait();
        }
        self.0.rendered.lock().unwrap().push((page, scale));
        Ok(Rgba8Image::from_premultiplied(1, 1, vec![0; 4]).unwrap())
    }
}

/// Fails page 0 at once and takes 40 ms over the others, so a cheap failure
/// would drag the median down.
struct SlowGoodEngine;

impl PdfEngine for SlowGoodEngine {
    fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        Ok(Box::new(SlowGoodEngine))
    }
}

impl Document for SlowGoodEngine {
    fn page_count(&self) -> usize {
        2
    }
    fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 612.0,
            height: 792.0,
        })
    }
    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        tile: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        if page == 0 {
            return Err(EngineError::Render("broken".into()));
        }
        std::thread::sleep(Duration::from_millis(40));
        let width = ((tile.width as f32 * scale) as u32).max(1);
        let height = ((tile.height as f32 * scale) as u32).max(1);
        Rgba8Image::from_premultiplied(width, height, vec![255; (width * height * 4) as usize])
            .ok_or_else(|| EngineError::Render("size".into()))
    }
}

/// Costs `ms_per_megapixel` of what it is asked for: the same page takes four
/// times as long at twice the scale, as a real one does.
#[derive(Clone, Copy)]
struct PixelCostEngine {
    ms_per_megapixel: f32,
}

impl PdfEngine for PixelCostEngine {
    fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        Ok(Box::new(*self))
    }
}

impl Document for PixelCostEngine {
    fn page_count(&self) -> usize {
        2
    }
    fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 612.0,
            height: 792.0,
        })
    }
    fn render_tile(
        &self,
        _page: usize,
        _scale: f32,
        tile: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        let megapixels = tile.width as f32 * tile.height as f32 / 1e6;
        std::thread::sleep(Duration::from_secs_f32(
            self.ms_per_megapixel * megapixels / 1000.0,
        ));
        Rgba8Image::from_premultiplied(
            tile.width,
            tile.height,
            vec![255; (tile.width * tile.height * 4) as usize],
        )
        .ok_or_else(|| EngineError::Render("size".into()))
    }
}

#[test]
fn a_page_costs_a_megapixel_whatever_the_scale() {
    let cost_at = |scale: f32| {
        let renderer = Renderer::start_background(
            Arc::new(PixelCostEngine {
                ms_per_megapixel: 20.0,
            }),
            PdfBytes::default(),
            noido_render::Transforms::default(),
            no_waker(),
        );
        renderer.request(RenderRequest { page: 0, scale });
        assert!(recv(&renderer).result.is_ok());
        renderer.render_cost_per_megapixel().expect("a cost")
    };
    // Sixteen times the pixels, and the same cost a megapixel: a median of
    // per-page milliseconds would be sixteen times larger at the larger
    // scale, and would go stale the moment the reader zoomed.
    let small = cost_at(1.0);
    let large = cost_at(4.0);
    assert!(
        small > 15.0 && large > 15.0,
        "{small} and {large} ms a megapixel, for a page that costs 20"
    );
    assert!(
        (large / small - 1.0).abs() < 0.5,
        "{large} against {small} ms a megapixel: not a cost per pixel"
    );
}

#[test]
fn a_failed_render_does_not_count_as_a_cheap_one() {
    let renderer = Renderer::start_background(
        Arc::new(SlowGoodEngine),
        PdfBytes::default(),
        noido_render::Transforms::default(),
        no_waker(),
    );
    // The good page is the slow one; it alone decides the cost.
    renderer.request(RenderRequest {
        page: 1,
        scale: 1.0,
    });
    let rendered = recv(&renderer);
    assert!(rendered.result.is_ok());
    let cost = renderer.render_cost_per_megapixel().expect("a cost");
    assert!(cost >= 35.0, "a slow page measured {cost} ms a megapixel");

    // Two failures against one slow success: the median of all three would
    // be a failure's.
    for _ in 0..2 {
        renderer.request(RenderRequest {
            page: 0,
            scale: 1.0,
        });
        let failed = recv(&renderer);
        assert!(failed.result.is_err());
    }
    let cost = renderer.render_cost_per_megapixel().expect("a cost");
    assert!(
        cost >= 35.0,
        "a failed render pulled the cost down to {cost} ms a megapixel"
    );
}

#[test]
fn the_median_is_the_middle_of_the_last_few() {
    let mut costs = vec![1.0, 2.0, 3.0, 4.0, 90.0];
    assert_eq!(noido_render::median_of(&costs), Some(3.0));
    costs = vec![1.0, 2.0, 3.0, 4.0];
    assert_eq!(noido_render::median_of(&costs), Some(3.0));
    assert_eq!(noido_render::median_of(&[]), None);
}

#[test]
fn coalesces_to_latest_per_page() {
    let engine = GateEngine {
        entered: Arc::new(AtomicBool::new(false)),
        gate: Arc::new(Barrier::new(2)),
        rendered: Arc::new(Mutex::new(Vec::new())),
    };
    let (renderer, _) = Renderer::start(
        Arc::new(engine.clone()),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();

    renderer.request(RenderRequest {
        page: 0,
        scale: 1.0,
    });
    while !engine.entered.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(1));
    }
    renderer.request(RenderRequest {
        page: 0,
        scale: 2.0,
    });
    renderer.request(RenderRequest {
        page: 1,
        scale: 1.0,
    });
    renderer.request(RenderRequest {
        page: 0,
        scale: 3.0,
    });
    engine.gate.wait();

    for _ in 0..3 {
        recv(&renderer);
    }
    assert_eq!(
        *engine.rendered.lock().unwrap(),
        vec![(0, 1.0), (0, 3.0), (1, 1.0)]
    );
}

#[test]
fn skips_pages_outside_keep_range() {
    let engine = GateEngine {
        entered: Arc::new(AtomicBool::new(false)),
        gate: Arc::new(Barrier::new(2)),
        rendered: Arc::new(Mutex::new(Vec::new())),
    };
    let (renderer, _) = Renderer::start(
        Arc::new(engine.clone()),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();

    renderer.request(RenderRequest {
        page: 0,
        scale: 1.0,
    });
    while !engine.entered.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(1));
    }
    renderer.request(RenderRequest {
        page: 1,
        scale: 1.0,
    });
    renderer.set_keep(0..1);
    engine.gate.wait();

    assert_eq!(recv(&renderer).page, 0);
    std::thread::sleep(Duration::from_millis(100));
    assert!(renderer.try_recv().is_none());
    assert_eq!(*engine.rendered.lock().unwrap(), vec![(0, 1.0)]);
}

#[test]
fn renders_extra_kept_page_outside_window() {
    let (renderer, _) = Renderer::start(
        Arc::new(HayroEngine),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();
    renderer.set_keep(Keep {
        window: 0..0,
        extra: vec![1],
    });
    renderer.request(RenderRequest {
        page: 1,
        scale: 0.1,
    });
    assert_eq!(recv(&renderer).page, 1);
}

#[test]
fn renders_in_priority_order_then_the_rest() {
    let engine = GateEngine {
        entered: Arc::new(AtomicBool::new(false)),
        gate: Arc::new(Barrier::new(2)),
        rendered: Arc::new(Mutex::new(Vec::new())),
    };
    let (renderer, _) = Renderer::start(
        Arc::new(engine.clone()),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();

    renderer.request(RenderRequest {
        page: 0,
        scale: 1.0,
    });
    while !engine.entered.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(1));
    }
    // Requested while scrolling past; then the reader stops at pages 4–5.
    for page in 1..6 {
        renderer.request(RenderRequest { page, scale: 1.0 });
    }
    renderer.set_priority(vec![4, 5, 2]);
    engine.gate.wait();

    for _ in 0..6 {
        recv(&renderer);
    }
    let pages: Vec<usize> = engine
        .rendered
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(pages, vec![0, 4, 5, 2, 1, 3]);
}

#[test]
fn background_start_renders_requested_page() {
    let renderer = Renderer::start_background(
        Arc::new(HayroEngine),
        fixture(),
        noido_render::Transforms::default(),
        no_waker(),
    );
    renderer.request(RenderRequest {
        page: 1,
        scale: 0.5,
    });
    let rendered = recv(&renderer);
    assert_eq!((rendered.page, rendered.scale), (1, 0.5));
    let image = rendered.result.unwrap();
    assert_eq!((image.width(), image.height()), (421, 298));
}

#[test]
fn background_start_on_garbage_delivers_nothing() {
    let renderer = Renderer::start_background(
        Arc::new(HayroEngine),
        PdfBytes::from(b"nope".to_vec()),
        noido_render::Transforms::default(),
        no_waker(),
    );
    renderer.request(RenderRequest {
        page: 0,
        scale: 1.0,
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(renderer.try_recv().is_none());
}

#[test]
fn renders_to_whole_pixels_without_a_light_edge() {
    // Letter at 1.37 is 838.44 × 1085.04 px; a light last column or row
    // shows as a line between pages drawn without a gap.
    let black = PdfBytes::from(
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/black.pdf"
        ))
        .unwrap(),
    );
    let (renderer, _) = Renderer::start(
        Arc::new(HayroEngine),
        black,
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();
    renderer.request(RenderRequest {
        page: 0,
        scale: 1.37,
    });
    let image = recv(&renderer).result.unwrap();
    assert_eq!((image.width(), image.height()), (838, 1085));
    let (right, bottom) = (image.width() - 1, image.height() - 1);
    assert!(
        image.pixel(right, 500)[0] < 8,
        "{:?}",
        image.pixel(right, 500)
    );
    assert!(
        image.pixel(400, bottom)[0] < 8,
        "{:?}",
        image.pixel(400, bottom)
    );
}

#[test]
fn renderer_reports_turned_sizes_and_stamps() {
    use noido_core::{PageTransform, PageTransforms, Rotation};
    let transforms = noido_render::Transforms::new(PageTransforms::from_records([(
        0,
        PageTransform {
            rotation: Rotation::R90,
            ..PageTransform::default()
        },
    )]));
    let (renderer, info) =
        Renderer::start(Arc::new(HayroEngine), fixture(), transforms, no_waker()).unwrap();
    assert_eq!(
        info.page_sizes[0],
        Size {
            width: 792.0,
            height: 612.0
        }
    );
    renderer.request(RenderRequest {
        page: 0,
        scale: 0.5,
    });
    let rendered = recv(&renderer);
    assert_eq!(rendered.transform.rotation, Rotation::R90);
    let image = rendered.result.unwrap();
    assert!(image.width() > image.height());
}

#[test]
fn start_with_scale_to_fit_reports_fitted_sizes() {
    let bytes = PdfBytes::from(
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/mixed-sizes.pdf"
        ))
        .unwrap(),
    );
    let transforms = noido_render::Transforms::default();
    transforms.set_scale_to_fit(true);
    let (_renderer, info) = Renderer::start(
        Arc::new(HayroEngine),
        bytes.clone(),
        transforms.clone(),
        no_waker(),
    )
    .unwrap();
    let a4 = Size {
        width: 595.0,
        height: 842.0,
    };
    assert_eq!(info.page_sizes, vec![a4; 4]);
    // The shared transforms are the app's to write; opening leaves them.
    assert_eq!(transforms.get(3).scale, 1.0);
    assert!(transforms.scale_to_fit());
    assert_eq!(info.engine_sizes[3].width, 1190.0);
    assert_eq!(info.transforms.get(3).scale, 0.5);
    assert!(info.transforms.scale_to_fit());

    let (_renderer, info) = Renderer::start(
        Arc::new(HayroEngine),
        bytes,
        noido_render::Transforms::default(),
        no_waker(),
    )
    .unwrap();
    assert_eq!(info.page_sizes[3].width, 1190.0);
}
