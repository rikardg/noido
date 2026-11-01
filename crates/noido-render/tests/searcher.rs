use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use noido_engine::{
    Document, EngineError, Glyph, PdfBytes, PdfEngine, PixelRect, Point, Quad, Rgba8Image, Size,
};
use noido_render::{MAX_HITS, SearchUpdate, TextSearcher};

/// A document whose pages each hold one line of text.
#[derive(Clone)]
struct TextEngine {
    pages: Vec<String>,
    /// A page whose text fails.
    failing: Option<usize>,
    /// How often `text` was called.
    calls: Arc<AtomicUsize>,
}

impl TextEngine {
    fn new(pages: &[&str]) -> TextEngine {
        TextEngine {
            pages: pages.iter().map(|&p| p.to_owned()).collect(),
            failing: None,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl PdfEngine for TextEngine {
    fn open(&self, _: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        Ok(Box::new(self.clone()))
    }
}

impl Document for TextEngine {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, _: usize) -> Result<Size, EngineError> {
        Ok(Size {
            width: 612.0,
            height: 792.0,
        })
    }

    fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
        Err(EngineError::Render("no pixels here".into()))
    }

    fn text(&self, page: usize) -> Result<Vec<Glyph>, EngineError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.failing == Some(page) {
            return Err(EngineError::Render("broken text".into()));
        }
        // One glyph per character, 5 pt wide, at 10 pt.
        Ok(self.pages[page]
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let x = 72.0 + i as f32 * 5.0;
                let p = |x, y| Point { x, y };
                Glyph {
                    text: c.to_string(),
                    origin: p(x, 100.0),
                    size: 10.0,
                    quad: Quad {
                        corners: [p(x, 102.0), p(x + 5.0, 102.0), p(x + 5.0, 92.0), p(x, 92.0)],
                    },
                    invisible: false,
                }
            })
            .collect())
    }
}

fn start(engine: &TextEngine) -> TextSearcher {
    TextSearcher::start(
        Arc::new(engine.clone()),
        Arc::new(Vec::new()),
        Box::new(|| {}),
    )
}

