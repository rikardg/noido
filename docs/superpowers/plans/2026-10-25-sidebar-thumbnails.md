# Page thumbnails in the sidebar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The sidebar shows one column of page thumbnails at the sidebar's width, sharp where visible, highlighting and following the current page; clicking one goes to it.

**Architecture:** `noido-core` gets the pure parts: `View::go_to_page` and a `Column` that lays thumbnails out and answers which are visible and how far to scroll to reveal one. `noido-render` gets a non-blocking `Renderer::start_background`. The frontend's `sidebar.rs` becomes a `SidebarView` that owns the sidebar's renderer, tracker and sharp textures and draws the column; `app.rs` keeps a clicked ("chosen") page that overrides the centre page until the reader moves the page view, and feeds the thumbnailer whichever of the page view or the sidebar moved last.

**Tech Stack:** Rust 2024, eframe/egui 0.37, hayro via `noido-engine`.

**Spec:** `docs/superpowers/specs/2026-10-25-sidebar-thumbnails-design.md`

## Global Constraints

- `unsafe_code = "forbid"`; clippy clean (`cargo clippy --workspace --all-targets -- -D warnings`).
- Column geometry: `COLUMN_MARGIN = 12.0` pt each side and above the first thumbnail; `LABEL_HEIGHT = 20.0` pt for the page number under each thumbnail; `COLUMN_GAP = 12.0` pt between a number and the next thumbnail. Every thumbnail has width `inner width − 2 × COLUMN_MARGIN` (at least 1 pt); height follows the page.
- Always one thumbnail per page, never spreads.
- Sharp renders only for thumbnails visible in the sidebar, at `render_scale(size, thumb_width_pt × pixels_per_point / size.width, MAX_RENDER_SIDE)`. No margin beyond the visible area.
- Drawing per thumbnail: sharp image (possibly an older scale) → 120 px low-res thumbnail → white. Never the page view's full render.
- The chosen (clicked) page lasts until the reader moves the page view: a view key action, a title-bar view action, wheel flip, zoom, or a scroll by egui (wheel, drag, scrollbar) seen while the page view's size is unchanged. A window or sidebar resize doesn't clear it. The saved state is still the page view's centre.
- The sidebar renderer starts without blocking, at launch, even with the sidebar closed.
- The sidebar's scroll position is not saved.
- Never send synthetic input to the desktop; test in the headless harness.

## Review Focus

1. **Wheel over the sidebar in paged mode** must scroll the sidebar, not flip the page view (today `wheel_flip` ignores where the pointer is). Test: `wheel_over_sidebar_does_not_flip` (Task 5).
2. **Moving the window to a screen with another scale** (`pixels_per_point` changes) must re-request sharp thumbnails at the new scale, as a width change does. Test: `sidebar_rerequests_on_pixels_per_point_change` (Task 4).
3. **A window resize after a click** must keep the chosen page (egui re-clamps its offset then). Test: `resize_keeps_chosen_page` (Task 5).
4. **Renders in flight across a reload** must not be shown for the new version's pages at a wrong size: the old sidebar renderer is dropped with its channel; images past the new count go. Test: `reload_swaps_sidebar_renderer` (Task 6).
5. **Clicking the last page, and a one-page document:** the chosen page is the clicked one, `go_to_page` clamps. Tests: `go_to_last_page_clamps` (Task 1), `click_on_last_page_highlights_it` (Task 5).

---

### Task 1: `View::go_to_page`

**Files:**
- Modify: `crates/noido-core/src/view.rs` (method + tests)

**Interfaces:**
- Produces: `View::go_to_page(&mut self, page: usize)` — out-of-range pages clamp to the last; zero pages do nothing. Zoom mode, scale and arrangement are unchanged except as paged fit needs. Continuous: `offset.1 = page_rect(page).y − MARGIN`, x unchanged, clamped. Paged: shows the spread holding the page from the top, refitting the fit modes for it (as `flip` does).

