# Thumbnails Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pages without a full render show a stretched low-res thumbnail instead of white.

**Architecture:** Pure window/order functions in `noido-core`; a `Thumbnailer` thread in `noido-render` with its own `Document`; the frontend feeds it the focus page and draws thumbnails as the fallback.

**Spec:** `docs/superpowers/specs/2026-10-25-thumbnails-design.md`

## Global Constraints

- `noido-core` stays pure; `noido-render` may now depend on `noido-core`.
- `THUMBNAIL_BUDGET = 100`, `THUMBNAIL_WIDTH = 120.0` (pixels).
- fmt, clippy `-D warnings`, `cargo test --workspace` after every task.

## Review Focus

1. **Order at the edges and for tiny documents:** no duplicates, no page out of
   range, length `min(budget, count)`. → `order_*` tests.
2. **Re-targeting:** a new focus mid-way abandons the old order; nothing is
   rendered twice while in range. → `new_focus_retargets`, `delivered_pages_not_rendered_again`.
3. **Eviction consistency:** the frontend and the thread use the same
   `thumbnail_keep`; a returning page is rendered again. → `evicted_pages_render_again`.
4. **No panic on open failure or a zero-width page.** → `open_failure_stops_quietly`, `thumbnail_scale_zero_width`.
5. **Draw priority:** full texture over thumbnail over white. → `page_image_prefers_full_render`.

---

### Task 1: Order and window in noido-core

**Files:** new `crates/noido-core/src/thumbnails.rs`, `lib.rs`.

**Produces:** `Direction { Forward, Backward }`, `THUMBNAIL_BUDGET`,
`THUMBNAIL_WIDTH`, `thumbnail_order(count, focus, direction, budget) -> Vec<usize>`,
`thumbnail_keep(count, focus, budget) -> Range<usize>`,
`thumbnail_scale(size: Size) -> f32` (`THUMBNAIL_WIDTH / width`, 1.0 when
width isn't positive and finite). `View::centre_page() -> Option<usize>`.

- [ ] Failing tests:
  - `order_forward_bias`: (1000, 500, Forward, 7) → `[500, 501, 502, 499, 503, 504, 498]`.
  - `order_backward_bias`: (1000, 500, Backward, 7) → `[500, 499, 498, 501, 497, 496, 502]`.
  - `order_at_start_uses_one_side`: (1000, 0, Backward, 5) → `[0, 1, 2, 3, 4]`.
  - `order_at_end_uses_one_side`: (1000, 999, Forward, 4) → `[999, 998, 997, 996]`.
  - `order_full_budget_split`: (1000, 500, Forward, 100) → 66 pages > 500, 33 < 500, no duplicates.
  - `order_small_document`: (3, 1, Forward, 100) → a permutation of 0..3 starting with 1.
  - `order_zero_pages`: empty; focus past the end is clamped: (5, 99, Forward, 2) → `[4, 3]`.
  - `keep_is_clamped`: (1000, 50, 100) → 0..151; (1000, 500, 100) → 400..601; (0, 0, 100) → 0..0.
  - `thumbnail_scale_zero_width`: width 0 → 1.0; width 612 → 120/612.
  - view: `centre_page_follows_scroll` (letter view at y 3000 → the page at the centre); zero pages → None.
- [ ] Implement; `cargo test -p noido-core`. Commit "Thumbnail window and order".

### Task 2: Thumbnailer thread

**Files:** `crates/noido-render/{Cargo.toml,src/lib.rs (or src/thumbnails.rs)}`, new `tests/thumbnailer.rs`.

**Produces:** `Thumbnail { page, result }`; `Thumbnailer::start(engine, bytes,
page_sizes, waker) -> Thumbnailer` (non-blocking); `set_focus(page,
direction)`; `try_recv() -> Option<Thumbnail>`.

- [ ] Failing tests (fake engine with N pages recording the order of
  `render_tile` calls; a gate to hold renders):
  - `renders_nearest_first_from_focus`: 20 pages, focus 10 Forward → first
    renders 10, 11, 12, 9 (budget covers the whole document, all 20 arrive).
  - `new_focus_retargets`: gated; focus 0; while page 0 renders, set focus 15
    → the next render after 0 is 15.
  - `delivered_pages_not_rendered_again`: focus 5 then 6 → every page rendered
    once.
  - `evicted_pages_render_again`: 400 pages; focus 0 (all of 0..100 done),
    focus 399 (page 0 outside keep), focus 0 → page 0 rendered twice.
  - `open_failure_stops_quietly`: engine whose open fails → `set_focus` and
    `try_recv` keep working, nothing arrives, no panic.
- [ ] Implement; `cargo test -p noido-render`. Commit "Thumbnailer renders a sliding window of thumbnails".

### Task 3: Frontend

**Files:** `crates/frontends/gui-egui/src/{main.rs,app.rs}`.

- `NoidoApp::with_thumbnails(thumbnailer)`; `main` starts it with the
  document's page sizes and the same waker.
- Each frame: `centre_page()`; on change, direction from the change, then
  `set_focus`. Receive thumbnails into `thumbnails` (ignore and drop outside
  `thumbnail_keep`). Drop entries outside `thumbnail_keep` when the focus
  changes.
- `page_image(page) -> Option<&TextureHandle>`: full texture, else
  thumbnail. Drawing uses it; white when `None`.

- [ ] Failing headless tests:
  - `page_image_prefers_full_render`: insert both → the full one; only a
    thumbnail → the thumbnail.
  - `jump_shows_thumbnail_before_full_render`: full renderer gated (never
    renders), thumbnailer with BlankEngine; PageDown ×10 → the centre page
    gets a thumbnail within 1 s, and `page_image` returns it.
- [ ] Implement; suite; clippy. Commit "Show thumbnails until pages render".
- [ ] Ask the user to check by hand: holding PageDown.
