# Reflow and places (L3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The reader's place survives a reflow. The view, Back/Forward,
pins and the pagelet hold a `Place`, and across a reflow the first
character in view keeps its height on screen. PDF behaves and saves
exactly as before.

**Architecture:** `LocationMap`, a value in `noido-engine`, says where
each page's characters sit in the parts, line by line. The engine hands
it over beside the page sizes, and the window keeps it. `Place` in
`noido-core` is a page point (fixed pages) or a `Location` (reflowable
text). `View::reflow` takes the old and new maps and new page sizes and
puts the anchor line back at its height. Nothing reflows for real yet,
so a fake reflowable document in the core tests drives it.

**Tech Stack:** Rust 2024, serde/TOML.

**Spec:** `docs/superpowers/specs/2026-11-15-unified-locations-design.md`

## Scope against the spec

L3 is the core and the saved forms. Three things wait for the first
reflowable engine (EPUB or markdown), which brings their first caller:

- **The render thread laying out again on request** and sending back
  new sizes and a new map. Nothing asks for a reflow until virtual
  paper settings (E3) or the lock toggle (M2) exist.
- **The layout key's contents.** The key names the paper, text,
  margins, theme and lock settings, none of which exist yet. L3 stores
  and compares it as an opaque string.
- **Re-anchoring in reflowable parts.** The text thread's anchoring
  reads PDF pages as parts. It is left alone.

## Rulings to agree before starting

1. **A map's runs are lines, with their height on the page.** The spec
   says "per page, runs of characters, each starting at a `Location`".
   L3 makes each run one line and gives it `top` and `bottom` in page
   space. Then "first character in view" and "put its line at the same
   height" are answered from the map alone. The alternative is waiting
   for the new page's text from the text thread, which arrives a frame
   or more later and makes the view jump twice. Task 1 writes this into
   the spec.
2. **`ViewState` loses `Copy`**, since the layout key is a `String`.
3. **A history entry made in reflowable text is `Text`, the first
   character in view.** Going to a `Text` place puts its line at the top
   of the view. A `Point` place is gone to as today.

## Global Constraints

- PDF: view, history, pins, pagelet and highlights save and load
  byte-for-byte as before this change; old state files read the same.
- `Place` is saved as `page`, `x`, `y` for `Point` and as `part`, `char`
  for `Text`.
- A pin, and the pagelet's page, is saved as a bare number when it is a
  page and as a table `{ part, char }` when it is `Text`.
- The view of a reflowable document adds `anchor_part`, `anchor_char`
  and `layout` next to `page`, `x`, `y`. All three are absent for PDF.
- A pinned PDF page is `Point { part, x: 0.0, y: 0.0 }`. A pin in
  reflowable text is the first character of the page that was showing;
  the pin list shows its current page number.
- Across a reflow: the line holding the first character in view goes to
  the same height on screen if the new layout allows, otherwise to the
  top of the view. In a fit-page mode that shows the page holding it.
- TDD for every step; `just check` clean before each commit.
- Commits: `jj commit -m "<crate>: …"`; never move `main`.

## Review Focus

1. **The reflow changes the page count**, so a spread index, a pinned
   page or a history page past the new end must not panic or point off
   the end. Test in Task 4 (reflow to fewer pages) and Task 3 (pins on
   pages past the end).
2. **A view with nothing in it** (zero-size viewport, no pages, an
   empty document) asked for its place or reflowed: `None`, no panic,
   and the anchor applied when the viewport gets a size, as
   `View::pending` does today. Test in Task 4.
3. **The anchor character is at the very end of the text** (the last
   line of the last page) and the new layout can't put it at the same
   height because the document ends: it goes as far as clamping allows,
   still in view. Test in Task 4.
4. **A saved view whose layout key matches but whose page is gone**
   (the file shrank): restore through the anchor, not a clamped point on
   the wrong page. Test in Task 5.
5. **Dual mode**: "first in reading order" is the left page before the
   right, top to bottom. Test in Task 4.

---

### Task 1: Spec amendment and `LocationMap`

**Files:**
- Modify: `docs/superpowers/specs/2026-11-15-unified-locations-design.md`
  ("Mapping: a value the window holds")
- Create: `crates/noido-engine/src/locations.rs`
- Modify: `crates/noido-engine/src/lib.rs`, `crates/noido-engine/src/engine.rs`

