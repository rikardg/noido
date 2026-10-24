# Slice 1: Workspace and `noido file.pdf` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Cargo workspace whose `noido <path>` binary opens a resizable egui window showing every page of a PDF in a continuously scrolling, fit-to-width column, rendered off the UI thread by hayro.

**Architecture:** Library crates hold all logic: `noido-engine` (traits and types), `engine-hayro` (hayro implementation), `noido-render` (one render thread that owns the document), `noido-core` (layout and render bookkeeping). `gui-egui` is a thin frontend that turns RGBA buffers into textures and draws them where `noido-core` says.

**Tech Stack:** Rust 1.99.1 (rustup, edition 2024, resolver 3), hayro 0.8, eframe/egui 0.37 (wgpu backend, the eframe default), clap 4, thiserror 2.

**Spec:** `docs/superpowers/specs/2026-10-25-workspace-and-viewer-design.md`

## Global Constraints

- `cargo build` on a fresh checkout needs only rustup: no downloads in build scripts, no C toolchain, no PDFium.
- `rust-toolchain.toml` pins `channel = "1.99.1"` with components `rustfmt`, `clippy`.
- Workspace `Cargo.toml`: `resolver = "3"`, `edition = "2024"` via `[workspace.package]`, shared `[workspace.dependencies]` and `[workspace.lints]` (`clippy::all = "warn"`, `rust.unsafe_code = "forbid"`). Every crate uses `lints.workspace = true`.
- Library crates never depend on egui/eframe. Only `gui-egui` does.
- Pixel buffers crossing crate boundaries are premultiplied RGBA8, row-major, no padding.
- Engine documents are not `Send`; they are created and used only on the render thread.
- Window title: `<file name> — noido`. Binary name: `noido`.
- Errors before the window opens go to stderr as `noido: <message>` with exit code 1.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass at the end of every task.

## Review Focus