- [ ] **Step 1: Write the failing tests** in `view.rs` tests, using the existing helpers there:
  - `go_to_page_continuous_puts_top_at_top`: 10 Letter pages, viewport 800×600, fit width; `go_to_page(4)`; `offset().1 == layout.page_rect(4).y − MARGIN` (within 0.01); `mode()` still `FitWidth`, `scale()` unchanged.
  - `go_to_page_keeps_custom_zoom`: zoom in twice, `go_to_page(3)`; `mode() == Custom`, scale unchanged; page 3 top at `MARGIN`.
  - `go_to_page_paged_shows_spread`: paged mode (`ToggleContinuous`), `go_to_page(6)`; `current_spread() == Some(6)`, `offset().1 == 0`.
  - `go_to_page_paged_dual_shows_spread_of_page`: paged + dual; `go_to_page(4)`; `current_spread() == Some(layout.spread_of(4))`.
  - `go_to_last_page_clamps`: continuous; `go_to_page(99)` → offset is the max offset (bottom), no panic; paged → `current_spread() == Some(spread_count − 1)`.
  - `go_to_page_with_zero_pages_is_safe`.
- [ ] **Step 2: Run** `cargo test -p noido-core go_to` — Expected: compile error, no `go_to_page`.
- [ ] **Step 3: Implement** `go_to_page`. In paged mode reuse the body of `flip` after its `next` computation (extract a private `show_spread(&mut self, spread, from_top: bool)` and call it from both).
- [ ] **Step 4: Run** `cargo test -p noido-core` — Expected: all pass.
- [ ] **Step 5: Commit** `jj commit -m "View: go to a page, keeping zoom and modes"`

### Task 2: `Column` — thumbnail layout

**Files:**
- Create: `crates/noido-core/src/column.rs` (export from `lib.rs`)

