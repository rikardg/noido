# Text Selection and Copy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Select text on the pages with Shift+drag or a select tool, see it highlighted with handles and a small bar, and copy it.

**Architecture:** Geometry on one page (caret at a point, word, line, rectangles) lives on `PageText` in `noido-text`, beside the copy-text joining that reuses search's hyphen rule. The selection across pages (carets in document order, gaps between pages, pending presses) and the pure decisions (which drag, edge-scroll speed) live in `noido-core/src/selection.rs`. The text thread hands each page's `Arc<PageText>` to the window on request; the window keeps them like links, and does pointer, handles, bar, edge scrolling and drawing.

**Tech Stack:** Rust 2024, eframe/egui 0.37, nextest, jj.

**Spec:** `docs/superpowers/specs/2026-11-04-text-selection-design.md` (overview: `docs/superpowers/specs/2026-11-04-annotations-design.md`).

## Global Constraints

- TDD: each test written first and watched failing.
- `just lint` clean (clippy `-D warnings`), `just fmt`.
- Commits with `jj commit -m "<prefix>: …"`; prefixes `text`, `render`, `core`, `egui`; title ≤ 72 chars. `main` is not moved.
- Tests and examples use only generated fixtures, never `../testfiles/`.
- Positions are in page coordinates (PDF points, y down, as `PageText` gives them).
- Select tool shortcut: plain `S`. Menu check item label: `Select text`. Copy: Mod+C via `egui::Event::Copy`.
- With the tool off and no Shift, presses, clicks, double- and triple-clicks never select (user's call).
- Not saved, not part of history; cleared on reload.

## Review Focus

1. **A selection over pages whose text hasn't arrived** (fast edge scroll, or Shift+click far ahead): Copy must not silently drop those pages. It waits for their text, then copies. Test in Task 8.
2. **Zoom or window resize with a selection**: the fill, handles and bar stay on the same text. Test in Task 6.
3. **A caret at the end of a page's text** (`char == char_count`, also on a page with `char_count == 0`): no index panics in `caret_rect` or `rects`. Test in Task 1.
4. **A drag that starts on a page and leaves the window sideways** (dual spreads, zoomed in): edge scrolling also works horizontally. Test in Task 7.
5. **Reload with a selection whose pages no longer exist** (shorter new version): the selection is cleared, and nothing indexes past the end. Test in Task 6.

---

### Task 1: Geometry on a page (`noido-text`)

**Files:**
- Modify: `crates/noido-text/src/page.rs`
- Modify: `crates/noido-text/src/lib.rs` (make `testing` public behind a `testing` feature)
- Modify: `crates/noido-text/Cargo.toml` (`[features] testing = []`)
- Modify: `crates/noido-core/Cargo.toml` (`[dev-dependencies] noido-text = { workspace = true, features = ["testing"] }`)

**Interfaces:**
- Produces, on `PageText` (char indices are carets `0..=char_count`):
  - `pub fn caret_at(&self, x: f32, y: f32) -> Option<usize>`: `None` without text. Otherwise the line whose rectangle is nearest the point (distance 0 inside it; in a tie, the lower line index); then on that line the caret before the glyph under or nearest the point, or after it when the point is right of the glyph's horizontal middle. Left of the line gives its first character, right of it the caret after its last glyph (before the line's `\n`).
  - `pub fn word_at(&self, x: f32, y: f32) -> Option<Range<usize>>`: the run of `char::is_alphanumeric` / `'` / `’` / `-` around the hit character, or that single character.
  - `pub fn line_range_at(&self, x: f32, y: f32) -> Option<Range<usize>>`: the hit line's characters, without its `\n`.
  - `pub fn rects(&self, range: Range<usize>) -> Vec<Rect>`: one per line touched, the union of the glyph bounds of the range's characters on it, top and height those of the whole line.
  - `pub fn caret_rect(&self, caret: usize) -> Option<Rect>`: zero width, at the left edge of glyph `caret` or the right edge of the glyph before it (the end of a line), the line's top and height. `None` without text.
  - `pub fn line_rect_of(&self, char: usize) -> Option<Rect>` (for copy's paragraph rule, Task 2).
- `noido_text::testing` (feature `testing`): `page`, `line`, `glyph`, `glyph_along`, unchanged.

- [ ] **Step 1: Write the failing tests** in `page.rs`'s tests, on `testing::page(&["the cat sat", "on the mat"])` (glyphs 5 pt wide from x = 72; lines at baselines 100 and 114, 10 pt):
  - `caret_at_glyph_halves`: `caret_at(73.0, 97.0) == Some(0)`, `caret_at(76.0, 97.0) == Some(1)`.
  - `caret_at_left_and_right_of_line`: `caret_at(10.0, 97.0) == Some(0)`; `caret_at(500.0, 97.0) == Some(11)` (after "sat", before the `\n`).
  - `caret_at_above_and_below`: `caret_at(73.0, 0.0) == Some(0)`; `caret_at(500.0, 700.0) == Some(char_count)`.
  - `caret_at_between_lines_takes_nearest`: a point 1 pt below line 0's rectangle → on line 0.
  - `caret_at_two_columns`: page with "left col" at x 72 and "right col" at x 300 on the same baseline as separate lines (build with `testing::line`); `caret_at(310.0, 97.0)` lies in "right col".
  - `caret_at_without_text`: `PageText::new(vec![]).caret_at(0.0, 0.0) == None`.
  - `word_and_line_at`: `word_at(93.0, 97.0) == Some(4..7)` ("cat"); `line_range_at(78.0, 111.0) == Some(12..22)` ("on the mat").
  - `rects_per_line`: `rects(4..15)` gives 2 rects; the first starts at x 92 and spans to 127, the second at 72; heights equal the lines' heights.
  - `caret_rect_at_ends`: `caret_rect(0)` x == 72; `caret_rect(11)` x == 127 (after "sat"); `caret_rect(char_count)` is `Some` with x at the last glyph's right edge; on an empty `PageText`, `caret_rect(0) == None` and `rects(0..0)` is empty (no panic).
- [ ] **Step 2:** `cargo nextest run -p noido-text`: the new tests fail to compile (methods missing).
- [ ] **Step 3:** Implement the methods. Reuse `line_rects` (make it per line index, lines with no glyph skipped); line of a caret at `char_count` is the last line.
- [ ] **Step 4:** `cargo nextest run -p noido-text`: all pass. `cargo nextest run -p noido-core` still builds with the dev feature.
- [ ] **Step 5:** `jj commit -m "text: carets, words, lines and rectangles at a point on a page"`

### Task 2: Copied text (`noido-text`)

**Files:**
- Create: `crates/noido-text/src/copy.rs` (+ `mod copy; pub use copy::*;` in `lib.rs`)
- Modify: `crates/noido-text/src/search.rs` (make `line_end_hyphen` and `Hyphen` `pub(crate)`)

**Interfaces:**
- Consumes: `PageText::line_rect_of` (Task 1).
- Produces: `pub fn copy_text(pieces: &[(&PageText, Range<usize>)]) -> String`: pieces in document order. Rules:
  - A `\n` inside a piece becomes `" "`, or `"\n"` when it is a paragraph break: the step between the two lines' tops is more than 1.4 × the page's median step between consecutive lines.
  - A line-end hyphen (per `line_end_hyphen`): `Breaks` removes the hyphen and the break; `Belongs` keeps the hyphen and removes the break.
  - Runs of spaces collapse to one; the result is trimmed.
  - Between pieces (pages): a `" "`, except when the previous piece ends in a hyphen that `line_end_hyphen` would treat as `Breaks` if the next piece followed after a `\n` (the hyphen goes, no space), or `Belongs` (the hyphen stays, no space).

- [ ] **Step 1: Failing tests** in `copy.rs`:
  - `lines_join_with_spaces`: `["the cat", "sat down"]` all → `"the cat sat down"`.
  - `line_end_hyphen_rejoins`: `["inter-", "preter"]` → `"interpreter"`; `["Anglo-", "Saxon"]` → `"Anglo-Saxon"`; `["word¬", "end"]` → `"wordend"`.
  - `paragraph_gap_is_newline`: lines at baselines 100, 114, 128, 160, 174 (build with `testing::line`) → newline only between the 3rd and 4th line.
  - `pages_join`: two pieces "end of page" / "next page" → `"end of page next page"`; `"inter-"` / `"preter"` → `"interpreter"`.
  - `partial_ranges`: a range inside one line copies just those characters.
- [ ] **Step 2:** run, see them fail.
- [ ] **Step 3:** implement `copy_text`.
- [ ] **Step 4:** `cargo nextest run -p noido-text`: pass.
- [ ] **Step 5:** `jj commit -m "text: copied text joins lines and pages, rejoining hyphenated words"`

### Task 3: Page text for the window (`noido-render`)

**Files:**
- Modify: `crates/noido-render/src/text.rs`

**Interfaces:**
- Produces:
  - `TextSearcher::request_text(&self, page: usize)`.
  - New variant `SearchUpdate::Text { page: usize, text: Arc<PageText> }`.
  - The worker's cache becomes `Vec<Option<Arc<PageText>>>`. Requests are queued, and served before any search step or background extraction. A request for a page out of range is ignored. A request answered once is answered again if asked again (the window asks once per page).

- [ ] **Step 1: Failing tests** (`text.rs` tests, with the crate's existing fake document):
  - `text_is_answered_on_request`: `request_text(2)` → a `Text { page: 2, .. }` whose `text()` equals the page's text.
  - `text_request_jumps_ahead_of_search`: on a document whose `text()` is slow (gated, or counting calls), start a search, then `request_text(last)`. The `Text` update for `last` arrives before the search's `Done`.
  - `text_shares_the_search_cache`: requesting a page, then searching, extracts that page once (count `text()` calls).
- [ ] **Step 2:** run, fail.
- [ ] **Step 3:** implement (a `Command::Text(page)`; a `VecDeque<usize>` of requests, drained first in `step`; `busy()` counts it).
- [ ] **Step 4:** `cargo nextest run -p noido-render`: pass.
- [ ] **Step 5:** `jj commit -m "render: the text thread hands a page's text to the window on request"`

### Task 4: The selection model (`noido-core`)

**Files:**
- Create: `crates/noido-core/src/selection.rs` (+ `mod`/`pub use` in `lib.rs`)

**Interfaces:**
- Consumes: Task 1's `PageText` methods; `Layout`, `DocPoint`.
- Produces:
  ```rust
  /// A caret in document order: before character `char` of `page`.
  /// The boundary between pages k and k+1 is Caret { page: k+1, char: 0 }.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
  pub struct Caret { pub page: usize, pub char: usize }

  /// Where a point lands: a caret, or a point on a page whose text hasn't
  /// arrived, kept to be placed when it does.
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum Spot { At(Caret), Waiting(DocPoint) }

  /// What a page's text is, as the window knows it.
  pub enum PageTextState<'a> { Unknown, Known(&'a PageText) }

  pub fn spot_at(layout: &Layout, p: DocPoint, text: PageTextState) -> Spot;
  pub fn resolve(spot: Spot, text_of: impl Fn(usize) -> PageTextState) -> Spot;

  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Selection { pub anchor: Spot, pub focus: Spot }
  impl Selection {
      /// Both ends placed: (start, end) in document order.
      pub fn carets(&self) -> Option<(Caret, Caret)>;
      /// The selected characters on `page`, given its char count.
      pub fn on_page(&self, page: usize, char_count: usize) -> Option<Range<usize>>;
      /// Pages from start to end.
      pub fn pages(&self) -> Option<RangeInclusive<usize>>;
      /// Whether no character lies between the ends.
      pub fn is_empty(&self, text_of: impl Fn(usize) -> PageTextState) -> bool;
      /// Moves the end nearer to `spot` there (Shift+click; the anchor
      /// becomes the far end).
      pub fn extend_to(&mut self, spot: Spot);
      /// The copied text; None while a page in range is Unknown.
      pub fn text(&self, text_of: impl Fn(usize) -> PageTextState) -> Option<String>;
  }
  ```
  `spot_at` rules:
  - With the point inside the page rect (0..=width, 0..=height): `Known` with text gives `At(caret_at)`, `Known` without text gives `At(Caret { page, char: 0 })`, `Unknown` gives `Waiting(p)`.
  - Outside the page: above it (`y < 0`), or left of it (`x < 0`) within its row's height, gives `At(Caret { page, 0 })`; below or right of it gives `At(Caret { page: page + 1, 0 })`.

- [ ] **Step 1: Failing tests** (use `noido_text::testing::page` for texts; a `Layout` of 3 equal pages, continuous, and one dual):
  - `spot_inside_page_with_text`, `spot_on_page_without_text` (`At(page, 0)`), `spot_waiting_without_text_known`.
  - `spot_in_gap_is_boundary`: a point between pages 0 and 1 → `At(Caret { page: 1, char: 0 })`.
  - `spot_in_dual_gap`: between the two pages of a spread → boundary before the right page.
  - `resolve_places_waiting`: `Waiting(p)` resolves to the caret `caret_at` gives once `Known`.
  - `order_across_pages`: anchor on page 2, focus on page 0 → `carets()` start on page 0.
  - `empty_from_gap_to_gap`: anchor in the gap before page 1, focus in the gap after page 1, and page 1 has no text → `is_empty`.
  - `on_page_ranges`: a selection from page 0 char 4 to page 2 char 3 → page 0 `4..len0`, page 1 `0..len1`, page 2 `0..3`, page 3 `None`.
  - `extend_from_either_end`: a selection 10..20 on one page, `extend_to` 5 → 5..20; `extend_to` 25 → 10..25.
  - `text_waits_for_unknown_pages`: page 1 `Unknown` → `text()` is `None`; all `Known` → `Some("…")` equal to `copy_text` of the pieces.
- [ ] **Step 2:** run, fail.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** `cargo nextest run -p noido-core`: pass.
- [ ] **Step 5:** `jj commit -m "core: a text selection across pages, from points on the layout"`

### Task 5: Actions, keys, menu, drag kind, edge scroll (`noido-core`)

**Files:**
- Modify: `crates/noido-core/src/keymap.rs`, `crates/noido-core/src/menu.rs`, `crates/noido-core/src/selection.rs`

**Interfaces:**
- Produces:
  - `Key::S`; `Action::ToggleSelectTool` bound to plain `S`; `Action::Copy` bound to `command(Key::C)` (for its label; the window acts on `Event::Copy`).
  - `Check::SelectTool`; menu entry `check(Action::ToggleSelectTool, "Select text", Check::SelectTool)`, in its own section after "Show sidebar".
  - In `selection.rs`:
    ```rust
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum DragKind { Pan, Select, Handle }
    pub fn drag_kind(tool: bool, shift: bool, on_handle: bool) -> DragKind;
    /// Points per second to scroll by while dragging at `pos` in a
    /// viewport `(min_x, min_y, max_x, max_y)`, per axis: 0 more than
    /// EDGE_MARGIN inside; growing linearly to EDGE_MAX_SPEED at
    /// EDGE_RAMP past the edge; capped.
    pub fn edge_scroll(pos: (f32, f32), viewport: (f32, f32, f32, f32)) -> (f32, f32);
    pub const EDGE_MARGIN: f32 = 24.0;
    pub const EDGE_RAMP: f32 = 100.0;
    pub const EDGE_MAX_SPEED: f32 = 2400.0;
    ```
    `drag_kind`: `on_handle` → `Handle`; else `tool != shift` → `Select`; else `Pan`.

- [ ] **Step 1: Failing tests:**
  - keymap `s_toggles_select_tool` (and `None` while editing, like `O`/`G`); `shortcut_label` of Copy is `"Ctrl+C"` / `"⌘C"`.
  - menu `menu_sections` and the checked list get `"Select text"` (update the expected vectors); `every_menu_action_has_a_key` stays green.
  - `drag_kind_table`: all 8 combinations.
  - `edge_scroll_zones`: centre → (0, 0); 10 pt inside the bottom → positive y less than max; 200 pt below → `EDGE_MAX_SPEED`; left edge → negative x.
- [ ] **Step 2:** run, fail.
- [ ] **Step 3:** implement. Map `S` in the egui key translation (`app.rs`, beside `egui::Key::G => Some(Key::G)`) so the binding works; add `Action::Copy` and `Action::ToggleSelectTool` arms to the app's action match as no-ops for now, so it compiles.
- [ ] **Step 4:** `cargo nextest run --workspace`: pass.
- [ ] **Step 5:** `jj commit -m "core: select tool and copy actions, drag kinds and edge scroll speed"`

### Task 6: Selecting with the mouse, drawn on the pages (`gui-egui`)

**Files:**
- Create: `crates/frontends/gui-egui/src/texts.rs` (page texts, as `links.rs` keeps links)
- Create: `crates/frontends/gui-egui/src/select.rs` (the window's selection state: tool on/off, `Option<Selection>`, the drag under way)
- Modify: `crates/frontends/gui-egui/src/search.rs` (route `SearchUpdate::Text` out: `receive` returns them, or stashes them for `take_texts()`)
- Modify: `crates/frontends/gui-egui/src/app.rs`, `crates/frontends/gui-egui/src/titlebar.rs`

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces:
  - `Texts { want(page), get(page) -> PageTextState, insert(page, Arc<PageText>), reset(page_count) }`. `want` sends `request_text` once per page.
  - `Bar.select_tool: bool`; a title-bar `icon_button` (an I-beam drawn with `paint_select_icon`) next to the sidebar button, `selected` while on, emitting `Action::ToggleSelectTool`, hover text `"Select text"`.
  - `NoidoApp.select: select::State` with `tool: bool`, `selection: Option<Selection>`.

Behaviour in this task:
- Visible pages' text is wanted each frame (beside `self.links.want`). This changes `opening_does_not_extract`: rename it to `opening_extracts_only_visible_pages`, asserting `engine.calls()` equals the number of visible pages (not 10).
- The drag kind is decided at press from `drag_kind(tool, shift, false)`. `Select` sets the scroll area's `DragScroll::Never` while the press lasts. The pages' response senses `click_and_drag` (still not focusable).
- During a `Select` drag, the anchor is the spot at the press and the focus is the spot at the pointer each frame. `Waiting` spots are resolved each frame.
- Drawing: each visible page's `rects` for its `on_page` range, filled with `palette.selection.gamma_multiply(0.35)`, after the page image and before the search highlights.
- Clearing: a click (no drag) on the pages with `Pan` kind; Esc when no widget has focus and no settings or info page is open; reload (`self.select.selection = None`; `texts.reset`).
- `S`, the menu item and the title-bar button toggle `tool`. The pointer shows `CursorIcon::Text` over text (a `Known` page and a point inside a line's rectangle) when `drag_kind(tool, shift_held, false) == Select`.
- With the tool on, Shift+drag pans and keeps the selection.

- [ ] **Step 1: Failing harness tests** (on `searching(&TextEngine::new(&[&["the cat sat", "on the mat"], &["a dog"]]))`, pointing via `screen_pos`). Add a `drag_with(at, by, modifiers)` helper beside `drag`:
  - `shift_drag_selects_and_does_not_pan`: Shift-drag from (73, 95) to (100, 95) on page 0 → `selection` on page 0 is `0..5`; `view.offset()` unchanged.
  - `plain_drag_pans_and_selects_nothing`.
  - `tool_drag_selects_and_shift_drag_pans_keeping_it`.
  - `s_menu_and_button_toggle_the_tool`: `S` → on; the menu's "Select text" is checked; the title-bar button (find by hover text) toggles it off.
  - `selection_is_drawn`: after a selection, the shapes contain a rect filled with the selection colour over the selected text's screen position.
  - `esc_clears_selection`; `esc_in_search_field_keeps_selection`; `click_on_pages_clears_selection`.
  - `ibeam_over_text_with_shift`: pointer over text with Shift held → `CursorIcon::Text`; without → `Grab`.
  - `opening_extracts_only_visible_pages` (renamed, see above).
  - Review Focus 2, `selection_follows_zoom`: select, `Mod+=`, the drawn rect is over the same text's new screen position.
  - Review Focus 5, `reload_clears_selection`: select on page 1, reload with a one-page engine → `selection` is `None`, frames run without panic.
- [ ] **Step 2:** `cargo nextest run -p gui-egui`: new tests fail.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** `just check`: green.
- [ ] **Step 5:** `jj commit -m "egui: select text with Shift+drag or the select tool"`

### Task 7: Edge scrolling (`gui-egui`)

**Files:** Modify `crates/frontends/gui-egui/src/app.rs`, `select.rs`.

**Interfaces:** Consumes `edge_scroll` (Task 5).

Behaviour: while a `Select` or `Handle` drag is under way, each frame adds `edge_scroll(pointer, viewport) × dt` to the offset (set through the view, like keyboard scrolling), updates the focus at the pointer, and requests a repaint while the speed is nonzero. In paged mode the offset stays within the spread (the view's clamping does that; nothing turns the spread).

- [ ] **Step 1: Failing tests:**
  - `drag_past_bottom_scrolls_and_extends`: on a 3-page document, a Shift-drag held 50 pt below the viewport's bottom for 30 frames → the offset grows, and the selection's focus is on a later page than the anchor.
  - Review Focus 4, `drag_past_right_edge_scrolls_sideways`: zoomed in so the page is wider than the viewport, a Shift-drag held past the right edge → the x offset grows.
  - `no_edge_scroll_when_panning`: a plain drag held past the edge doesn't keep scrolling after the pointer stops (beyond egui's own kinetic scroll on release; compare to before the change).
- [ ] **Step 2:** run, fail. **Step 3:** implement. **Step 4:** `just check`.
- [ ] **Step 5:** `jj commit -m "egui: a selection drag near the window's edge scrolls toward it"`

### Task 8: Handles, the bar, Copy (`gui-egui`)

**Files:** Modify `app.rs`, `select.rs`; maybe Create `crates/frontends/gui-egui/src/select_bar.rs` if `select.rs` grows past one responsibility.

**Interfaces:** Consumes `Selection::text`, `caret_rect`, `drag_kind(…, on_handle)`.

Behaviour:
- **Handles**: a circle of radius 6 pt hanging below the start caret's bottom and the end caret's bottom (a teardrop is the builder's choice). The hit area is a 24 pt square around each. A press there is `DragKind::Handle`: that end becomes the focus and the other the anchor, edge scrolling applies, and the bar hides while dragging.
- **The bar**: an `egui::Area` at the end caret, above it (8 pt gap), or below it if the bar's top would leave the viewport. It shows after release when the selection isn't empty, and holds one button, "Copy". It moves with the pages (positioned from page coordinates each frame).
- **Copy**: `Event::Copy` in the frame's input when no widget has focus (`memory().focused().is_none()`), or the bar's button → if `selection.text(…)` is `Some(text)`, `ctx.copy_text(text)`. If it's `None` (pages whose text hasn't arrived), want those pages and set `copy_pending`. Each frame, while `copy_pending`, retry, and copy once it's `Some`. A new selection or clearing drops the pending copy.

- [ ] **Step 1: Failing tests:**
  - `copy_event_copies_selection`: select "the cat", send `Event::Copy` → `h.copied == ["the cat"]`.
  - `copy_in_search_field_does_not_copy_selection`: select, focus the search field, `Event::Copy` → `copied` doesn't contain the selection.
  - `copy_with_nothing_selected_does_nothing`.
  - `bar_shows_on_release_and_copies`: after a Shift-drag, `text_rect("Copy")` is `Some`, above the selection's end; clicking it → `copied`.
  - `bar_hidden_while_dragging`.
  - `handle_drag_moves_that_end_with_tool_off`: drag the end handle 12 pt to the right → the range grows by 2 characters; `view.offset()` unchanged.
  - Review Focus 1, `copy_waits_for_unknown_pages`: a gated `TextEngine`, a selection whose end is `Waiting` or on an `Unknown` page; `Event::Copy` → nothing copied yet; open the gate, run frames until arrival → `copied` has the full text.
- [ ] **Step 2:** run, fail. **Step 3:** implement. **Step 4:** `just check`.
- [ ] **Step 5:** `jj commit -m "egui: handles and a bar with Copy on the selection; Mod+C copies it"`

### Task 9: Clicks: word, line, extend; links (`gui-egui`)

**Files:** Modify `app.rs`, `select.rs`.

Behaviour:
- Only where `drag_kind(tool, shift, false) == Select`: a double-click selects `word_at` and a triple-click `line_range_at` (egui's `double_clicked` / `triple_clicked` on the pages' response).
- Shift+click (no drag) with a selection: `extend_to` the spot, also when the click is on a link (the link isn't followed).
- With the tool on and no Shift, a click on a link follows it, and a drag starting on a link selects. With the tool off and no Shift, links behave as today.

- [ ] **Step 1: Failing tests:**
  - `double_click_selects_word_with_tool`; `triple_click_selects_line_with_shift`.
  - `double_click_with_tool_off_selects_nothing`.
  - `shift_click_extends`; `shift_click_on_link_extends_and_does_not_follow` (on `link_engine()` text).
  - `tool_click_on_link_follows`; `tool_drag_from_link_selects_and_does_not_follow`.
  - `drag_starting_on_link_pans_and_does_not_follow` (existing) stays green.
- [ ] **Step 2:** run, fail. **Step 3:** implement. **Step 4:** `just check`.
- [ ] **Step 5:** `jj commit -m "egui: double- and triple-click select a word and a line; Shift+click extends"`

---

After Task 9: a release build (`just build`), a fresh review subagent on the most capable model over the whole change, Critical and Important findings fixed test-first, and minors brought to the user. Then the by-hand list from the spec.
