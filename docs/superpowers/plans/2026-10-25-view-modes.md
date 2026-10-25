# Page View Modes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Continuous/paged (`c`) and single/dual (`d`) view modes with paged PageUp/PageDown flipping at page edges.

**Architecture:** `Layout` arranges rows (one page or a spread); a `LayoutMode` says single/dual and, for paged mode, which spread is shown. `View` owns the toggles and the paged navigation rules. The frontend only maps the new keys.

**Tech Stack:** as before.

**Spec:** `docs/superpowers/specs/2026-10-25-view-modes-design.md`

## Global Constraints

- `noido-core` stays free of egui.
- Defaults: continuous, single. `MARGIN`, `PAGE_GAP` (also the gap between the two pages of a spread), zoom constants unchanged.
- "At the edge" means within 0.5 UI points of the top/bottom offset limit.
- fmt, clippy `-D warnings`, `cargo test --workspace` pass after every task.

## Review Focus

1. **Odd page count in dual mode**: the last spread has one page; no index past the end, flips stop there. → `odd_last_page_takes_left_slot`, `paged_flip_stops_at_ends`.
2. **Flip at the first/last spread**: PageUp at the top of spread 0 and PageDown at the bottom of the last spread do nothing. → `paged_flip_stops_at_ends`.
3. **Zero pages with every mode toggled**: no panic. → `zero_pages_is_safe` extended with the toggles.
4. **Mode toggle in a fit mode**: the scale refits to the new arrangement (dual fit width halves roughly). → `toggle_dual_refits_fit_width`.
5. **Pages of different sizes in one spread**: rows use the taller page's height and both pages' widths. → `spread_of_mixed_sizes`.

---

### Task 1: Rows and spreads in Layout

**Files:** Modify `crates/noido-core/src/layout.rs`.