**Interfaces:**
- Produces:
  ```rust
  /// One line of a reflowed page: `chars` characters from `start`,
  /// between `top` and `bottom` in page space.
  pub struct Run { pub start: Location, pub chars: usize, pub top: f32, pub bottom: f32 }
  pub enum LocationMap { Identity, Runs(Vec<Vec<Run>>) }   // per page, in reading order
  impl LocationMap {
      pub fn identity() -> Self;
      pub fn reflows(&self) -> bool;                          // Runs
      pub fn location(&self, page: usize, char: usize) -> Option<Location>;
      pub fn page_char(&self, at: Location) -> Option<(usize, usize)>;
      pub fn line(&self, at: Location) -> Option<(usize, Run)>; // page and the run holding it
      pub fn first_from(&self, page: usize, y: f32) -> Option<(Location, f32)>; // first run with bottom > y: its start and top
      pub fn page_start(&self, page: usize) -> Option<Location>;
  }
  // on Document:
  fn locations(&self) -> LocationMap { LocationMap::identity() }
  ```
  `Clone, Debug, PartialEq` on both types. `Identity` answers
  `location(p, c) = Location { part: p, char: c }`, `page_char` the
  reverse, and `None` from `line`, `first_from`. `page_start(p)` is
  `{ part: p, char: 0 }`. No `reflows()` on `Document`: the map says it.

- [ ] **Step 1: Amend the spec.** In "Mapping", say each run is one line
  with its `top` and `bottom` on the page, so the window answers "first
  character in view" and "where is its line" without page text; drop
  the separate `reflows()` method for `LocationMap::reflows()`; add the
  three L3 scope notes above (render-thread reflow, layout key's
  contents, re-anchoring in reflowable parts wait for the first
  reflowable engine). Commit `Spec: lines in the location map, and what L3 leaves for the first reflowable engine`.
- [ ] **Step 2: Failing tests** in `locations.rs`:
  `identity_is_page_and_char` (round trip on `(3, 17)`),
  `runs_map_both_ways` (two pages, part 0 continuing across them:
  page 1's first run starts at `{0, 120}`; `location(1, 5) == {0, 125}`,
  `page_char({0, 125}) == (1, 5)`),
  `a_location_between_runs_is_on_the_later_one` (the end of one line is
  the start of the next: `{0, 40}` is page 0 line 2 if line 1 has 40),
  `past_the_end_is_none`,
  `first_from_skips_lines_above` (`first_from(0, y)` with `y` inside line
  2 gives line 2's start and top; below the last line gives `None`).
- [ ] **Step 3:** Run `cargo test -p noido-engine locations`; expect
  compile failure.
- [ ] **Step 4:** Implement. `page_char` and `line` binary-search the
  pages by their first run's start, then the runs.
- [ ] **Step 5:** `just check`; commit `engine: a location map from pages to parts, line by line`.

### Task 2: `Place`, and Back/Forward over it

**Files:**
- Create: `crates/noido-core/src/place.rs`
- Modify: `crates/noido-core/src/history.rs`, `crates/noido-core/src/lib.rs`

**Interfaces:**
- Consumes: `LocationMap` (Task 1).
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum Place { Point(DocPoint), Text(Location) }
  impl Place {
      pub fn page(&self, map: &LocationMap) -> Option<usize>;
      /// Where the view goes for it: the point itself, or the left edge of
      /// its line's top.
      pub fn point(&self, map: &LocationMap) -> Option<DocPoint>;
  }
  ```
  `Point` holds a `DocPoint` (not separate fields) so existing callers
  wrap rather than rebuild. `History` keeps its API with `Place` in
  place of `DocPoint`: `from_parts(Vec<Place>, Vec<Place>)`,
  `leave(Place)`, `back(Place) -> Option<Place>`,
  `forward(Place) -> Option<Place>`, `map_points(page, f)` touching only
  `Point` entries on `page`, `back_entries()`/`forward_entries()`
  returning `&[Place]`.

- [ ] **Step 1: Failing tests.** In `place.rs`: `a_point_is_its_own_page`,
  `text_goes_to_its_line_top` (with a two-page `Runs` map: `Text({0,125})`
  → `DocPoint { page: 1, x: 0.0, y: <line top> }`),
  `text_past_the_end_has_no_page`. In `history.rs`: the existing tests
  ported to `Place::Point`, plus `map_points_leaves_text_alone` and
  `leave_twice_at_the_same_text_keeps_one`.
- [ ] **Step 2:** Run `cargo test -p noido-core place history`; expect
  compile failure.
- [ ] **Step 3:** Implement. Fix the callers in `noido-core`,
  `noido-state` and `gui-egui` by wrapping in `Place::Point` and going
  through `Place::point(&map)`; the app's map is
  `LocationMap::identity()` until Task 6. `noido-state` saves history
  in Task 5; until then it saves `Point` entries and skips `Text` ones.
- [ ] **Step 4:** `just check`; commit `core: Back and Forward keep places, a point or a character`.

### Task 3: Pins over `Place`

**Files:**
- Modify: `crates/noido-core/src/pins.rs`, its callers in
  `crates/frontends/gui-egui/src/app.rs` and `sidebar.rs`

**Interfaces:**
- Consumes: `Place`, `LocationMap`.
- Produces:
  ```rust
  pub struct Pins(Vec<Place>);   // in document order, one per page
  impl Pins {
      pub fn from_places(places: impl IntoIterator<Item = Place>, map: &LocationMap) -> Self;
      pub fn places(&self) -> &[Place];
      /// The pinned pages now, in order, without repeats.
      pub fn pages(&self, map: &LocationMap) -> Vec<usize>;
      pub fn contains(&self, page: usize, map: &LocationMap) -> bool;
      /// Pins `page`: `Point { page, 0, 0 }` for an identity map, `Text` of
      /// the page's first character otherwise. Unpins every pin on it.
      pub fn toggle(&mut self, page: usize, map: &LocationMap) -> bool;
      pub fn remove(&mut self, page: usize, map: &LocationMap);
      pub fn retain_below(&mut self, page_count: usize, map: &LocationMap);
  }
  ```
  Order is by `(part, char)`, a `Point` counting as `(page, 0)`.

- [ ] **Step 1: Failing tests:** the existing pin tests ported to an
  identity map; `a_pin_in_text_is_the_page_start`;
  `a_text_pin_follows_its_character_to_another_page` (pin under one map,
  `pages()` under a second map where that character is on page 3 gives
  `[3]`); `two_text_pins_on_one_page_show_once` (after a reflow merges
  them; toggling that page then removes both);
  `pins_past_the_end_are_dropped`.
- [ ] **Step 2:** Run `cargo test -p noido-core pins`; expect compile
  failure.
- [ ] **Step 3:** Implement; fix callers with the app's identity map.
  `noido-state` keeps saving numbers for `Point` pins until Task 5.
- [ ] **Step 4:** `just check`; commit `core: pins keep places, so a pin in text follows its page`.

### Task 4: The view's place, and the reflow event

**Files:**
- Modify: `crates/noido-core/src/view.rs`
- Create: `crates/noido-core/src/reflow_tests.rs` (`#[cfg(test)]`
  module, with the fake reflowable document)