1. **Very tall or very large pages** (posters, long receipts): the rendered image must never exceed 8192 px on its longest side (GPU texture limits, hayro's `u16` canvas). Expect a softer page, not a crash. → `render_scale` tests in Task 3; `TooLarge` test in Task 2.
2. **A page that fails to render**: it keeps showing its placeholder and is not requested again every frame. → `RenderTracker` failure test in Task 3.
3. **Dragging the window edge**: the scale changes every frame. Renders must coalesce to the latest request per page, and the old image stays on screen, stretched, until the new one arrives. → coalescing test in Task 4; tracker "keeps old image" test in Task 3.
4. **Degenerate documents** (zero pages, a page of zero width): there must be no panic or division by zero in layout. → layout tests in Task 3.
5. **Bad paths** (missing file, a directory, an empty file, a non-PDF): there must be a clear message and exit 1 before any window opens. → `load_document` tests in Task 5.

---

## File Structure

```
Cargo.toml                                   workspace
rust-toolchain.toml
fixtures/make_fixtures.py                    generates the test PDFs (committed with its output)
fixtures/two-pages.pdf
crates/noido-engine/src/lib.rs               re-exports
crates/noido-engine/src/types.rs             Size, PixelRect, Rgba8Image
crates/noido-engine/src/engine.rs            PdfEngine, Document, EngineError
crates/engine-hayro/src/lib.rs               HayroEngine, HayroDocument
crates/engine-hayro/tests/render.rs
crates/noido-core/src/lib.rs
crates/noido-core/src/layout.rs              Layout, PageRect, render_scale
crates/noido-core/src/tracker.rs             RenderTracker
crates/noido-render/src/lib.rs               Renderer, RenderRequest, RenderedPage, DocumentInfo
crates/noido-render/tests/renderer.rs
crates/frontends/gui-egui/src/main.rs        arg parsing, error exit, eframe startup
crates/frontends/gui-egui/src/load.rs        load_document
crates/frontends/gui-egui/src/app.rs         NoidoApp (eframe::App)
```

---

### Task 1: Workspace, toolchain, engine types, fixture

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `crates/noido-engine/{Cargo.toml,src/lib.rs,src/types.rs,src/engine.rs}`, `fixtures/make_fixtures.py`, `fixtures/two-pages.pdf`

**Interfaces:**
- Produces (crate `noido-engine`):
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Size { pub width: f32, pub height: f32 }`: PDF points.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PixelRect { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }`: pixel space of a page rendered at a given scale.
  - `#[derive(Clone, Debug)] pub struct Rgba8Image { width: u32, height: u32, pixels: Vec<u8> }` with `pub fn from_premultiplied(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self>` (None unless `pixels.len() == width*height*4`), `width()`, `height()`, `pixels() -> &[u8]`, `pixel(x, y) -> [u8; 4]`.
  - `#[derive(Debug, thiserror::Error)] pub enum EngineError { #[error("not a readable PDF: {0}")] Open(String), #[error("page {page} out of range (document has {count})")] PageOutOfRange { page: usize, count: usize }, #[error("render of {width}×{height} px is too large")] TooLarge { width: u32, height: u32 }, #[error("render failed: {0}")] Render(String) }`
  - `pub trait PdfEngine: Send + Sync { fn open(&self, bytes: Arc<[u8]>) -> Result<Box<dyn Document>, EngineError>; }`
  - `pub trait Document { fn page_count(&self) -> usize; fn page_size(&self, page: usize) -> Result<Size, EngineError>; fn render_tile(&self, page: usize, scale: f32, rect: PixelRect) -> Result<Rgba8Image, EngineError>; }`. `page_size` is after `/Rotate`. `render_tile` renders the page at `scale` pixels per point and returns only `rect`, with a white background.
  - Fixture `fixtures/two-pages.pdf`. Page 0: 612×792 pt, white, with a black filled rectangle at PDF coordinates x 100–200, y 100–200 (origin bottom-left). Page 1: MediaBox 595×842 with `/Rotate 90`, blank.

- [ ] **Step 1:** Write the workspace `Cargo.toml` (members `crates/*`, `crates/frontends/*`), with `[workspace.dependencies]` for `hayro = "0.8"`, `eframe = "0.37"`, `clap = { version = "4", features = ["derive"] }`, `thiserror = "2"`, plus `rust-toolchain.toml` per Global Constraints. Run `cargo metadata --format-version 1 >/dev/null`. Expected: exit 0.
- [ ] **Step 2:** Write `fixtures/make_fixtures.py`. It uses the Python standard library only, writes the PDF with a correct xref table (computed byte offsets), and draws the rectangle with the content stream `0 0 0 rg 100 100 100 100 re f`. Run `python3 fixtures/make_fixtures.py`. Expected: `fixtures/two-pages.pdf` exists and starts with `%PDF-1.`.
- [ ] **Step 3: Write the failing tests** in `types.rs`: `rgba_rejects_wrong_length` (`from_premultiplied(2, 2, vec![0; 15])` is `None`, `vec![0; 16]` is `Some`) and `rgba_pixel_reads_row_major` (2×1 image `[1,2,3,4, 5,6,7,8]`, `pixel(1, 0) == [5,6,7,8]`).
- [ ] **Step 4:** Run `cargo test -p noido-engine`. Expected: FAIL (types not defined).
- [ ] **Step 5:** Implement the types and traits listed under Produces.
- [ ] **Step 6:** Run `cargo test -p noido-engine`, then the fmt and clippy checks. Expected: PASS, no warnings.
- [ ] **Step 7:** Commit with the message "Set up workspace and engine traits".

### Task 2: hayro engine

**Files:**
- Create: `crates/engine-hayro/{Cargo.toml,src/lib.rs,tests/render.rs}`

**Interfaces:**
- Consumes: everything from `noido-engine`; `fixtures/two-pages.pdf`.
- Produces: `pub struct HayroEngine;` (`Default`) implementing `PdfEngine`. Its `Document` impl is private.

Implementation notes:
- `hayro::hayro_syntax::Pdf::new(bytes.to_vec())`. A `LoadPdfError` maps to `EngineError::Open` with its `Debug` text.
- `page_size` uses `page.render_dimensions()`, which already accounts for rotation and crop box.
- `render_tile`:
  1. Check the page index.
  2. Reject `rect` with a zero side, or wider or taller than `u16::MAX`, as `TooLarge`.
  3. `RenderContext::new(rect.width as u16, rect.height as u16)`.
  4. Transform `Affine::translate((-x, -y)) * Affine::scale(scale) * page.initial_transform(true).to_kurbo()`.
  5. `hayro::render_into(..)`, `ctx.flush()`, `ctx.render_to_pixmap` / `render_with` on a `Pixmap` cleared to `WHITE`, as `hayro::render` does.
  6. Copy `pixmap.data()` (`PremulRgba8`) into bytes.
- `RenderCache` and `InterpreterSettings::default()` (embedded standard fonts) are created per `render_tile` call. `RenderCache` borrows from `Pdf`, so it can't be stored next to it without a self-referential struct. Caching across calls is deferred.

- [ ] **Step 1: Write the failing tests** in `tests/render.rs`, loading the fixture via `concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/two-pages.pdf")`:
  - `opens_and_counts_pages`: `page_count() == 2`.
  - `page_sizes_respect_rotation`: page 0 is `Size{612,792}`, page 1 is `Size{842,595}`.
  - `renders_full_page_at_scale`: page 0, scale 0.5, rect `0,0,306,396` → image 306×396; `pixel(75, 396-75)` (inside the rectangle, which spans x 50–100 and y 296–346 top-down at this scale) has R,G,B < 32; `pixel(10,10) == [255,255,255,255]`.
  - `renders_offset_tile`: page 0, scale 1.0, rect `x 110, y 602, 80×80` (wholly inside the rectangle) → every pixel has R < 32.
  - `page_out_of_range`: `page_size(2)` and `render_tile(2, ..)` are `Err(PageOutOfRange{page:2,count:2})`.
  - `rejects_too_large`: rect width `70_000` → `Err(TooLarge{..})`; width 0 → `Err(TooLarge{..})`.
  - `garbage_and_empty_fail_to_open`: `open(b"hello".into())` and `open([][..].into())` → `Err(Open(_))`.
- [ ] **Step 2:** Run `cargo test -p engine-hayro`. Expected: FAIL (`HayroEngine` not defined).
- [ ] **Step 3:** Implement `HayroEngine` per the notes above.
- [ ] **Step 4:** Run `cargo test -p engine-hayro` and the checks. Expected: PASS.
- [ ] **Step 5:** Commit with the message "Add hayro engine".

### Task 3: Core layout, render scale and render tracker

**Files:**
- Create: `crates/noido-core/{Cargo.toml,src/lib.rs,src/layout.rs,src/tracker.rs}`

**Interfaces:**
- Consumes: `noido_engine::Size`.
- Produces:
  - `pub const PAGE_GAP: f32 = 12.0; pub const MARGIN: f32 = 12.0; pub const MAX_RENDER_SIDE: u32 = 8192;`: UI points / pixels.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct PageRect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }`: UI points, origin top-left of the scroll content.
  - `pub struct Layout`:
    - `pub fn new(page_sizes: Vec<Size>) -> Self`
    - `pub fn set_available_width(&mut self, width: f32)`
    - `pub fn scale(&self) -> f32`: UI points per PDF point. It equals `(width - 2*MARGIN) / widest_page_width`, at least 0, and 0 when there are no pages or the widest page has zero width.
    - `pub fn page_rect(&self, page: usize) -> Option<PageRect>`: pages are centred horizontally and stacked from `y = MARGIN`, with `PAGE_GAP` between them.
    - `pub fn content_height(&self) -> f32`: the bottom of the last page plus `MARGIN`, or 0 with no pages.
    - `pub fn visible_pages(&self, top: f32, bottom: f32) -> std::ops::Range<usize>`: the pages whose rect intersects `[top, bottom)`. Empty when none.
  - `pub fn render_scale(page: Size, display_scale: f32, max_side: u32) -> f32`: returns `display_scale` unless the longest rendered side would exceed `max_side`, in which case it returns the scale that makes it exactly `max_side`. The frontend passes `layout.scale() * pixels_per_point`.
  - `pub fn full_page_rect(page: Size, scale: f32) -> noido_engine::PixelRect`: `0, 0, ceil(w*scale), ceil(h*scale)`.
  - `pub struct RenderTracker` (per page: nothing, requested at scale, ready at scale, failed at scale):
    - `pub fn new(page_count: usize) -> Self`
    - `pub fn wanted(&mut self, pages: Range<usize>, scale_for: impl Fn(usize) -> f32) -> Vec<(usize, f32)>`: returns the pages to request and marks them requested. A page is skipped when its requested or ready scale is within 2% of the wanted scale, or when it failed at a scale within 2%.
    - `pub fn mark_ready(&mut self, page: usize, scale: f32)`, `pub fn mark_failed(&mut self, page: usize, scale: f32)`
    - `pub fn forget_outside(&mut self, keep: Range<usize>) -> Vec<usize>`: resets pages outside `keep` to nothing and returns those that were ready, so the frontend drops their textures.

- [ ] **Step 1: Write the failing tests** in `layout.rs`:
  - `fit_width_scale`: pages `[612×792, 306×396]`, width `636` → `scale() == 1.0`. Page 0 rect `{x:12, y:12, w:612, h:792}`. Page 1 rect `{x: 12+153, y: 12+792+12, w:306, h:396}`. `content_height() == 12+792+12+396+12`.
  - `visible_pages_ranges`: same layout. `visible_pages(0, 10)` is `0..0`, `(0, 100)` is `0..1`, `(800, 820)` is `0..2`, `(10_000, 10_100)` is empty.
  - `no_pages_is_safe`: `Layout::new(vec![])` at width 500. `scale() == 0`, `content_height() == 0`, `page_rect(0)` is `None`, `visible_pages(0, 1000)` is empty.
  - `zero_width_page_is_safe`: `[Size{0,100}]` → `scale() == 0`, no panic.
  - `narrow_window_never_negative`: width 10 → `scale() >= 0`.
  - `render_scale_caps_longest_side`: `render_scale(Size{100, 20_000}, 2.0, 8192)`, then `20_000 * s <= 8192.0` and `> 8191.0`. `render_scale(Size{612,792}, 2.0, 8192) == 2.0`.
- [ ] **Step 2: Write the failing tests** in `tracker.rs`:
  - `requests_once`: `wanted(0..2, |_| 1.0)` → `[(0,1.0),(1,1.0)]`. Calling it again → empty.
  - `small_scale_change_ignored_large_rerequested`: after ready at 1.0, `|_| 1.01` → empty, and `|_| 1.5` → `[(0,1.5)]`.
  - `failed_page_not_rerequested`: `mark_failed(0, 1.0)`, then `wanted(0..1, |_| 1.0)` is empty, and at `2.0` it is requested again.
  - `keeps_old_image_while_rerendering`: ready at 1.0, then wanted at 1.5. `forget_outside(0..1)` returns empty (page 0 is kept).
  - `forget_outside_returns_ready_pages`: pages 0–3 ready. `forget_outside(1..2)` → `[0, 2, 3]`. Those pages are requested again on the next `wanted`.
- [ ] **Step 3:** Run `cargo test -p noido-core`. Expected: FAIL.
- [ ] **Step 4:** Implement `layout.rs` and `tracker.rs` per the interfaces.
- [ ] **Step 5:** Run `cargo test -p noido-core` and the checks. Expected: PASS.
- [ ] **Step 6:** Commit with the message "Add layout and render tracking to core".

### Task 4: Render thread

**Files:**
- Create: `crates/noido-render/{Cargo.toml,src/lib.rs,tests/renderer.rs}`. Dev-dependency: `engine-hayro`.

**Interfaces:**
- Consumes: `PdfEngine`, `Document`, `Size`, `Rgba8Image`, `EngineError`, `noido_core::full_page_rect`.
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct RenderRequest { pub page: usize, pub scale: f32 }`
  - `pub struct RenderedPage { pub page: usize, pub scale: f32, pub result: Result<Rgba8Image, EngineError> }`
  - `#[derive(Clone, Debug)] pub struct DocumentInfo { pub page_sizes: Vec<Size> }`
  - `pub struct Renderer` with:
    - `pub fn start(engine: Arc<dyn PdfEngine>, bytes: Arc<[u8]>, waker: Box<dyn Fn() + Send>) -> Result<(Renderer, DocumentInfo), EngineError>`. It spawns a thread named `noido-render`, which opens the document and sends back `DocumentInfo` or the error over a one-shot channel. `start` blocks on that answer.
    - `pub fn request(&self, req: RenderRequest)`
    - `pub fn try_recv(&self) -> Option<RenderedPage>`
    - Dropping the `Renderer` closes the job channel, and the thread exits after its current job.

