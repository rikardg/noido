# Slice 1: workspace setup and `noido file.pdf`

Date: 2026-10-25 · Status: draft for review

Companion to the overall design document (`bregne — Design Document.md`, the
older name of the project). That document describes where noido is going; this
one covers only the first slice.

## Intent

Get a working Rust workspace in place and the smallest useful program on top of
it: `noido myfile.pdf` opens a window that shows the document. Everything after
this (key bindings, window controls, zoom, view modes) builds on this slice, in
that order, each as its own slice.

The slice should already follow the architecture of the overall design: logic in
UI-free library crates, the PDF engine behind a trait, egui as a thin frontend.

## Goals

- `cargo build` on a fresh checkout works with nothing but rustup installed. No
  downloads, no C toolchain, no system libraries beyond what eframe needs.
- `noido <path>` opens a resizable window titled with the file name and shows
  all pages in a vertical, continuously scrolling column.
- Pages render off the UI thread; the window stays responsive while they render,
  showing a blank page-sized placeholder until the image arrives.
- Clear error and non-zero exit for: no argument, missing/unreadable file, file
  that is not a PDF or cannot be opened.

## Non-goals (later slices)

- Custom key bindings and the keymap (only what egui's scroll area gives for free).
- Zoom, view modes (single, dual), page navigation controls.
- Tiled rendering, LRU eviction, prefetch priorities.
- Text extraction, search, notes, recolouring.
- PDFium, packaging, CI.

## Decisions made together

- **Toolchain:** rustup (installed). The repo pins the toolchain with
  `rust-toolchain.toml`.
- **PDF engine:** hayro (pure Rust, `hayro` 0.8, Apache-2.0 OR MIT) behind the
  `PdfEngine` trait. PDFium stays a possible second engine for when text, search
  or performance need it.
- **Order of work:** open and view → key bindings → resizable window and other
  controls → zoom → view modes (single, dual, flowing).

## Approaches considered (PDF engine)

| Engine | Trade-off | Outcome |
| --- | --- | --- |
| hayro | Zero setup, single binary everywhere; young API, no performance work yet, text extraction immature. The first slices need rendering only. | **Chosen** |
| PDFium (`pdfium-render`) | Mature, fast, good text; shared library to download and ship, either by a fetch command or a `build.rs` download. | Deferred; trait keeps it open |
| MuPDF (`mupdf` crate) | Fastest, strong text, builds from source; AGPL and single-company governance. | Rejected |

## What was learned about hayro (affects the design)

- `hayro::render_into` takes an arbitrary transform into a render context of any
  size, so rendering a region of a page (a tile) is supported. The engine trait
  can keep `render_tile` from day one even though slice 1 renders whole pages.
- `RenderCache` uses `Rc`/`RefCell` and borrows from the parsed `Pdf`, so it is
  **not `Send`**. Like PDFium, the engine must live on one dedicated render
  thread. Consequence: `Document` is not required to be `Send`; instead the
  render thread opens the document itself from the shared bytes and owns it.
- The crate states that no performance optimisation has been done yet. Rendering
  off the UI thread with cached results matters from the start.

## Design

### Workspace

```
Cargo.toml                 workspace, resolver 3, edition 2024, shared lints
rust-toolchain.toml        pinned stable (1.99) + rustfmt, clippy
crates/
  noido-engine/            PdfEngine / Document traits, geometry and image types
  engine-hayro/            hayro implementation
  noido-render/            render thread, job queue, page image cache
  noido-core/              document session: page layout and viewer state
  frontends/gui-egui/      binary `noido`; eframe + clap
```

`noido-text`, `noido-notes` and the `cli` frontend are created when their
features start, not as empty shells now.

### Engine trait (adjusted from the overall design)

```rust
pub trait PdfEngine: Send + Sync {
    fn open(&self, bytes: Arc<[u8]>) -> Result<Box<dyn Document>, EngineError>;
}

pub trait Document {                       // not Send: lives on the render thread
    fn page_count(&self) -> usize;
    fn page_size(&self, page: usize) -> Size;   // PDF points, after page rotation
    fn render_tile(&self, page: usize, scale: f32, rect: PixelRect) -> Rgba8Image;
}
```

`text` and `outline` are added to the trait in the slices that need them.

### Rendering

- `noido-render` starts one thread that opens the document via the engine and
  receives jobs over a channel: render page N at scale S.
- Results (RGBA images) come back over a channel; the frontend is woken through a
  callback it registers (egui's `request_repaint`), so `noido-render` does not
  depend on egui.
- Slice 1 renders whole pages (one tile covering the page) at fit-to-width scale
  × device pixel ratio. A newer request for the same page replaces a pending one.
- Only pages that are visible, plus one page either side, are requested. Rendered
  images are kept in a simple map; eviction comes with the tile/LRU work.

### Core

`noido-core` holds the open document's metadata (page count, page sizes) and the
layout of the continuous view: page rectangles in a vertical column with a gap,
scaled to the available width. The frontend asks it which pages intersect the
visible area and where to draw them. This is the seed of the viewer state that
zoom and view modes extend later.

### Frontend

- `noido <path>` parsed with clap. Errors are printed before any window opens.
- eframe with the wgpu backend, window titled `<file name> — noido`.
- Central panel with a vertical scroll area; each visible page is drawn as an
  egui texture, or a white placeholder of the right size until its image arrives.
- Textures are uploaded once per rendered image and dropped when the image
  changes or the page scrolls far away.

## Testing

- `engine-hayro`: open a small PDF fixture checked into the repo, check page
  count and sizes, render a page and check the image has the expected size and
  is not blank.
- `noido-core`: unit tests for layout and visible-page queries.
- `noido-render`: open a fixture, submit jobs, receive images.
- The GUI is checked by running it against a few real PDFs.

## Left to the builder

- Exact names of geometry and error types; whether to use `thiserror`/`anyhow`.
- Page gap, background colour, placeholder appearance.
- How the PDF fixture is produced (hand-written minimal PDF is fine).

## My calls (please check)

- Pin the toolchain to an exact stable version rather than plain `stable`.
- Only five crates now; the rest of the planned crates come with their features.
- `noido-core` exists from slice 1 so viewer logic does not land in the egui crate.
- Continuous vertical scroll, fit-to-width, is the default view until view modes
  arrive.
- clap for argument parsing, since the CLI frontend will need subcommands later.
- The overall design document stays where it is under its old name for now.
