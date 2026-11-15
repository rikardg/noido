//! The input: a tiny markdown subset. `# heading`, paragraphs separated by
//! blank lines, `_italic_`.

pub enum Block {
    Heading(String),
    Para(Para),
}

/// A paragraph's text and its italic byte ranges.
pub struct Para {
    pub text: String,
    pub italic: Vec<std::ops::Range<usize>>,
}

pub fn parse(src: &str) -> Vec<Block> {
    src.split("\n\n")
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(|b| match b.strip_prefix("# ") {
            Some(h) => Block::Heading(h.to_string()),
            None => Block::Para(para(&b.replace('\n', " "))),
        })
        .collect()
}

fn para(src: &str) -> Para {
    let mut text = String::new();
    let mut italic = Vec::new();
    let mut start = None;
    for c in src.chars() {
        if c == '_' {
            match start.take() {
                Some(s) => italic.push(s..text.len()),
                None => start = Some(text.len()),
            }
        } else {
            text.push(c);
        }
    }
    Para { text, italic }
}

/// The page: a trade paperback, 5.5 × 8.5 in.
pub const PAGE_W: f32 = 396.0;
pub const PAGE_H: f32 = 612.0;
pub const MARGIN_X: f32 = 54.0;
pub const MARGIN_TOP: f32 = 60.0;
pub const FONT_SIZE: f32 = 11.0;
/// Baseline to baseline.
pub const LEADING: f32 = 14.0;
pub const LINES_PER_PAGE: usize = 34;
pub const INDENT: f32 = 1.2 * FONT_SIZE;
pub const HEADING_SIZE: f32 = 16.0;
pub const MEASURE: f32 = PAGE_W - 2.0 * MARGIN_X;