**Interfaces — Produces:**
- `#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub struct LayoutMode { pub dual: bool, pub paged: Option<usize> }` (`paged` = spread shown in paged mode).
- `Layout::set_mode(&mut self, mode: LayoutMode)` (takes effect at the next `arrange`); `Layout::mode() -> LayoutMode`.
- `Layout::spread_count() -> usize`, `Layout::spread_of(page: usize) -> usize`, `Layout::spread_pages(spread: usize) -> Range<usize>` (clamped to page count).
- `Layout::spread_size(spread: usize) -> Option<Size>`: in PDF points, sum of widths (the left page's width counted twice for an odd last page in dual mode) and the max height. Gap excluded.
- `Layout::widest_spread() -> f32` replaces `widest_page()` for fitting (keep `widest_page` only if still used; remove otherwise).
- `Layout::spread_gap(spread) -> f32`: UI points of gap inside the row (`PAGE_GAP` in dual, 0 in single).
- `arrange(scale, available_width)` lays out shown rows only: content width = `max(available_width, max_shown_row_width + 2*MARGIN)` where row width = `spread_width*scale + spread_gap`. Row is centred; pages inside it side by side, each vertically centred within the row height; odd last page in the left slot.
- `page_rect(page)` → `None` for pages not shown. `visible_pages` returns pages of shown rows intersecting the span. `doc_point`: row by y, then page by x (last page whose left ≤ x, else the row's first page). `page_count`, `page_size`, `content_*`, `content_point` as before.

- [ ] Failing tests (two 612×792 pages + one 306×396 unless noted, scale 1):
  - `single_mode_unchanged`: default mode reproduces the existing `arranges_at_scale` rects.
  - `dual_rows_side_by_side`: pages `[612×792; 4]`, dual, `arrange(1.0, 1260)` → page 0 at x 12, page 1 at x 12+612+12, same y 12; page 2 at y 12+792+12; content width 1260.
  - `odd_last_page_takes_left_slot`: 3 Letter pages, dual → page 2 x == page 0 x; `spread_count() == 2`; `spread_pages(1) == 2..3`.
  - `spread_of_mixed_sizes`: pages `[612×792, 306×396]`, dual → `spread_size(0) == Size{918, 792}`; page 1 vertically centred: y == 12 + (792−396)/2.
  - `paged_shows_one_spread`: 4 pages, single, `paged: Some(2)` → only page 2 has a rect, at y 12; `content_height == 12+792+12`; `visible_pages(0, 1e6) == 2..3`.
  - `doc_point_picks_page_in_row`: dual, point in the right page → page 1, x relative to page 1's left.
  - `spread_mapping`: dual: `spread_of(3) == 1`; single: `spread_of(3) == 3`; empty layout: `spread_count() == 0`.
- [ ] Run `cargo test -p noido-core`: FAIL. Implement. PASS. Keep `View` compiling (its fit-width uses `widest_spread` plus the in-row gap). Commit "Layout arranges rows of pages and spreads".

### Task 2: Modes and paged navigation in View; keys

**Files:** Modify `crates/noido-core/src/view.rs`, `crates/noido-core/src/keymap.rs`.

**Interfaces — Produces:**
- `Key::C`, `Key::D`; `Action::ToggleContinuous`, `Action::ToggleDual`; native bindings plain `c` → ToggleContinuous, plain `d` → ToggleDual.
- `View::continuous() -> bool`, `View::dual() -> bool`, `View::current_spread() -> Option<usize>` (Some in paged mode).
- Fit width: `(viewport_w − 2*MARGIN − gap_of_widest_row) / widest_spread`. Fit page: spread at the centre (continuous) or the current spread (paged): `min((vw − 2M − gap)/spread_w, (vh − 2M)/spread_h)`.
- Toggle: centre page found first; then switch mode; paged → show that page's spread from its top (offset.y = 0, x clamped); continuous → keep the centre doc point at the centre. Fit modes refit.
- Paged PageDown: at bottom (within 0.5) → next spread at offset.y 0 if it exists; else `scroll_target`. PageUp mirrored, landing at the bottom.

- [ ] Keymap failing tests: `plain_c_and_d_toggle_modes`, `c_with_mod_does_nothing`.
- [ ] View failing tests (10 Letter pages, viewport 636×800 unless noted):
  - `toggle_continuous_shows_centre_page_from_top`: `scrolled_to(0, 3000)`; centre page p; ToggleContinuous → `current_spread() == Some(p)`, offset.y == 0, `continuous() == false`.
  - `toggle_back_keeps_centre_page_visible`: then ToggleContinuous again → page p's rect intersects the viewport.
  - `paged_page_down_scrolls_then_flips`: paged at spread 0 (fit width, page 792 tall + margins → content 816, max y 16): PageDown → y 16 (partial fit to bottom), spread 0; PageDown → spread 1, y 0.
  - `paged_page_up_flips_to_bottom`: at spread 1, y 0: PageUp → spread 0, y == max y.
  - `paged_flip_stops_at_ends`: spread 0 at top: PageUp → unchanged; last spread at bottom: PageDown → unchanged.
  - `paged_zoomed_scrolls_screenfuls`: paged, ZoomIn ×3 (page taller than the window) → PageDown moves by 720 within the page until the bottom, then flips.
  - `paged_scroll_clamped_to_page`: paged, `scrolled_to(0, 99_999)` → offset.y == max y of the one-spread content.
  - `toggle_dual_refits_fit_width`: fit width, ToggleDual → scale == (636 − 24 − 12) / 1224.
  - `fit_page_in_paged_dual_fits_spread`: paged + dual, viewport 1260×600, FitPage → scale == min((1260−24−12)/1224, (600−24)/792).
  - `zero_pages_is_safe` (extended): also ToggleContinuous, ToggleDual, PageDown/PageUp in paged mode → no panic.
- [ ] Run `cargo test -p noido-core`: FAIL. Implement. PASS. Commit "View modes: continuous/paged and single/dual".

### Task 3: Frontend keys and headless checks

**Files:** Modify `crates/frontends/gui-egui/src/app.rs`.

- `core_key`: map `egui::Key::C` → `Key::C`, `egui::Key::D` → `Key::D`.
- [ ] Failing headless tests (Harness, 10 pages): `c_switches_to_paged_and_page_down_flips` (press `c`; `view.current_spread() == Some(0)`; PageDown until the spread changes within 3 presses → `Some(1)`); `d_switches_to_dual` (press `d`; `view.dual()`; page 1 rect x > page 0 rect x).
- [ ] Implement; full suite; clippy. Commit "Toggle view modes with c and d".
