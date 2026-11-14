//! Throwaway measurements for tiled rendering: shared RenderCache, tiles vs
//! full page, vello_cpu multithreading, tiles over threads.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Affine;
use hayro::{RenderCache, RenderSettings};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};
use vello_cpu::color::palette::css::WHITE;
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};

struct Counting;
static CUR: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CUR.fetch_add(l.size(), Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        CUR.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CUR.fetch_add(n, Relaxed);
        CUR.fetch_sub(l.size(), Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[derive(Clone, Copy)]
struct R {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Same as engine-hayro's render, with the cache and thread count passed in.
fn render<'a>(page: &'a Page<'a>, cache: &RenderCache<'a>, scale: f32, r: R, threads: u16) -> Duration {
    let t = Instant::now();
    let mut ctx = RenderContext::new_with(
        r.w as u16,
        r.h as u16,
        vello_cpu::RenderSettings {
            num_threads: threads,
            ..Default::default()
        },
    );
    let transform = Affine::translate((-f64::from(r.x), -f64::from(r.y)))
        * Affine::scale(f64::from(scale))
        * page.initial_transform(true).to_kurbo();
    hayro::render_into(
        page,
        cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &mut ctx,
        transform,
    );
    ctx.flush();
    let mut pixmap = Pixmap::new(r.w as u16, r.h as u16);
    ctx.render_with(
        &mut pixmap,
        &mut Resources::default(),
        RasterizerSettings {
            target_init: TargetInit::Clear(WHITE),
            ..Default::default()
        },
    );
    std::hint::black_box(&pixmap);
    t.elapsed()
}

fn full(page: &Page, scale: f32) -> R {
    let (w, h) = page.render_dimensions();
    R {
        x: 0,
        y: 0,
        w: (w * scale).ceil() as u32,
        h: (h * scale).ceil() as u32,
    }
}

fn tiles(f: R, k: u32) -> Vec<R> {
    let (tw, th) = (f.w.div_ceil(k), f.h.div_ceil(k));
    let mut v = vec![];
    for row in 0..k {
        for col in 0..k {
            let (x, y) = (col * tw, row * th);
            if x < f.w && y < f.h {
                v.push(R {
                    x,
                    y,
                    w: tw.min(f.w - x),
                    h: th.min(f.h - y),
                });
            }
        }
    }
    v
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

const RUNS: usize = 3;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let scale: f32 = args[2].parse().unwrap();
    let pages: Vec<usize> = args[3].split(',').map(|s| s.parse().unwrap()).collect();
    let mode = args.get(4).map(String::as_str).unwrap_or("page");
    let bytes = Arc::new(std::fs::read(path).unwrap());
    let pdf = Pdf::new(bytes.clone()).unwrap();
    let all = pdf.pages();

    match mode {
        // Per-page experiments.
        "page" => {
            for &p in &pages {
                let page = &all[p];
                let f = full(page, scale);
                println!("== page {p} at scale {scale}: {}x{} px", f.w, f.h);

                let fresh = median((0..RUNS).map(|_| ms(render(page, &RenderCache::new(), scale, f, 0))).collect());
                println!("full, fresh cache          {fresh:8.1} ms");

                let before = CUR.load(Relaxed);
                let cache = RenderCache::new();
                let cold = ms(render(page, &cache, scale, f, 0));
                let grown = CUR.load(Relaxed) as isize - before as isize;
                let warm = median((0..RUNS).map(|_| ms(render(page, &cache, scale, f, 0))).collect());
                println!("full, shared cache         cold {cold:8.1} ms, warm {warm:8.1} ms ({:.0}%), cache {:.1} MB",
                    100.0 * warm / fresh, grown as f64 / 1e6);
                drop(cache);

                for k in [2u32, 3, 4] {
                    let ts = tiles(f, k);
                    let fr = median((0..RUNS).map(|_| ts.iter().map(|&t| ms(render(page, &RenderCache::new(), scale, t, 0))).sum()).collect());
                    let sh = median((0..RUNS).map(|_| {
                        let c = RenderCache::new();
                        ts.iter().map(|&t| ms(render(page, &c, scale, t, 0))).sum()
                    }).collect());
                    let one = median((0..RUNS).map(|_| ms(render(page, &RenderCache::new(), scale, ts[0], 0))).collect());
                    println!("{k}x{k} tiles serial          fresh {fr:8.1} ms ({:.1}x), shared {sh:8.1} ms ({:.1}x); one tile {one:6.1} ms ({:.0}% of full)",
                        fr / fresh, sh / fresh, 100.0 * one / fresh);
                }

                for th in [1u16, 3, 7, 11] {
                    let t = median((0..RUNS).map(|_| ms(render(page, &RenderCache::new(), scale, f, th))).collect());
                    println!("full, vello {th:2} threads     {t:8.1} ms ({:.2}x speed-up)", fresh / t);
                }

                for k in [2u32, 3] {
                    let ts = tiles(f, k);
                    let n = ts.len();
                    let wall = median((0..RUNS).map(|_| {
                        let barrier = Arc::new(Barrier::new(n + 1));
                        let hs: Vec<_> = ts.iter().map(|&t| {
                            let (b, bytes) = (barrier.clone(), bytes.clone());
                            std::thread::spawn(move || {
                                let pdf = Pdf::new(bytes).unwrap();
                                let pages = pdf.pages();
                                let page = &pages[p];
                                b.wait();
                                render(page, &RenderCache::new(), scale, t, 0);
                            })
                        }).collect();
                        barrier.wait();
                        let t0 = Instant::now();
                        for h in hs {
                            h.join().unwrap();
                        }
                        ms(t0.elapsed())
                    }).collect());
                    println!("{k}x{k} tiles on {n:2} threads   {wall:8.1} ms wall ({:.2}x speed-up)", fresh / wall);
                }
            }
        }
        // Run of pages: fresh cache each vs one cache for all.
        "run" => {
            let f = |p: usize| full(&all[p], scale);
            let fr: f64 = pages.iter().map(|&p| ms(render(&all[p], &RenderCache::new(), scale, f(p), 0))).sum();
            let before = CUR.load(Relaxed);
            let c = RenderCache::new();
            let sh: f64 = pages.iter().map(|&p| ms(render(&all[p], &c, scale, f(p), 0))).sum();
            let grown = CUR.load(Relaxed) as isize - before as isize;
            println!("run of {} pages at {scale}: fresh {fr:.0} ms, shared {sh:.0} ms ({:.0}%), cache {:.1} MB",
                pages.len(), 100.0 * sh / fr, grown as f64 / 1e6);
        }
        // One page through zoom steps of 1.25 with one cache: memory growth.
        "zoom" => {
            let p = pages[0];
            let page = &all[p];
            let before = CUR.load(Relaxed);
            let c = RenderCache::new();
            let mut s = 1.0f32;
            while s <= scale {
                let f = full(page, s);
                let t = ms(render(page, &c, s, f, 0));
                println!("scale {s:5.2}: {t:7.1} ms, cache {:.1} MB", (CUR.load(Relaxed) - before) as f64 / 1e6);
                s *= 1.25;
            }
        }
        // Every page in a range, timed once: find the slow ones.
        "scan" => {
            let mut v: Vec<(f64, usize)> = (pages[0]..pages[1].min(all.len()))
                .map(|p| (ms(render(&all[p], &RenderCache::new(), scale, full(&all[p], scale), 0)), p))
                .collect();
            v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            let med = v[v.len() / 2].0;
            println!("median {med:.0} ms; slowest: {:?}", &v[..5.min(v.len())]);
        }
        // What's on the page: content size, ops, XObjects.
        "info" => {
            use hayro::hayro_syntax::content::ops::TypedInstruction;
            for &p in &pages {
                let page = &all[p];
                let t = Instant::now();
                let mut it = page.operations(); let mut n = 0; while it.next().is_some() { n += 1; }
                let ops_t = ms(t.elapsed());
                println!("page {p}: stream {} B, {n} ops (parse {ops_t:.1} ms)", page.page_stream().map_or(0, |s| s.len()));
                let mut it = page.typed_operations();
                while let Some(op) = it.next() {
                    if let TypedInstruction::XObject(x) = op {
                        if let Some(st) = page.resources().get_x_object(x.0) {
                            let t = Instant::now(); let len = st.decoded().map_or(0, |d| d.len()); let dt = ms(t.elapsed()); let w = st.dict().get::<u32>(b"Width".as_slice()); let h = st.dict().get::<u32>(b"Height".as_slice()); let filt = st.dict().get::<hayro::hayro_syntax::object::Name>(b"Filter".as_slice()); println!("  {w:?}x{h:?} filter {filt:?}, decode {dt:.1} ms");
                            println!("  Do {:?}: {:?}, decoded {len} B", x.0, st.dict().get::<hayro::hayro_syntax::object::Name>(b"Subtype".as_slice()));
                        }
                    }
                }
            }
        }
        "loop" => {
            let page = &all[pages[0]];
            let f = full(page, scale);
            for _ in 0..200 { render(page, &RenderCache::new(), scale, f, 0); }
        }
        _ => panic!("mode"),
    }
}
