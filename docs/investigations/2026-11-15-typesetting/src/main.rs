//! Typesets the same text three ways and writes the pages as PNGs:
//! typst; parley with its own greedy breaking; parley shaping with
//! Knuth–Plass breaking and hypher hyphenation.
//!
//! `typeset <fonts-dir> <out-dir> <input.md>...`

mod doc;
#[cfg(feature = "parley")]
mod kp;
#[cfg(feature = "parley")]
mod parley_backend;
#[cfg(feature = "parley")]
mod raster;
#[cfg(feature = "typst")]
mod typst_backend;

use std::path::Path;
use std::time::Instant;

const SCALE: f32 = 2.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fonts_dir = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    std::fs::create_dir_all(out).unwrap();
    let _ = fonts_dir;
    for input in &args[3..] {
        let name = Path::new(input).file_stem().unwrap().to_str().unwrap();
        let src = std::fs::read_to_string(input).unwrap();
        let blocks = doc::parse(&src);
        let mut firsts: Vec<(&str, Vec<tiny_skia::Pixmap>)> = Vec::new();

        #[cfg(feature = "typst")]
        {
            let t = Instant::now();
            let mut world = typst_backend::Typst::new(&blocks);
            let document = world.compile();
            let cold = t.elapsed();
            // An edit in the middle, as when iterating on an essay.
            let mid = src.len() / 2;
            let edited = format!("{}{}", &src[..mid], src[mid..].replacen(" the ", " that ", 1));
            world.edit(&doc::parse(&edited));
            let t = Instant::now();
            let _ = world.compile();
            let warm = t.elapsed();
            world.edit(&blocks);
            let t = Instant::now();
            let pages = typst_backend::render(&document, SCALE);
            let raster = t.elapsed();
            println!(
                "{name} typst: {} pages, layout {:.0} ms cold, {:.0} ms after an edit, render {:.0} ms",
                pages.len(),
                cold.as_secs_f64() * 1e3,
                warm.as_secs_f64() * 1e3,
                raster.as_secs_f64() * 1e3,
            );
            save(out, name, "typst", &pages);
            firsts.push(("typst", pages));
        }

        #[cfg(feature = "parley")]
        {
            let fonts: Vec<Vec<u8>> = ["Regular", "Italic", "Bold", "BoldItalic"]
                .iter()
                .map(|s| std::fs::read(fonts_dir.join(format!("LibertinusSerif-{s}.otf"))).unwrap())
                .collect();
            let mut p = parley_backend::Parley::new(&fonts);
            for (label, breaking) in [
                ("greedy", parley_backend::Breaking::Greedy),
                ("knuth-plass", parley_backend::Breaking::KnuthPlass),
            ] {
                let t = Instant::now();
                let sets = p.set(&blocks, breaking);
                let paged = parley_backend::paginate(&sets);
                let layout = t.elapsed();
                let t = Instant::now();
                let pages = parley_backend::render(&paged, SCALE);
                let raster = t.elapsed();
                let lines: Vec<_> = sets
                    .iter()
                    .filter_map(|s| match s {
                        parley_backend::Set::Para(l) => Some(l),
                        _ => None,
                    })
                    .flatten()
                    .filter(|l| !l.last)
                    .collect();
                let n = lines.len() as f32;
                let loose = lines.iter().filter(|l| l.ratio > 1.0).count();
                let very = lines.iter().filter(|l| l.ratio > 2.0).count();
                let tight = lines.iter().filter(|l| l.ratio < -0.5).count();
                let mean = lines.iter().map(|l| l.ratio.abs()).sum::<f32>() / n;
                let max = lines.iter().map(|l| l.ratio).fold(f32::MIN, f32::max);
                let hyph = lines.iter().filter(|l| l.hyphenated).count();
                println!(
                    "{name} {label}: {} pages, layout {:.0} ms, render {:.0} ms; \
                     {} justified lines: mean |ratio| {mean:.2}, max {max:.2}, \
                     loose (>1) {loose}, very loose (>2) {very}, tight (<-0.5) {tight}, hyphenated {hyph}",
                    pages.len(),
                    layout.as_secs_f64() * 1e3,
                    raster.as_secs_f64() * 1e3,
                    lines.len(),
                );
                save(out, name, label, &pages);
                firsts.push((label, pages));
            }
        }

        // Side by side, for each of the first pages.
        let count = firsts.iter().map(|(_, p)| p.len()).min().unwrap_or(0).min(4);
        for page in 0..count {
            let w: u32 = firsts.iter().map(|(_, p)| p[page].width()).sum::<u32>() + 20 * (firsts.len() as u32 - 1);
            let h = firsts[0].1[page].height();
            let mut sheet = tiny_skia::Pixmap::new(w, h).unwrap();
            sheet.fill(tiny_skia::Color::from_rgba8(180, 180, 180, 255));
            let mut x = 0;
            for (_, p) in &firsts {
                sheet.draw_pixmap(x, 0, p[page].as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
                x += p[page].width() as i32 + 20;
            }
            let labels: Vec<&str> = firsts.iter().map(|(l, _)| *l).collect();
            sheet
                .save_png(out.join(format!("{name}-compare-{}-p{}.png", labels.join("-vs-"), page + 1)))
                .unwrap();
        }
    }
}

fn save(out: &Path, name: &str, label: &str, pages: &[tiny_skia::Pixmap]) {
    for (i, p) in pages.iter().enumerate() {
        p.save_png(out.join(format!("{name}-{label}-{:02}.png", i + 1))).unwrap();
    }
}