**Interfaces:**
- Consumes: `Place`, `LocationMap`, `Run`.
- Produces:
  ```rust
  impl View {
      /// Where the reader is: the centre point for an identity map, the
      /// first character in view (reading order) otherwise.
      pub fn here(&self, map: &LocationMap) -> Option<Place>;
      /// The first character in view and its line top's height in the
      /// viewport, in UI points from the viewport's top.
      pub fn first_in_view(&self, map: &LocationMap) -> Option<(Location, f32)>;
      /// New page sizes and map from a reflow, keeping the first
      /// character in view at its height.
      pub fn reflow(&mut self, sizes: Vec<Size>, old: &LocationMap, new: &LocationMap);
      /// Goes to `place`: as `restore_place` for a point, its line at the
      /// top of the view for text.
      pub fn go_to_place(&mut self, place: &Place, map: &LocationMap);
  }
  ```
  `Layout` gains `replace_pages(sizes: Vec<Size>)` for a new page count,
  keeping its mode, the paged spread recomputed from the anchor's page.
  A zero viewport keeps the anchor pending, as `pending` does for a
  point.

  The fake: `fn flow(chars: usize, page: Size, line: f32, per_line: usize) -> (Vec<Size>, LocationMap)`,
  one part, `per_line` characters per line, lines of height `line`
  from a top margin of 36, as many lines per page as fit above a bottom
  margin of 36. A wider page gets a larger `per_line`.