Thread loop: block for one job, then drain everything queued without blocking, keeping only the latest request per page and preserving first-arrival order. Render each kept job as a full page via `full_page_rect`, send the `RenderedPage`, and call `waker()`.

- [ ] **Step 1: Write the failing tests** in `tests/renderer.rs`:
  - `start_reports_page_sizes`: the fixture with `HayroEngine` → `page_sizes == [612×792, 842×595]`.
  - `start_fails_on_garbage`: bytes `b"nope"` → `Err(EngineError::Open(_))`.
  - `renders_requested_page_and_wakes`: the waker increments an `AtomicUsize`. Request `{page:0, scale:0.5}`, then poll `try_recv` for up to 5 s. Expect `page 0`, an `Ok` image of 306×396, and a waker count ≥ 1.
  - `out_of_range_page_reports_error`: request page 9 → a `RenderedPage` with `Err(PageOutOfRange{..})`.
  - `coalesces_to_latest_per_page`: a test-local `GateEngine`/`GateDocument` (2 pages of 100×100). `render_tile` waits on a `Barrier` the first time, then records `(page, scale)` in an `Arc<Mutex<Vec<_>>>` and returns a 1×1 image. Request `(0,1.0)` and wait until it's inside the barrier. Then request `(0,2.0)`, `(1,1.0)` and `(0,3.0)`, and release the barrier. After all results arrive, the recorded order is `[(0,1.0), (0,3.0), (1,1.0)]`.
