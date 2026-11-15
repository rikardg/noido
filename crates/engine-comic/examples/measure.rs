//! Measures a real comic archive: open, hash, and a few renders. Not a test;
//! the path is the caller's.

use std::time::Instant;

use engine_comic::ComicEngine;
use noido_engine::{PdfBytes, PdfEngine};

fn main() {
    let path = std::env::args().nth(1).expect("a cbz path");
    let path = std::path::Path::new(&path);
    let dir = path.parent().unwrap();

    let start = Instant::now();
    let bytes = PdfBytes::mapped_in(path, dir)
        .unwrap_or_else(|| PdfBytes::from(std::fs::read(path).unwrap()));
    println!("load (mapped {}): {:?}", bytes.is_mapped(), start.elapsed());

    let start = Instant::now();
    let key = noido_state::DocKey::sampled(&bytes, path);
    println!("sampled state key: {:?} ({})", start.elapsed(), key.hex());

    for attempt in 1..=3 {
        let start = Instant::now();
        let hash = blake3::hash(&bytes);
        println!(
            "hash {} MB ({attempt}): {:?} ({})",
            bytes.len() / 1_000_000,
            start.elapsed(),
            hash.to_hex()
        );
    }

    let start = Instant::now();
    let document = ComicEngine.open(bytes).unwrap();
    println!(
        "open: {:?}, {} pages",
        start.elapsed(),
        document.page_count()
    );

    for page in [0, 1, 2, 100, 200, 405] {
        let size = document.page_size(page).unwrap();
        let start = Instant::now();
        let native = document.render_page(page, 100_000, 100_000).unwrap();
        let native_time = start.elapsed();
        let start = Instant::now();
        let fit = document.render_page(page, 1200, 1650).unwrap();
        let fit_time = start.elapsed();
        println!(
            "page {}: {:.0}x{:.0} pt, native {}x{} in {:?}, fit {}x{} in {:?}",
            page + 1,
            size.width,
            size.height,
            native.width(),
            native.height(),
            native_time,
            fit.width(),
            fit.height(),
            fit_time,
        );
        if page == 2 || page == 100 {
            let out = format!("/tmp/cbz-page-{}.png", page + 1);
            image::save_buffer(
                &out,
                fit.pixels(),
                fit.width(),
                fit.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
            println!("  wrote {out}");
        }
    }
}