/// Updates until the `Done` of `generation`, skipping other generations'.
fn until_done(searcher: &TextSearcher, generation: u64) -> Vec<SearchUpdate> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut updates = Vec::new();
    loop {
        match searcher.try_recv() {
            Some(update) => {
                let done =
                    matches!(update, SearchUpdate::Done { generation: g, .. } if g == generation);
                updates.push(update);
                if done {
                    return updates;
                }
            }
            None => {
                assert!(Instant::now() < deadline, "no Done within 5 s: {updates:?}");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

/// The pages searched, in order, and the hits on each.
fn pages(updates: &[SearchUpdate]) -> Vec<(usize, usize)> {
    updates
        .iter()
        .filter_map(|u| match u {
            SearchUpdate::Page { page, hits, .. } => Some((*page, hits.len())),
            SearchUpdate::Done { .. } => None,
        })
        .collect()
}

fn wait_for(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "condition not met within 5 s");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn nothing_extracted_until_asked() {
    let engine = TextEngine::new(&["a", "b", "c"]);
    let _searcher = start(&engine);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(engine.calls(), 0);
}

#[test]
fn extract_reads_every_page_in_background() {
    let engine = TextEngine::new(&["a", "b", "c"]);
    let searcher = start(&engine);
    searcher.extract();
    wait_for(|| engine.calls() == 3);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(engine.calls(), 3);
}

#[test]
fn search_starts_at_from_page_and_wraps() {
    let engine = TextEngine::new(&["a", "b", "c", "d", "e"]);
    let mut searcher = start(&engine);
    let generation = searcher.search("x", 3);
    let updates = until_done(&searcher, generation);
    assert_eq!(
        pages(&updates).iter().map(|&(p, _)| p).collect::<Vec<_>>(),
        [3, 4, 0, 1, 2]
    );
    assert!(matches!(
        updates.last(),
        Some(SearchUpdate::Done {
            cut_short: false,
            ..
        })
    ));
}

#[test]
fn search_finds_hits_per_page() {
    let engine = TextEngine::new(&["dog", "a cat", "dog", "cat and cat", "dog"]);
    let mut searcher = start(&engine);
    let generation = searcher.search("cat", 0);
    let updates = until_done(&searcher, generation);
    assert_eq!(pages(&updates), [(0, 0), (1, 1), (2, 0), (3, 2), (4, 0)]);
    let SearchUpdate::Page { hits, .. } = &updates[1] else {
        panic!("{:?}", updates[1]);
    };
    assert_eq!(hits[0].context.text, "a cat");
    assert_eq!(hits[0].rects.len(), 1);
}

#[test]
fn newer_search_drops_older() {
    let pages: Vec<String> = (0..200).map(|i| format!("page {i} cat")).collect();
    let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
    let engine = TextEngine::new(&pages);
    let mut searcher = start(&engine);
    let first = searcher.search("cat", 0);
    let second = searcher.search("page", 0);
    assert!(second > first);
    let updates = until_done(&searcher, second);
    let seen_second = updates
        .iter()
        .position(|u| matches!(u, SearchUpdate::Page { generation, .. } if *generation == second))
        .unwrap();
    for update in &updates[seen_second..] {
        let generation = match update {
            SearchUpdate::Page { generation, .. } | SearchUpdate::Done { generation, .. } => {
                *generation
            }
        };
        assert_eq!(generation, second);
    }
    assert_eq!(pages_of(&updates, second), 200);
}

fn pages_of(updates: &[SearchUpdate], generation: u64) -> usize {
    updates
        .iter()
        .filter(|u| matches!(u, SearchUpdate::Page { generation: g, .. } if *g == generation))
        .count()
}

#[test]
fn blank_query_sends_nothing() {
    let engine = TextEngine::new(&["a b c"]);
    let mut searcher = start(&engine);
    searcher.search("  ", 0);
    std::thread::sleep(Duration::from_millis(100));
    assert!(searcher.try_recv().is_none());
}

#[test]
fn blank_query_cancels_running_search() {
    let pages: Vec<String> = (0..2000).map(|i| format!("page {i}")).collect();
    let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
    let engine = TextEngine::new(&pages);
    let mut searcher = start(&engine);
    searcher.search("page", 0);
    searcher.search("", 0);
    std::thread::sleep(Duration::from_millis(200));
    while searcher.try_recv().is_some() {}
    std::thread::sleep(Duration::from_millis(50));
    assert!(searcher.try_recv().is_none(), "search still running");
}

#[test]
fn failing_page_has_no_hits() {
    let mut engine = TextEngine::new(&["cat", "cat", "cat"]);
    engine.failing = Some(1);
    let mut searcher = start(&engine);
    let generation = searcher.search("cat", 0);
    let updates = until_done(&searcher, generation);
    assert_eq!(pages(&updates), [(0, 1), (1, 0), (2, 1)]);
}

#[test]
fn cap_cuts_short_mid_page() {
    let page = "a ".repeat(3000);
    let engine = TextEngine::new(&[&page, &page, &page]);
    let mut searcher = start(&engine);
    let generation = searcher.search("a", 0);
    let updates = until_done(&searcher, generation);
    let total: usize = pages(&updates).iter().map(|&(_, n)| n).sum();
    assert_eq!(total, MAX_HITS);
    assert_eq!(pages(&updates).len(), 2);
    assert!(matches!(
        updates.last(),
        Some(SearchUpdate::Done {
            cut_short: true,
            ..
        })
    ));
}

#[test]
fn from_page_past_end_clamps() {
    let engine = TextEngine::new(&["a", "b", "c"]);
    let mut searcher = start(&engine);
    let generation = searcher.search("b", 10);
    let updates = until_done(&searcher, generation);
    assert_eq!(pages(&updates), [(2, 0), (0, 0), (1, 1)]);
}

#[test]
fn empty_document_is_done_at_once() {
    let engine = TextEngine::new(&[]);
    let mut searcher = start(&engine);
    let generation = searcher.search("a", 0);
    let updates = until_done(&searcher, generation);
    assert_eq!(updates.len(), 1);
}

#[test]
fn text_cached_across_searches() {
    let engine = TextEngine::new(&["a", "b", "c"]);
    let mut searcher = start(&engine);
    let generation = searcher.search("a", 0);
    until_done(&searcher, generation);
    let generation = searcher.search("b", 1);
    until_done(&searcher, generation);
    assert_eq!(engine.calls(), 3);
}

#[test]
fn wakes_per_update() {
    let engine = TextEngine::new(&["a", "b", "c"]);
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let mut searcher = TextSearcher::start(
        Arc::new(engine.clone()),
        Arc::new(Vec::new()),
        Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let generation = searcher.search("a", 0);
    let updates = until_done(&searcher, generation);
    wait_for(|| wakes.load(Ordering::SeqCst) >= updates.len());
    assert_eq!(wakes.load(Ordering::SeqCst), updates.len());
}