- [ ] **Step 2:** Run `cargo test -p noido-render`. Expected: FAIL.
- [ ] **Step 3:** Implement `Renderer` per the interfaces and the loop above (`std::sync::mpsc`; no async runtime).
- [ ] **Step 4:** Run `cargo test -p noido-render` and the checks. Expected: PASS.
- [ ] **Step 5:** Commit with the message "Add render thread".

### Task 5: egui frontend and `noido` binary

**Files:**
- Create: `crates/frontends/gui-egui/{Cargo.toml,src/main.rs,src/load.rs,src/app.rs}`. In `Cargo.toml`, set `[[bin]] name = "noido"`.

**Interfaces:**
- Consumes: `Renderer`, `RenderRequest`, `RenderedPage`, `DocumentInfo`, `Layout`, `RenderTracker`, `render_scale`, `MAX_RENDER_SIDE`, `HayroEngine`.
- Produces:
  - `load::load_document(path: &Path) -> Result<Arc<[u8]>, String>`. Error messages, exact:
    - `"<path>: no such file"`
    - `"<path>: is a directory"`
    - `"<path>: file is empty"`
    - `"<path>: <io error>"` for other read errors

    Opening errors from `Renderer::start` are formatted in `main` as `"<path>: <EngineError>"`.
  - CLI: `noido <PATH>` via clap derive (`#[command(name = "noido", version, about = "PDF reader and note taker")]`). If the argument is missing, clap prints its usage and exits 2. That's clap's convention and acceptable.

