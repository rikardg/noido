use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use engine_hayro::HayroEngine;
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};
use noido_render::{RenderRequest, RenderedPage, Renderer};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/two-pages.pdf");

fn fixture() -> PdfBytes {
    Arc::new(std::fs::read(FIXTURE).unwrap())
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
    let (_renderer, info) = Renderer::start(Arc::new(HayroEngine), fixture(), no_waker()).unwrap();
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
        Arc::new(b"nope".to_vec()),
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
    let (renderer, _) = Renderer::start(Arc::new(HayroEngine), fixture(), waker).unwrap();
    renderer.request(RenderRequest {
        page: 0,
        scale: 0.5,
    });
    let rendered = recv(&renderer);
    assert_eq!(rendered.page, 0);
    let image = rendered.result.unwrap();
    assert_eq!((image.width(), image.height()), (306, 396));
    assert!(wakes.load(Ordering::SeqCst) >= 1);
}

#[test]
fn out_of_range_page_reports_error() {
    let (renderer, _) = Renderer::start(Arc::new(HayroEngine), fixture(), no_waker()).unwrap();
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

#[test]
fn coalesces_to_latest_per_page() {
    let engine = GateEngine {
        entered: Arc::new(AtomicBool::new(false)),
        gate: Arc::new(Barrier::new(2)),
        rendered: Arc::new(Mutex::new(Vec::new())),
    };
    let (renderer, _) = Renderer::start(Arc::new(engine.clone()), fixture(), no_waker()).unwrap();

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
    let (renderer, _) = Renderer::start(Arc::new(engine.clone()), fixture(), no_waker()).unwrap();

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
fn renders_in_priority_order_then_the_rest() {
    let engine = GateEngine {
        entered: Arc::new(AtomicBool::new(false)),
        gate: Arc::new(Barrier::new(2)),
        rendered: Arc::new(Mutex::new(Vec::new())),
    };
    let (renderer, _) = Renderer::start(Arc::new(engine.clone()), fixture(), no_waker()).unwrap();

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
    let renderer = Renderer::start_background(Arc::new(HayroEngine), fixture(), no_waker());
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
        Arc::new(b"nope".to_vec()),
        no_waker(),
    );
    renderer.request(RenderRequest {
        page: 0,
        scale: 1.0,
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(renderer.try_recv().is_none());
}