**Interfaces:**
- Produces:
  - `pub const COLUMN_MARGIN: f32 = 12.0; pub const LABEL_HEIGHT: f32 = 20.0; pub const COLUMN_GAP: f32 = 12.0;`
  - `pub struct Column` built by `Column::new(page_sizes: &[Size], width: f32) -> Column` (width = the sidebar's inner width in points).
  - `fn thumb_width(&self) -> f32` — `(width − 2·COLUMN_MARGIN).max(1.0)`.
  - `fn thumb(&self, page) -> Option<PageRect>` — x = `COLUMN_MARGIN`; first y = `COLUMN_MARGIN`; next y = previous y + height + `LABEL_HEIGHT + COLUMN_GAP`. A page with a non-finite or non-positive width gets height = thumb width.
  - `fn label(&self, page) -> Option<PageRect>` — the `LABEL_HEIGHT` row under the thumbnail, same x and width.
  - `fn content_height(&self) -> f32` — last label bottom + `COLUMN_MARGIN` (`0` with no pages).
  - `fn visible(&self, top: f32, bottom: f32) -> Range<usize>` — pages whose thumbnail or label overlaps `top..bottom`.
  - `fn page_at(&self, y: f32) -> Option<usize>` — the page whose slot (thumb top to next thumb top) holds `y`, clamped to first/last.
  - `fn reveal(&self, page: usize, offset: f32, view_height: f32) -> Option<f32>` — `None` if thumbnail and label are fully inside `offset..offset+view_height`; otherwise the least-moved offset that shows them (top at `top − COLUMN_MARGIN` when scrolling up, bottom at `label bottom + COLUMN_MARGIN` when scrolling down); if they don't fit, centred on the thumbnail. Clamped to `0..=max(content − view_height, 0)`.

- [ ] **Step 1: Write the failing tests** in `column.rs`:
  - `thumbs_fill_width_and_stack`: 3 Letter pages, width 260 → thumb width 236, heights 236·792/612, page 1 y = 12 + h0 + 32.
  - `mixed_page_sizes_share_width`: Letter + landscape A4 → same width, different heights.
  - `narrow_width_is_at_least_one_point`: width 10 → `thumb_width() == 1`.
  - `visible_pages_overlap_range`; `visible_includes_label_only_overlap`.
  - `page_at_clamps_and_finds_slot`: y in the gap after page 0's label → 0 (a slot runs from a thumb's top to the next thumb's top); y at page 1's top → 1; y beyond content → last page; negative → 0.
  - `reveal_none_when_visible`; `reveal_scrolls_down_minimally` (bottom of label + margin at view bottom); `reveal_scrolls_up_minimally`; `reveal_centres_when_too_tall`; `reveal_clamps_to_content`.
  - `empty_column_is_safe`.
- [ ] **Step 2: Run** `cargo test -p noido-core column` — Expected: compile error.
- [ ] **Step 3: Implement** `Column` (store the rects in a `Vec<PageRect>`; `visible` by linear scan or binary search, either is fine).
- [ ] **Step 4: Run** `cargo test -p noido-core` — Expected: pass.
- [ ] **Step 5: Commit** `jj commit -m "Core: thumbnail column layout"`

### Task 3: `Renderer::start_background`

**Files:**
- Modify: `crates/noido-render/src/lib.rs`
- Test: `crates/noido-render/tests/renderer.rs`

**Interfaces:**
- Produces: `Renderer::start_background(engine: Arc<dyn PdfEngine>, bytes: PdfBytes, waker: Box<dyn Fn() + Send>) -> Renderer` — returns at once; the thread opens the document itself and then serves requests exactly as `start`'s does. If opening fails the thread ends; requests are dropped and nothing arrives. `start` keeps its blocking behaviour and signature, sharing the thread body.

- [ ] **Step 1: Write the failing tests**: `background_start_renders_requested_page` (request page 0 of the fixture at scale 0.5, `recv` it — same helper as `renders_requested_page_and_wakes`); `background_start_on_garbage_delivers_nothing` (garbage bytes; request page 0; nothing arrives within 200 ms; no panic).
- [ ] **Step 2: Run** `cargo test -p noido-render background` — Expected: compile error.
- [ ] **Step 3: Implement**, factoring the shared thread spawn (open, then `run`) so `start` passes an "opened" channel and `start_background` doesn't.
- [ ] **Step 4: Run** `cargo test -p noido-render` — Expected: pass.
- [ ] **Step 5: Commit** `jj commit -m "Renderer: start without waiting for the document to open"`

### Task 4: `SidebarView` — the column, low-res fallback and sharp renders

**Files:**
- Create: `crates/frontends/gui-egui/src/image.rs` — move `PageImage` and `page_uv` here from `app.rs` unchanged (`pub(crate)`).
- Modify: `crates/frontends/gui-egui/src/sidebar.rs` (replace the placeholder), `app.rs` (own a `SidebarView`, pass inputs, tests), `main.rs` (`mod image;`)

**Interfaces:**
- Consumes: `Column` (Task 2), `Renderer::start_background` (Task 3).
- Produces in `sidebar.rs`:
  - `pub struct SidebarView` with `SidebarView::new(page_count: usize) -> Self` (no renderer: draws low-res/white only) and `with_renderer(self, renderer: Renderer) -> Self`.
  - `fn receive(&mut self, ctx: &egui::Context, layout: &Layout)` — takes in sharp renders (as `receive_renders` does: insert/mark ready; on `Err` remove the sharp image, mark failed, print as the page view does).
  - `fn show(&mut self, ui: &mut egui::Ui, state: &mut SidebarState, input: &Input) -> Output` — draws the resizable panel (as today) holding a vertical `ScrollArea` over a `Column` laid out at the panel's inner width; records `state.width`.
  - `pub struct Input<'a> { pub layout: &'a Layout, pub lowres: Option<&'a HashMap<usize, PageImage>>, pub highlighted: Range<usize>, pub current: Option<usize> }` (the last two used from Task 5; pass `0..0`/`None` now).
  - `pub struct Output { pub clicked: Option<usize>, pub browsing: Option<usize> }` (filled in Task 5).
  - `fn closed(&mut self)` — drops all sharp images, forgets the tracker, sets the renderer's keep to `0..0`.
  - `fn replace(&mut self, renderer: Renderer, page_count: usize)` — for reloads (Task 6): new renderer and tracker; sharp images kept below `page_count`.
  - `#[cfg(test)]` accessors: `drawn(&self) -> &[(usize, Rect)]` (visible thumbnails' screen rects last frame), `sharp_pages(&self) -> Vec<usize>`.
- Each frame while open: visible = `column.visible(offset, offset+height)`; `set_keep(visible)`, `set_priority(visible)`, request via the tracker at the scale in Global Constraints, `forget_outside(visible)` and drop sharp images outside it. Draw each visible thumbnail: image or white, a 1 pt outline (`ui.visuals().widgets.noninteractive.bg_stroke`), and the page number (`page + 1`) centred in its label row.
- `app.rs`: field `sidebar_view: SidebarView`; `show` calls `sidebar_view.receive` each frame, `sidebar_view.show` when open, `sidebar_view.closed()` on the frame the sidebar is closed (open → closed transition). `with_sidebar_renderer(self, Renderer) -> Self`. The harness gets `build_sidebar(engine)` which adds a sidebar renderer from `start_background`, and a `RecordingEngine(Arc<Mutex<Vec<(usize, f32)>>>)` (BlankDocument pages, records every `render_tile` page and scale).

- [ ] **Step 1: Write the failing tests** in `app.rs`:
  - `sidebar_shows_a_thumbnail_per_visible_page`: 10 pages, sidebar open; `drawn()` pages are consecutive from 0, each rect width == `Column::thumb_width` for the panel's inner width (±0.5), height/width == 792/612.
  - `sidebar_shows_pages_not_spreads_in_dual_mode`: toggle dual; same widths, one rect per page.
  - `sidebar_requests_only_visible_pages_at_its_width`: recording engine; after `until` renders arrive, recorded pages == `drawn()` pages; each scale == `thumb_width × pixels_per_point / 612` (±1 %).
  - `widening_sidebar_requests_larger_scale`: drag the panel's right edge (x = `h.app.sidebar.width`, mid-height) 100 pt right with `h.drag`; `h.app.sidebar.width` grew and a new recorded scale > the old one.
  - `sidebar_rerequests_on_pixels_per_point_change`: set `ctx.set_pixels_per_point(2.0)`; a new request at about twice the scale.
  - `sidebar_drops_sharp_images_scrolled_out`: wheel over the sidebar to the end; `sharp_pages()` ⊆ `drawn()` pages and doesn't contain 0.
  - `closing_sidebar_drops_sharp_images`: F9 twice → `sharp_pages()` empty.
  - `sidebar_draws_lowres_without_sharp`: no sidebar renderer, a low-res texture inserted for page 0 (as `thumbnail_shown_when_page_not_rendered` does); an image mesh with that texture id lies inside `drawn()[0]`.
- [ ] **Step 2: Run** `cargo test -p gui-egui sidebar` — Expected: compile errors.
- [ ] **Step 3: Implement** `image.rs` move, then `SidebarView`.
- [ ] **Step 4: Run** `cargo test --workspace` and clippy — Expected: pass, including the existing sidebar tests (`f9_opens_sidebar_and_page_view_narrows`, `restored_sidebar_width_is_clamped`, toggle tests).
- [ ] **Step 5: Commit** `jj commit -m "Sidebar: a column of page thumbnails, sharp where visible"`

### Task 5: Current page — click, highlight, follow, thumbnailer focus

**Files:**
- Modify: `crates/frontends/gui-egui/src/sidebar.rs`, `app.rs`

**Interfaces:**
- Consumes: `View::go_to_page` (Task 1), `Column::reveal`/`page_at` (Task 2), `SidebarView` (Task 4).
- Produces:
  - `NoidoApp.chosen: Option<usize>`; `fn current_page(&self) -> Option<usize>` = `chosen` else `view.centre_page()`. `update_focus` follows `current_page()`.
  - `fn reader_moved(&mut self)` clears `chosen`. Called from: `apply` for every action handled by `View::apply`; `handle_input` when a wheel step flips or zoom ≠ 1; `sync_from_egui` when egui's offset differs from the offset noido told egui last frame (new field, set in `show` where `wanted` is computed) by more than `OFFSET_EPSILON` **and** the inner rect's size equals the previous frame's.
  - Wheel flips only when the pointer is over the page view (`last_scroll.inner` contains `hover_pos`).
  - `Input.highlighted`: dual → `layout.spread_pages(layout.spread_of(current))`, else `current..current+1`. `Input.current = current_page()`.
  - `SidebarView` follows: when `input.current` differs from the page it last followed, or the sidebar just opened, or its width changed, it sets the scroll offset to `column.reveal(current, offset, height)` (if `Some`). Highlight: a rounded rect in `ui.visuals().selection.bg_fill`, 4 pt outside each highlighted thumbnail, drawn behind it; the number in `selection.stroke.color`.
  - Each thumbnail's rect senses clicks; `Output.clicked = Some(page)`. App: `view.go_to_page(page)`, `chosen = Some(page)`; the sidebar marks that page as followed (no scroll).
  - `Output.browsing = Some(column.page_at(offset + height / 2))` on frames where the sidebar's offset changed without noido setting it and its width didn't change; else `None`. App: `browse = Some(page)` from it; cleared when `current_page()` changes. The thumbnailer focus is `browse.unwrap_or(current)` (direction as today, from consecutive focus values).

- [ ] **Step 1: Write the failing tests** in `app.rs`:
  - `click_on_thumbnail_goes_to_page`: click the centre of `drawn()` for page 2 → `centre_page()` is 2 or the page view's top page is 2 (`offset().1 == page_rect(2).y − MARGIN`); zoom mode unchanged.
  - `click_keeps_paged_and_dual`: paged + dual; click page 5 → `current_spread() == Some(spread_of(5))`, still paged and dual.
  - `click_on_short_page_highlights_it`: fit page + continuous (pages much shorter than the view); click page 3; `current_page() == Some(3)` although `centre_page() != Some(3)`; the highlighted range is `3..4`.
  - `click_on_last_page_highlights_it`: 10 pages; click page 9 → `current_page() == Some(9)`.
  - `scrolling_page_view_clears_chosen_page`: after a click, wheel over the page view → `chosen == None`.
  - `page_key_clears_chosen_page`: after a click, PageDown → `chosen == None`.
  - `resize_keeps_chosen_page`: after a click, change `h.size` by 100×100 and run frames → `chosen == Some(page)`.
  - `dual_mode_highlights_both_pages_of_spread`.
  - `sidebar_follows_current_page`: 30 pages, sidebar open, go to page 20 by PageDown → page 20's rect is in `drawn()` and fully inside the sidebar's scroll area.
  - `hand_scrolled_sidebar_stays_until_page_changes`: wheel the sidebar to the end; frames pass; the visible pages stay at the end; PageDown → the sidebar shows the current page again.
  - `browsing_sidebar_moves_thumbnailer_focus`: 200-page `SizedEngine` with a thumbnailer; wheel the sidebar far down; the thumbnailer's `sent` focus ≥ 100.
  - `wheel_over_sidebar_does_not_flip`: paged fit page; wheel over the sidebar → `current_spread()` unchanged.
- [ ] **Step 2: Run** `cargo test -p gui-egui -- click chosen follows browsing wheel_over highlights` — Expected: failures/compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` and clippy — Expected: pass.
- [ ] **Step 5: Commit** `jj commit -m "Sidebar: click to go, highlight and follow the current page"`

### Task 6: Wiring — startup and reload

**Files:**
- Modify: `crates/frontends/gui-egui/src/main.rs`, `watch.rs`, `app.rs`

**Interfaces:**
- Consumes: `Renderer::start_background`, `SidebarView::replace`, `NoidoApp::with_sidebar_renderer`.
- Produces: `Reload` gains `pub sidebar: Renderer`; `open_new` starts it with `start_background` after the page count check. `main.rs` starts it next to the thumbnailer and passes it via `with_sidebar_renderer`. `NoidoApp::reload` calls `sidebar_view.replace(reload.sidebar, page_count)` and clears `chosen` and `browse`. The harness `reload` helper builds the sidebar renderer too.

- [ ] **Step 1: Write the failing test** `reload_swaps_sidebar_renderer`: sidebar open with a recording engine A; reload with recording engine B (12 pages → 5 pages via `SizedEngine`-style recording); after `until`, B has recorded requests, sharp pages are all < 5, and `chosen` is `None`.
- [ ] **Step 2: Run** — Expected: compile error (`Reload` has no `sidebar`).
- [ ] **Step 3: Implement.** Also remove the old placeholder constant and update `sidebar.rs`'s module doc.
- [ ] **Step 4: Run** `cargo test --workspace`, clippy, `cargo build --release` — Expected: pass.
- [ ] **Step 5: Smoke test** the release binary with a scratch `XDG_STATE_HOME` whose saved state has the sidebar open (run once, quit, set `open = true` in the state file; no synthetic input): screenshot with `import -window "<name> — noido"`; thumbnails show, sharp, and the current page is highlighted.
- [ ] **Step 6: Commit** `jj commit -m "Sidebar renderer at startup and on reload"`
