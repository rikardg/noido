//! typst as a library: the blocks become typst markup, typst lays out
//! pages, `typst-render` draws them.

use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst_layout::PagedDocument;
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use crate::doc::*;

struct Fonts {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        Fonts {
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

pub struct Typst {
    library: LazyHash<Library>,
    source: Source,
}

impl Typst {
    pub fn new(blocks: &[Block]) -> Self {
        Self {
            library: LazyHash::new(Library::default()),
            source: Source::detached(markup(blocks)),
        }
    }

    /// Replaces the source, as an edit would; typst reuses what it can.
    pub fn edit(&mut self, blocks: &[Block]) {
        self.source.replace(&markup(blocks));
    }

    pub fn compile(&self) -> PagedDocument {
        let warned = typst::compile::<PagedDocument>(self);
        match warned.output {
            Ok(doc) => doc,
            Err(errors) => {
                for e in errors {
                    eprintln!("typst: {}", e.message);
                }
                panic!("typst failed");
            }
        }
    }
}

pub fn render(doc: &PagedDocument, pixel_per_pt: f32) -> Vec<tiny_skia::Pixmap> {
    let opts = typst_render::RenderOptions {
        pixel_per_pt: (pixel_per_pt as f64).into(),
        ..Default::default()
    };
    doc.pages()
        .iter()
        .map(|page| typst_render::render(page, &opts))
        .collect()
}

impl World for Typst {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &fonts().book
    }
    fn main(&self) -> FileId {
        self.source.id()
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(id.vpath().get_without_slash().into()))
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().get_without_slash().into()))
    }
    fn font(&self, index: usize) -> Option<Font> {
        fonts().fonts.get(index).cloned()
    }
    fn today(&self, _: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}

fn markup(blocks: &[Block]) -> String {
    // The box of a line is one em (0.8 above the baseline, 0.2 below), so
    // leading = LEADING - FONT_SIZE puts baselines LEADING apart, and the
    // bottom margin leaves room for exactly LINES_PER_PAGE lines.
    let lines = LINES_PER_PAGE as f32;
    let body = lines * FONT_SIZE + (lines - 1.0) * (LEADING - FONT_SIZE);
    let bottom = PAGE_H - MARGIN_TOP - body - 0.5;
    let mut out = format!(
        "#set page(width: {PAGE_W}pt, height: {PAGE_H}pt, \
         margin: (x: {MARGIN_X}pt, top: {MARGIN_TOP}pt, bottom: {bottom}pt))\n\
         #set text(font: \"Libertinus Serif\", size: {FONT_SIZE}pt, lang: \"en\", \
         top-edge: 0.8em, bottom-edge: -0.2em)\n\
         #set par(justify: true, leading: {lead}pt, spacing: {lead}pt, \
         first-line-indent: {INDENT}pt)\n\
         #show heading: set text(size: {HEADING_SIZE}pt)\n\
         #show heading: set block(above: {above}pt, below: {below}pt)\n\n",
        lead = LEADING - FONT_SIZE,
        above = 2.0 * LEADING,
        below = LEADING,
    );
    for block in blocks {
        match block {
            Block::Heading(h) => {
                out.push_str("#heading(");
                out.push_str(&string(h));
                out.push_str(")\n\n");
            }
            Block::Para(p) => {
                let mut at = 0;
                for r in &p.italic {
                    push_text(&mut out, &p.text[at..r.start], false);
                    push_text(&mut out, &p.text[r.clone()], true);
                    at = r.end;
                }
                push_text(&mut out, &p.text[at..], false);
                out.push_str("\n\n");
            }
        }
    }
    out
}

fn push_text(out: &mut String, text: &str, italic: bool) {
    if text.is_empty() {
        return;
    }
    if italic {
        out.push_str("#emph(");
        out.push_str(&string(text));
        out.push(')');
    } else {
        out.push('#');
        out.push_str(&string(text));
    }
}

fn string(text: &str) -> String {
    let mut s = String::from("\"");
    for c in text.chars() {
        if c == '"' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}