App behaviour (`NoidoApp`, `impl eframe::App`), each frame:
1. Drain `renderer.try_recv()`. On `Ok`, upload a texture with `ctx.load_texture(format!("page-{n}"), ColorImage::from_rgba_premultiplied([w,h], pixels), TextureOptions::LINEAR)`, replace the page's previous texture, and call `tracker.mark_ready`. On `Err`, call `tracker.mark_failed`; logging to stderr is fine.
2. In a `CentralPanel` with a `ScrollArea::vertical().auto_shrink(false).show_viewport`:
   - `layout.set_available_width(viewport width)`.
   - Allocate `content_height()`.
   - Compute `visible_pages(top, bottom)`, extended by one page each side, clamped.
   - Call `tracker.wanted(range, |p| render_scale(size_p, layout.scale() * ctx.pixels_per_point(), MAX_RENDER_SIDE))` and pass each result to `renderer.request`.
   - Call `tracker.forget_outside(range extended by 2 pages)` and drop those textures.
   - Paint each visible page: its texture stretched to the page rect, or a white rect if it has none, on the panel's default background.
3. The window title is set at startup per Global Constraints, and the initial inner size is 900×1100.

`main`:
1. Parse args.
2. `load_document`, then `Renderer::start(Arc::new(HayroEngine), bytes, waker)`. The waker is built from an `egui::Context` that is only available inside the eframe creator closure. So: create an `Arc<OnceLock<egui::Context>>`, have the waker call `request_repaint()` if it's set, and set it in the `run_native` creator closure.
3. On error, print `noido: <message>` to stderr and exit 1.
4. Otherwise call `eframe::run_native`.

