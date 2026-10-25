use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noido_core::Direction;
use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};
use noido_render::Thumbnailer;

/// Letter pages that log each render; with a gate, each render waits for a
/// token (or for the gate's sender to be dropped).
struct LogEngine {
    pages: usize,
    log: Arc<Mutex<Vec<usize>>>,
    gate: Option<Arc<Mutex<Receiver<()>>>>,
    fail_open: bool,
}

struct LogDocument {
    pages: usize,
    log: Arc<Mutex<Vec<usize>>>,
    gate: Option<Arc<Mutex<Receiver<()>>>>,
}

impl PdfEngine for LogEngine {
    fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        if self.fail_open {
            return Err(EngineError::Open("broken".into()));
        }
        Ok(Box::new(LogDocument {
            pages: self.pages,
            log: self.log.clone(),
            gate: self.gate.clone(),
        }))
    }
}

impl Document for LogDocument {
    fn page_count(&self) -> usize {
        self.pages
    }
    fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 612.0,
            height: 792.0,
        })
    }
    fn render_tile(&self, page: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
        self.log.lock().unwrap().push(page);
        if let Some(gate) = &self.gate {
            let _ = gate.lock().unwrap().recv();
        }
        Ok(Rgba8Image::from_premultiplied(1, 1, vec![255; 4]).unwrap())
    }
}

struct Setup {
    thumbnailer: Thumbnailer,
    log: Arc<Mutex<Vec<usize>>>,
    gate: Option<Sender<()>>,
}

fn start(pages: usize, gated: bool) -> Setup {
    let log = Arc::new(Mutex::new(vec![]));
    let (gate_tx, gate_rx) = mpsc::channel();
    let engine = LogEngine {
        pages,
        log: log.clone(),
        gate: gated.then(|| Arc::new(Mutex::new(gate_rx))),
        fail_open: false,
    };
    let thumbnailer = Thumbnailer::start(Arc::new(engine), Arc::new(vec![]), Box::new(|| {}));
    Setup {
        thumbnailer,
        log,
        gate: gated.then_some(gate_tx),
    }
}

/// Waits until `count` renders were logged; returns the log.
fn wait_for(log: &Mutex<Vec<usize>>, count: usize) -> Vec<usize> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = log.lock().unwrap().clone();
        if current.len() >= count {
            return current;
        }
        assert!(Instant::now() < deadline, "only {current:?} within 5 s");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn renders_nearest_first_from_focus() {
    let s = start(20, false);
    s.thumbnailer.set_focus(10, Direction::Forward);
    let log = wait_for(&s.log, 20);
    assert_eq!(log[..4], [10, 11, 12, 9]);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received = vec![];
    while received.len() < 20 && Instant::now() < deadline {
        match s.thumbnailer.try_recv() {
            Some(thumbnail) => {
                assert!(thumbnail.result.is_ok());
                received.push(thumbnail.page);
            }
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    received.sort();
    assert_eq!(received, (0..20).collect::<Vec<_>>());
}

#[test]
fn new_focus_retargets() {
    let s = start(400, true);
    let gate = s.gate.as_ref().unwrap();
    s.thumbnailer.set_focus(0, Direction::Forward);
    wait_for(&s.log, 1);
    s.thumbnailer.set_focus(300, Direction::Forward);
    gate.send(()).unwrap();
    let log = wait_for(&s.log, 2);
    assert_eq!(log[..2], [0, 300]);
}

#[test]
fn delivered_pages_not_rendered_again() {
    let s = start(20, false);
    s.thumbnailer.set_focus(5, Direction::Forward);
    wait_for(&s.log, 20);
    s.thumbnailer.set_focus(6, Direction::Backward);
    std::thread::sleep(Duration::from_millis(100));
    let mut log = s.log.lock().unwrap().clone();
    assert_eq!(log.len(), 20);
    log.sort();
    log.dedup();
    assert_eq!(log.len(), 20);
}

#[test]
fn evicted_pages_render_again() {
    let s = start(400, false);
    s.thumbnailer.set_focus(0, Direction::Forward);
    wait_for(&s.log, 100);
    s.thumbnailer.set_focus(399, Direction::Forward);
    wait_for(&s.log, 200);
    s.thumbnailer.set_focus(0, Direction::Forward);
    let log = wait_for(&s.log, 300);
    assert_eq!(log.iter().filter(|&&page| page == 0).count(), 2);
}

#[test]
fn open_failure_stops_quietly() {
    let engine = LogEngine {
        pages: 3,
        log: Arc::new(Mutex::new(vec![])),
        gate: None,
        fail_open: true,
    };
    let thumbnailer = Thumbnailer::start(Arc::new(engine), Arc::new(vec![]), Box::new(|| {}));
    thumbnailer.set_focus(0, Direction::Forward);
    std::thread::sleep(Duration::from_millis(50));
    assert!(thumbnailer.try_recv().is_none());
}
