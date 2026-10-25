//! Times text search on a real file. Usage: `search_text <file.pdf> <query>…`.
//! Each query searches from the first page; the first one also extracts
//! every page's text.

use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_hayro::HayroEngine;
use noido_render::{SearchUpdate, TextSearcher};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: search_text <file.pdf> <query>…");
    let bytes = Arc::new(std::fs::read(path).expect("read file"));
    let mut searcher = TextSearcher::start(Arc::new(HayroEngine), bytes, Box::new(|| {}));
    for query in &args[2..] {
        let start = Instant::now();
        let generation = searcher.search(query, 0);
        let mut hits = Vec::new();
        let cut_short = loop {
            match searcher.try_recv() {
                Some(SearchUpdate::Page {
                    generation: g,
                    page,
                    hits: found,
                }) if g == generation => hits.extend(found.into_iter().map(|h| (page, h))),
                Some(SearchUpdate::Done {
                    generation: g,
                    cut_short,
                }) if g == generation => break cut_short,
                Some(_) => {}
                None => std::thread::sleep(Duration::from_micros(200)),
            }
        };
        let more = if cut_short { "+" } else { "" };
        println!(
            "{query:?}: {}{more} hits in {:.0} ms",
            hits.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        for (page, hit) in hits.iter().take(3) {
            let context: Vec<char> = hit.context.text.chars().collect();
            let bold = &hit.context.bold;
            let part = |r: std::ops::Range<usize>| context[r].iter().collect::<String>();
            println!(
                "  p. {}: {}[{}]{}",
                page + 1,
                part(0..bold.start),
                part(bold.clone()),
                part(bold.end..context.len())
            );
        }
    }
}