- [ ] **Step 1: Failing tests** in `reflow_tests.rs`:
  - `the_fake_keeps_locations_across_a_page_size_change` (every location
    present in one flow maps to some page in the other).
  - `first_in_view_is_the_first_line_below_the_viewport_top` (scrolled
    so line 3 of page 2 is cut at the top: gives line 3's start when its
    bottom is below the viewport top, line 4's otherwise).
  - `reflow_keeps_the_line_at_its_height` (continuous, custom zoom:
    first in view at height `h` before; after reflowing to a narrower
    page, `first_in_view` gives a line holding the same character at
    `h ± 0.5`).
  - `reflow_to_fewer_pages_does_not_panic` and keeps the character in
    view.
  - `reflow_at_the_end_of_the_text_keeps_it_in_view` (Review Focus 3).
  - `reflow_in_fit_page_shows_the_page_holding_it` (paged, fit-page).
  - `reflow_in_dual_reads_left_before_right` (Review Focus 5).
  - `reflow_without_a_viewport_waits_for_one` (Review Focus 2).
  - `here_on_an_identity_map_is_the_centre_point`, equal to
    `Place::Point(view.place().unwrap())`.
  - `going_to_text_puts_its_line_at_the_top`.
- [ ] **Step 2:** Run `cargo test -p noido-core reflow`; expect compile
  failure.
- [ ] **Step 3:** Implement. `first_in_view`: visible pages in page
  order (left before right in a spread), first whose `first_from` at the
  viewport top's page y gives a run with top above the viewport bottom.
  `reflow`: take `first_in_view(old)`, set sizes, arrange, find the
  character's line in `new`, set the offset so its top is at the same
  height; if clamping moves it off that height, put it at the top
  instead, then clamp.
- [ ] **Step 4:** `just check`; commit `core: across a reflow, the first character in view keeps its height`.

### Task 5: Saved places

**Files:**
- Modify: `crates/noido-state/src/lib.rs`, `crates/noido-core/src/view.rs`
  (`ViewState`), and `crates/frontends/gui-egui/src/app.rs` (the
  pagelet's `page`)

**Interfaces:**
- Consumes: `Place`, `Pins`, `History`, `View::go_to_place`.
- Produces:
  - `ViewState` gains `anchor: Option<Location>` and
    `layout: Option<String>`; it loses `Copy`.
  - `View::restore(&mut self, state: &ViewState, layout: Option<&str>, map: &LocationMap)`:
    the centre point when `state.layout == layout` and its page exists,
    else `go_to_place(Text(anchor))` when there is an anchor, else the
    point as today.
  - `Pagelet::page: Option<Place>`.
  - The private TOML struct `Place` in `noido-state` is renamed
    `StoredView` to free the name.

- [ ] **Step 1: Failing tests** in `noido-state`:
  - `a_pdf_state_file_reads_and_writes_back_the_same` (an existing state
    file with history, pins and a pagelet: loaded and saved, the TOML
    text is unchanged apart from `saved`).
  - `text_places_round_trip` (history entries, pins and the pagelet's
    page as `Text`: written as `part`/`char` and tables, read back
    equal).
  - `a_view_anchor_round_trips` (`anchor_part`, `anchor_char`, `layout`).
  - `old_history_entries_with_view_fields_still_read`.
  In `noido-core`: `restore_uses_the_point_when_the_layout_matches`,
  `restore_uses_the_anchor_when_the_layout_differs`,
  `restore_uses_the_anchor_when_the_page_is_gone` (Review Focus 4).
- [ ] **Step 2:** Run `cargo test -p noido-state -p noido-core restore round_trip`;
  expect failures.
- [ ] **Step 3:** Implement with untagged serde enums: a history entry
  is `{ page, x, y }` or `{ part, char }`; a pin and the pagelet's page
  are a number or `{ part, char }`. Point variants first, so old
  entries with extra fields still read as points.
- [ ] **Step 4:** `just check`; commit `state: save places in text for history, pins, the pagelet and the view`.

### Task 6: The window holds the map

**Files:**
- Modify: `crates/noido-render/src/lib.rs` (`DocumentInfo`),
  `crates/noido-render/src/transformed.rs` (pass `locations()` through),
  `crates/noido-core/src/highlights.rs`,
  `crates/frontends/gui-egui/src/app.rs`, `select.rs`, `sidebar.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `DocumentInfo::locations: LocationMap`, from `Document::locations()`
    at open; the app keeps it beside the page sizes and passes it where
    Tasks 2–5 took the identity map.
  - `Highlight::new(start: Location, end: Location, colour)`; the window
    converts carets with `map.location(page, char)` when a highlight is
    made.
  - `Highlight::on_page(&self, page: usize, char_count: usize, map: &LocationMap)`
    and `Highlight::pages(&self, map: &LocationMap)` convert through the
    map.
  - The window saves `view.here(&map)`'s `Text` character as the view's
    anchor, and `None` as the layout key, for a reflowable map.

- [ ] **Step 1: Failing tests:**
  - In `highlights.rs`: `a_highlight_draws_where_the_map_puts_it` (a
    `Runs` map where part 0 char 125 is page 1 char 5).
  - In the app's harness: `a_pdf_view_history_pins_and_pagelet_restore_as_before`
    (open, move, pin, jump, open the pagelet, close and reopen with the
    same store: the same view, Back target, pins and pagelet).
- [ ] **Step 2:** Run the two; expect failures.
- [ ] **Step 3:** Implement. No reflow request or reflow message on the
  render thread (see Scope).
- [ ] **Step 4:** `just check`; `just build`; commit `egui: keep the document's location map, and draw and save through it`.

### Final

- [ ] Whole-branch review; fix Critical and Important findings
  test-first; minors to the user.
- [ ] Release build. By hand: open a PDF with highlights, pins, history
  and the pagelet from before the branch. All come back as they were.
  Close and diff the state file against a copy taken before: unchanged.
