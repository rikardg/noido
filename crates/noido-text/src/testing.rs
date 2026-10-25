//! Glyphs laid out by hand for tests.

use noido_engine::{Glyph, Point, Quad};

/// A glyph with `text` whose baseline starts at (`x`, `y`) and runs along
/// `dir` (a unit vector), `advance` ems long.
pub fn glyph_along(text: &str, x: f32, y: f32, size: f32, advance: f32, dir: (f32, f32)) -> Glyph {
    // Up is the writing direction turned a quarter anticlockwise on screen.
    let (ux, uy) = (dir.1, -dir.0);
    let at = |along: f32, up: f32| Point {
        x: x + dir.0 * along + ux * up,
        y: y + dir.1 * along + uy * up,
    };
    let length = advance * size;
    Glyph {
        text: text.to_owned(),
        origin: Point { x, y },
        size,
        quad: Quad {
            corners: [
                at(0.0, -0.2 * size),
                at(length, -0.2 * size),
                at(length, 0.8 * size),
                at(0.0, 0.8 * size),
            ],
        },
        invisible: false,
    }
}

/// A left-to-right glyph half an em wide.
pub fn glyph(text: &str, x: f32, y: f32, size: f32) -> Glyph {
    glyph_along(text, x, y, size, 0.5, (1.0, 0.0))
}

/// One glyph per character of `text`, left to right from (`x`, `y`), each
/// half an em wide; spaces are glyphs too.
pub fn line(text: &str, x: f32, y: f32, size: f32) -> Vec<Glyph> {
    text.chars()
        .enumerate()
        .map(|(i, c)| glyph(&c.to_string(), x + i as f32 * 0.5 * size, y, size))
        .collect()
}

/// Lines of `text` 14 pt apart at 10 pt, starting at (72, 100).
pub fn page(lines: &[&str]) -> Vec<Glyph> {
    lines
        .iter()
        .enumerate()
        .flat_map(|(i, text)| line(text, 72.0, 100.0 + i as f32 * 14.0, 10.0))
        .collect()
}