- [ ] **Step 1: Write the failing tests** in `load.rs`, using `std::env::temp_dir()` with a unique subfolder:
  - `missing_file`: the error ends with `": no such file"`.
  - `directory`: the error ends with `": is a directory"`.
  - `empty_file`: the error ends with `": file is empty"`.
  - `reads_fixture`: `fixtures/two-pages.pdf` → `Ok(bytes)` with `bytes.starts_with(b"%PDF-")`.
- [ ] **Step 2:** Run `cargo test -p gui-egui`. Expected: FAIL.
- [ ] **Step 3:** Implement `load.rs`. Run the tests. Expected: PASS.
- [ ] **Step 4:** Implement `main.rs` and `app.rs` as described.
- [ ] **Step 5:** Verify the error paths. Expected: stderr `noido: …` and exit 1 for each.

```bash
cargo run -q -p gui-egui -- /nonexistent.pdf; echo "exit $?"
cargo run -q -p gui-egui -- fixtures; echo "exit $?"
cargo run -q -p gui-egui -- Cargo.toml; echo "exit $?"
```

- [ ] **Step 6:** Check by hand: `cargo run -p gui-egui -- fixtures/two-pages.pdf`, then a real multi-page PDF from the user. Expected:
  - The window is titled `two-pages.pdf — noido`.
  - Both pages are visible; the second is landscape.
  - Resizing the window refits the pages without blanking them.
  - Scrolling through a long document stays responsive, with pages filling in after their placeholders.
- [ ] **Step 7:** Run `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Expected: all pass.
- [ ] **Step 8:** Commit with the message "Add egui frontend: noido <file.pdf>".
