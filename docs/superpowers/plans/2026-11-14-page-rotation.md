# Page Rotation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The reader turns a single page 0°/90°/180°/270° from a right-click menu on the page, its thumbnail or its pin; the turn is saved in the document's state, and the PDF is never changed.

**Architecture:** The turn lives in a `Rotation` in `noido-engine` with all the quarter-turn geometry. A `Transformed` wrapper in `noido-render` sits directly on the engine's `Document` and hands every thread pages, renders, glyphs, links and destinations already turned, reading a shared `Transforms` handle. Above it nothing knows about rotation, except the text layer, which first learns to pick and draw text running in any of the four directions, and the frontend, which on a change drops that page's caches and keeps the view still.

**Tech Stack:** Rust 2024, eframe/egui 0.37, hayro, serde/toml. jj for commits.

**Spec:** `docs/superpowers/specs/2026-11-14-page-rotation-design.md`

## Global Constraints

- The PDF file is never written.
- Menu entries, in order: `0°`, `90°`, `180°`, `270°`; a check by the current one; clockwise; absolute (not added to the current turn).
- 0° is the page as noido shows it today (the PDF's own `/Rotate` applied).
- Point mapping on a page of unturned size `w × h`, origin top-left, y down: 90° → `(h − y, x)`, 180° → `(w − x, h − y)`, 270° → `(y, w − x)`.
- State file: `[[pages]]` records with `page` (0-based index, as pins) and `rotation` in degrees, one of 90, 180, 270; only non-identity records written; other angles and pages past the end dropped on load; files without the table load as no transforms.
- Rotation is not in the highlights' undo history and has no shortcut or menu-bar entry.
- Rotating never navigates; the view keeps its centre point (`View::place`).
- The engine trait (`noido_engine::Document`, `PdfEngine`) does not change.
- Tests use only `fixtures/`. TDD: each test watched failing first.
- Commits: `jj commit -m "<prefix>: …"`, prefixes `engine`, `text`, `core`, `state`, `render`, `egui`. Don't move `main`.
- `just check` clean (tests, clippy, fmt) at the end of every task.

## Review Focus

1. A page turned while a search is showing hits on it: the hits must be redrawn on the turned text, not left in the old frame. (Task 7 test `rotating_a_page_redoes_the_search`.)
2. A render or thumbnail of the page that was already under way when it was turned arrives afterwards: it must not be shown. (Task 6 stamps; Task 7 test `a_stale_render_is_not_shown`.)
3. A link on another page that points at the turned page, and an outline entry that does: following it must land on the same spot of the content. (Task 7 test `links_into_a_rotated_page_land_on_the_content`.)
4. The file is reloaded (edited in place) with fewer pages: rotations past the end go, the rest stay applied to the new threads. (Task 7 test `reload_keeps_rotations_and_drops_those_past_the_end`.)
5. Right-click in the gap between pages or on the desk beside them: no menu, nothing turns. (Task 8 test `right_click_off_the_pages_opens_no_menu`.)

---

### Task 1: `Rotation` and its geometry

**Files:**
- Create: `crates/noido-engine/src/rotation.rs`
- Modify: `crates/noido-engine/src/lib.rs` (module and re-export)
- Test: in `rotation.rs`

**Interfaces:**
- Produces, in `noido_engine`:
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] pub enum Rotation { #[default] R0, R90, R180, R270 }`
  - `Rotation::ALL: [Rotation; 4]` (menu order)
  - `fn degrees(self) -> u16`, `fn from_degrees(d: u16) -> Option<Rotation>` (only 0/90/180/270)
  - `fn inverse(self) -> Rotation`, `fn then(self, next: Rotation) -> Rotation`
  - `fn size(self, s: Size) -> Size` (swapped for 90/270)
  - `fn point(self, p: Point, s: Size) -> Point` — `s` is the unturned size; with `Size { 0, 0 }` it is a pure turn about the origin (Task 2 uses that)
  - `fn rect(self, r: Rect, s: Size) -> Rect` (bounds of the turned corners)
  - `fn quad(self, q: Quad, s: Size) -> Quad` (corner order kept, so the writing direction turns with it)
  - `fn remap(p: Point, from: Rotation, to: Rotation, unturned: Size) -> Point` (a point in `from`'s frame to `to`'s)
  - `Rgba8Image::turned(&self, r: Rotation) -> Rgba8Image` in `types.rs`

- [ ] **Step 1: Write failing tests**: `point_follows_the_spec_formulas` (each turn on `w=100, h=200`, `p=(10,20)` → R90 `(180,10)`, R180 `(90,180)`, R270 `(20,90)`); `inverse_undoes_every_turn` (for all `r`, `r.inverse().point(r.point(p,s), r.size(s)) == p`); `then_adds_turns` (`R90.then(R270) == R0`, `R90.then(R90) == R180`); `size_swaps_on_quarter_turns`; `degrees_round_trip_and_reject_others` (`from_degrees(45) == None`, `from_degrees(360) == None`); `remap_between_turns_equals_through_unturned`; `turned_image_moves_the_corner_pixel` (a 3×2 image with one red pixel at (0,0) → R90: 2×3, red at (1,0); R180: red at (2,1); R270: red at (0,2)).
- [ ] **Step 2:** `cargo nextest run -p noido-engine rotation` → FAIL (not defined).
- [ ] **Step 3: Implement** in `rotation.rs` and `types.rs`.
- [ ] **Step 4:** same command → PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "engine: a quarter-turn rotation and how points, rects and images turn"`

---

### Task 2: Text in any direction

The spec's first step: fixes `/Rotate` pages today, needed for turned ones.

**Files:**
- Modify: `crates/noido-text/src/page.rs`
- Modify: `crates/noido-text/src/search.rs` only if `find` reads `bounds` directly rather than through `rects`
- Test: `crates/noido-text/src/page.rs` tests; `crates/noido-render/tests/searcher.rs` (fixture)

**Interfaces:**
- Consumes: `Rotation::{point, rect, quad, inverse}` with `Size { width: 0.0, height: 0.0 }` (pure turn).
- Produces: no new public API. `PageText::{caret_at, word_at, line_range_at, rects, caret_rect, line_rect_of, glyph_bounds, line_near}` give right answers for text running in any of the four directions, in the page's own frame as before. `text_span` keeps its page-frame meaning (left, right of the glyphs).

**Approach (the decision the tests don't make):** `PageText::new` finds the page's writing direction: each visible glyph votes with the quarter turn nearest its baseline direction (`quad.corners[0] → corners[1]`); the most votes win, ties to `R0`. `PageText` keeps that as `upright: Rotation` and stores `bounds` in the upright frame (`upright.inverse()` applied, pure turn), so every existing algorithm (`lines`, `line_for`, `columns`, `caret_at`, `rects`, `caret_rect`) runs unchanged on horizontal text. Public methods map incoming points with `upright.inverse().point(…)` and outgoing rects with `upright.rect(…)`. Mixed-direction pages behave for their minority text as today.

- [ ] **Step 1: Write failing tests** in `page.rs`, using a helper `fn turned(glyphs: Vec<Glyph>, r: Rotation) -> Vec<Glyph>` (origin and quad turned, pure turn) over the existing `page(&["the cat sat", "on the mat"])` glyphs:
  - `caret_at_follows_every_direction`: for each `r` in `Rotation::ALL`, `turned_page.caret_at(rp(73,97)) == Some(0)` and `caret_at(rp(76,97)) == Some(1)`, where `rp` turns the point by `r`.
  - `word_at_follows_every_direction`: same for an existing `word_at` case.
  - `rects_follow_every_direction`: `turned.rects(4..7) == straight.rects(4..7).map(|x| r.rect(x, ZERO))`.
  - `caret_rect_crosses_the_line_in_every_direction`: for R90, the caret rect has `height == 0.0` and `width` equal to the line's extent across.
  - `upside_down_lines_keep_their_order`: R180 — `caret_at` below the second line (in the turned frame) gives the end of text.
- [ ] **Step 2: Fixture test** in `crates/noido-render/tests/searcher.rs`: `rotated_fixture_page_selects_like_the_upright_one` — `text.pdf` page 1 (`/Rotate 90`) and page 0: `word_at` at the centre of "World"'s glyph bounds on each gives the same char range; `rects` of "Hello" on page 1 has one rect taller than wide.
- [ ] **Step 3:** `cargo nextest run -p noido-text -p noido-render` → the new tests FAIL.
- [ ] **Step 4: Implement** as in Approach.
- [ ] **Step 5:** all tests PASS, including every existing `noido-text`, `noido-core` selection and harness test; `just check` clean.
- [ ] **Step 6:** `jj commit -m "text: pick and draw text in whichever direction it runs"` (body: fixes selection on pages with `/Rotate 90`).

---

### Task 3: Page transforms in core and a view that keeps still

**Files:**
- Create: `crates/noido-core/src/transforms.rs` (+ `mod`/`pub use` in `lib.rs`)
- Modify: `crates/noido-core/src/layout.rs`, `view.rs`, `history.rs`
- Test: in each file

**Interfaces:**
- Consumes: `noido_engine::Rotation`.
- Produces, in `noido_core`:
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct PageTransform { pub rotation: Rotation }` with `fn is_identity(&self) -> bool`
  - `#[derive(Clone, Debug, Default, PartialEq, Eq)] pub struct PageTransforms` (a `BTreeMap<usize, PageTransform>` holding only non-identity): `get(page) -> PageTransform`, `set(page, PageTransform)` (identity removes), `iter() -> impl Iterator<Item = (usize, PageTransform)>` in page order, `retain_below(page_count)`, `from_records(impl IntoIterator<Item = (usize, PageTransform)>)`.
  - `Layout::set_page_size(&mut self, page: usize, size: Size)` (takes effect at next `arrange`).
  - `View::set_page_size(&mut self, page: usize, size: Size, remap: impl Fn(DocPoint) -> DocPoint)`: takes `place()`, applies `remap` to it if it is on `page`, sets the size, re-applies the zoom mode (fit modes refit, `Custom` keeps scale), and puts the (remapped) point back at the viewport centre.
  - `History::map_points(&mut self, page: usize, f: impl Fn(DocPoint) -> DocPoint)` (back and forward entries on `page`).

- [ ] **Step 1: Failing tests**:
  - `transforms.rs`: `identity_is_not_kept`, `retain_below_drops_past_the_end`, `iter_is_in_page_order`.
  - `view.rs`: `turning_a_landscape_page_lifts_fit_width` (pages `[612×792, 842×595, 612×792]`, FitWidth: after `set_page_size(1, 595×842, id)`, `scale()` is larger than before and equals the all-portrait fit); `turning_the_page_in_view_keeps_its_spot` (centre on page 1 at (100, 300) in 842×595; set to 595×842 with `remap` = `Rotation::remap(.., R0, R90, ..)`; `place()` == the remapped point, within 0.5); `turning_a_page_before_the_view_keeps_the_view` and `…_after_…` (centre on page 2; turn page 0 / page 3; `place()` unchanged within 0.5); `turning_in_a_spread_keeps_the_spread` (dual, paged; `current_spread()` unchanged).
  - `history.rs`: `map_points_touches_only_that_page`.
- [ ] **Step 2:** `cargo nextest run -p noido-core` → FAIL.
- [ ] **Step 3: Implement.** `View::set_page_size` reuses the anchoring of `rescale` (`layout.doc_point` → `arrange` → `content_point`) with the centre as both `from` and `to`; in paged mode the spread stays the one holding the anchor.
- [ ] **Step 4:** PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "core: page transforms, and a view that keeps still when a page changes shape"`

---

### Task 4: Saving rotations

**Files:**
- Modify: `crates/noido-state/src/lib.rs` (`DocState`, `Entry`)
- Test: `crates/noido-state/tests/store.rs`

**Interfaces:**
- Consumes: `PageTransforms`, `PageTransform`, `Rotation::{degrees, from_degrees}`.
- Produces: `DocState.transforms: PageTransforms`. `Entry` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pages: Vec<PageEntry>` with `PageEntry { page: usize, rotation: u16 }` → TOML `[[pages]]`.

- [ ] **Step 1: Failing tests** in `store.rs`: `rotations_round_trip`; `rotations_are_written_as_pages_table` (saved text contains `[[pages]]`, `page = 3`, `rotation = 90`); `odd_angles_are_dropped_on_load` (hand-written `rotation = 45` and `rotation = 0` → no transform); `files_without_pages_load_without_transforms` (an existing pre-feature fixture string). Out-of-range pages are dropped by the app (Task 7), which knows the page count.
- [ ] **Step 2:** `cargo nextest run -p noido-state` → FAIL.
- [ ] **Step 3: Implement**; update every `DocState { … }` literal in the workspace (`transforms: PageTransforms::default()`).
- [ ] **Step 4:** PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "state: keep each page's rotation"`

---

### Task 5: The `Transformed` document

**Files:**
- Create: `crates/noido-render/src/transformed.rs` (+ `mod`/`pub use` in `lib.rs`)
- Test: `crates/noido-render/tests/transformed.rs`

**Interfaces:**
- Consumes: `PageTransforms`, `PageTransform`, `Rotation` (Task 1, 3).
- Produces, in `noido_render`:
  - `#[derive(Clone, Default)] pub struct Transforms(Arc<RwLock<PageTransforms>>)`: `new(PageTransforms)`, `get(page) -> PageTransform`, `set(page, PageTransform)`, `snapshot() -> PageTransforms`, `retain_below(page_count)`.
  - `pub struct Transformed { inner: Box<dyn Document>, transforms: Transforms }`, `impl Document`; `Transformed::new(inner, transforms)`; `fn transform(&self, page) -> PageTransform`.
  - `pub fn open_transformed(engine: &dyn PdfEngine, bytes: PdfBytes, transforms: &Transforms) -> Result<Transformed, EngineError>`.
- Behaviour, with `r` the page's rotation and `s` the inner (unturned) size:
  - `page_size` → `r.size(s)`; `render_page(p, w, h)` → inner at the unturned pixel size, then `.turned(r)`; `render_tile` → the rect mapped by `r.inverse()` against the page's pixel size rounded, rendered, turned.
  - `text` → each glyph's `origin` by `r.point`, `quad` by `r.quad`.
  - `links` → `areas` by `r.rect`; `LinkTarget::Internal` destinations by the *target* page's rotation (each turned coordinate depends on one source coordinate (90°: `x' = h − y`, `y' = x`), so a turned coordinate is `None` exactly when its source is).
  - `named_destination`, `outline` (recursively) → destinations as for links.
  - `page_count`, `page_labels`, properties and anything else pass through.

- [ ] **Step 1: Failing tests** in `tests/transformed.rs`, against fixtures:
  - `sizes_swap_on_quarter_turns` (`two-pages.pdf` page 0 at R90 → 792×612; page 1 stays 842×595 at R0).
  - `render_turns_the_square` (`two-pages.pdf` page 0, square at x 100–200, y 100–200 PDF space (bottom-left origin, so top-left y 592–692); render at scale 1: at R0 the pixel at (150, 642) is black; at R90 the pixel at `R90.point((150,642), 612×792)` is black and (150, 642)'s turned counterpart for R0 is white).
  - `glyphs_turn_with_the_page` (`text.pdf` page 0 at R90: every glyph inside the 792×612 page; "Hello" origins go down the page, as the `/Rotate` fixture page does).
  - `link_areas_and_targets_turn` (`links.pdf`: page 0 at R90, link A's area equals `R90.rect` of its R0 area; page 1 at R90, link A's target `(72, 600 PDF)` maps by page 1's turn).
  - `outline_targets_turn` (`outline.pdf`, its first entry's page turned).
  - `changes_apply_to_the_next_call` (`set` after `new`; next `page_size` reflects it).
- [ ] **Step 2:** `cargo nextest run -p noido-render transformed` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "render: hand out pages turned by the reader's rotation"`

---

### Task 6: The threads open `Transformed` and stamp what they return

**Files:**
- Modify: `crates/noido-render/src/lib.rs`, `thumbnails.rs`, `text.rs`, `links.rs`
- Modify: every `Renderer::start`/`start_background`, `Thumbnailer::start`, `TextSearcher::start`, `Linker::start` call site (frontend and tests) to pass a `Transforms`
- Test: `crates/noido-render/tests/renderer.rs`, `searcher.rs`, `linker.rs`, a thumbnail test

**Interfaces:**
- Consumes: `Transforms`, `open_transformed`, `Transformed::transform` (Task 5).
- Produces:
  - Each `start` gains a last-but-one parameter `transforms: Transforms` (before `waker`).
  - `RenderedPage.transform: PageTransform`, `Thumbnail.transform: PageTransform`, `SearchUpdate::Text { …, transform: PageTransform }`, `LinkReply::Links { …, transform: PageTransform }` — each read from the document *before* the work starts.
  - `Thumbnailer::refresh(&self, page: usize)`: the page is rendered again even if delivered.
  - `TextSearcher::forget(&self, page: usize)`: drops the page's cached text.
  - `DocumentInfo.page_sizes` come out turned.

- [ ] **Step 1: Failing tests**: `renderer_reports_turned_sizes_and_stamps` (`Renderer::start` with page 0 at R90 → `info.page_sizes[0]` swapped; a render result carries `transform.rotation == R90`); `thumbnail_refresh_renders_again`; `forgotten_text_is_read_again_turned` (request text, set R90, `forget`, request again → a `Text` update with `transform.rotation == R90` and glyph bounds in 792×612); `links_carry_their_stamp`.
- [ ] **Step 2:** `cargo nextest run -p noido-render` → FAIL.
- [ ] **Step 3: Implement**; pass `Transforms::default()` at test call sites, and at frontend call sites for now (Task 7 replaces them).
- [ ] **Step 4:** PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "render: open documents turned and stamp results with their turn"`

---

### Task 7: The app turns a page

**Files:**
- Modify: `crates/frontends/gui-egui/src/document.rs`, `watch.rs`, `app.rs`, `links.rs`, `search.rs`, `texts.rs`
- Test: harness tests in `app.rs`

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces:
  - `document::open` builds `Transforms::new(restored.transforms.retained_below(..))` (page count is known only after open; drop past-the-end with `Transforms::retain_below` once `info` arrives) and passes it to every thread and to `Watcher::start` (new parameter), which passes it to a reload's threads.
  - `NoidoApp.transforms: Transforms`; `doc_state()` writes `transforms: self.transforms.snapshot()`.
  - `NoidoApp::set_rotation(&mut self, page: usize, rotation: Rotation)`: no-op if unchanged; else, in this order: compute the old and new rotations and the unturned size; `transforms.set`; `view.set_page_size(page, new size, remap)` and `history.map_points(page, remap)` with `remap = Rotation::remap(.., old, new, unturned)`; drop `textures[page]`, the thumbnail texture, `texts` slot (`Texts::forget(page)`, new); `thumbnailer.refresh(page)`; `searcher.forget(page)` and, if a search is showing, run it again from the current page; drop *every* page's cached links (`Links::forget_pages()`, new) and ask for the outline again; mark the state changed.
  - Results whose stamp differs from `transforms.get(page)` are dropped where they are taken in (renders, thumbnails, texts, links).
- On reload, `transforms.retain_below(new page count)`.

- [ ] **Step 1: Failing harness tests** (fake engine with mixed page sizes, as existing harness tests build; real `two-pages.pdf`/`links.pdf`/`text.pdf` where the test names a fixture):
  - `set_rotation_turns_the_page_and_keeps_the_view` (layout size of the page swapped; `centre()` unchanged off the page).
  - `a_stale_render_is_not_shown` (deliver a `RenderedPage` stamped R0 after turning to R90; `textures` has no entry for the page).
  - `rotating_a_page_redoes_the_search` (search "World" in `text.pdf`; turn page 0; after settling, the hit's rect on page 0 is taller than wide).
  - `links_into_a_rotated_page_land_on_the_content` (`links.pdf`: turn page 1 to R90; follow link A on page 0; the view's target point equals `R90.point` of the unturned target).
  - `rotations_are_saved_and_restored` (turn, `doc_state()` has it; a new app `with_persistence` from that state opens with the page turned).
  - `reload_keeps_rotations_and_drops_those_past_the_end` (rotations on pages 1 and 5 of a 6-page fake; reload with 4 pages → page 1 still turned, page 5's gone).
- [ ] **Step 2:** `cargo nextest run -p gui-egui` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "egui: turn a page, keep the view still and redo what depended on it"`

---

### Task 8: The right-click menu

**Files:**
- Modify: `crates/frontends/gui-egui/src/app.rs` (page area), `sidebar.rs` (the thumbnail `interact` at ~line 924, shared by the Pages and Pins tabs)
- Test: harness tests in `app.rs`; a harness helper `fn right_click(&mut self, at: Pos2)` (secondary press and release, like `click`)

**Interfaces:**
- Consumes: `NoidoApp::set_rotation`, `Rotation::ALL`, `Rotation::degrees`.
- Produces: `fn rotation_menu(ui: &mut egui::Ui, current: Rotation) -> Option<Rotation>` in a new `crates/frontends/gui-egui/src/rotation_menu.rs`: four entries labelled `format!("{}°", r.degrees())`, a check (egui's `checkbox`-style selectable or `✓` prefix, as the existing menus mark state) on `current`; returns the chosen one, `None` if none or the current one was chosen. The sidebar reports `rotate: Option<(usize, Rotation)>` in its output alongside `clicked`; the page area takes the page from `layout.doc_point` at the press position, latched when the menu opens.

- [ ] **Step 1: Failing harness tests**:
  - `right_click_on_a_page_offers_four_angles` (texts `0°`, `90°`, `180°`, `270°` drawn after a right-click on page 0).
  - `choosing_an_angle_turns_the_page_under_the_pointer` (dual mode, right-click the right page of a spread, `click_text("90°")` → that page R90, the other R0).
  - `the_current_angle_is_checked`.
  - `angles_are_absolute` (choose 90°, then 90° again → R90; then 180° → R180).
  - `right_click_on_a_thumbnail_turns_that_page` and `right_click_on_a_pin_turns_that_page` (sidebar open on Pages / Pins; neither navigates: `centre()` unchanged).
  - `right_click_off_the_pages_opens_no_menu` (right-click in the margin left of a page: no `90°` drawn).
- [ ] **Step 2:** `cargo nextest run -p gui-egui` → FAIL.
- [ ] **Step 3: Implement** with `Response::context_menu`; the page-area response needs `Sense::click()` for the secondary button without disturbing text selection and link clicks.
- [ ] **Step 4:** PASS; `just check` clean.
- [ ] **Step 5:** `jj commit -m "egui: a right-click menu to rotate a page"`
- [ ] **Step 6:** `just build`; tell the user what to try by hand (see below).

---

## After the plan

- Final review by a fresh subagent on the most capable model; Critical/Important fixed test-first as fixup commits, squashed before reporting; Minors to the user.
- By hand, with `just run` on a document with landscape pages: right-click a landscape page → 90°; fit-width grows; the thumbnail and pin turn; select text and highlight on the turned page; search a word on it; quit and reopen → still turned; 0° restores. Also a PDF with its own `/Rotate 90` page: text selection now works there.
