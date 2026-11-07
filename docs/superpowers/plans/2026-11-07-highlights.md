# Highlights Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn text selections into coloured highlights on the page, click existing highlights to adjust their ends with handles, recolour, copy, or delete them, undo and redo changes, and persist highlights in the document's state file.

**Architecture:** `noido-core::highlights` owns the pure `Highlight` and `Highlights` types, hit-testing against `PageText::rects`, and snapshot-based `undo`/`redo` stacks. `noido-state` serialises `Vec<Highlight>` inside `DocState` in `<hash>.toml`. In `gui-egui`, `select::State` tracks `editing: Option<usize>` alongside `selection: Option<Selection>` so fresh text selections and selected highlights share caret/handle placement, handle dragging, edge scrolling, `Mod+C` copying, and the popup bar.

**Tech Stack:** Rust 2024, eframe/egui 0.37, serde/toml, nextest, jj.

**Spec:** `docs/superpowers/specs/2026-11-07-highlights-design.md`

## Global Constraints

- TDD: write the failing test first and watch it fail before implementing.
- `just lint` clean (clippy `-D warnings`), `just fmt`, `just test`.
- Commit each task with `jj commit -m "<prefix>: …"`; prefixes `core`, `state`, `egui`; title ≤ 72 chars. Do not move the `main` bookmark.
- Tests use only generated fixtures or `TextEngine`, never `../testfiles/`.
- All highlight positions are document-order `Caret { page, char }` pairs (`start < end`); page rectangles come from `PageText::rects`.
- Five colours: `Yellow`, `Green`, `Blue`, `Pink`, `Purple` (`kebab-case` in TOML).
- `UNDO_LIMIT = 100`. Undo (`Mod+Z`), Redo (`Mod+Shift+Z`, `Mod+Y`), and `Backspace`/`Delete` must not act on highlights while a text field or widget has keyboard focus.

## Review Focus

1. **Typing `Backspace`, `Delete`, `Mod+Z`, or `Mod+Shift+Z` in the search field while a highlight is selected:** the search field handles the key; the highlight is neither deleted nor undone/redone. Tested in Tasks 2 and 5.
2. **Dragging a highlight's handle past the opposite handle or onto the same caret (`start == end`):** crossing swaps `start` and `end` cleanly in document order; collapsing to an empty range reverts the highlight to its stored range on release instead of deleting it or saving `start >= end`. Tested in Tasks 1 and 5.
3. **Selecting and copying a multi-page highlight whose second page's text hasn't arrived yet:** `texts.want` is requested for every page the highlight spans, and `Mod+C` waits for unknown pages before copying. Tested in Task 5.
4. **A link and a highlight at the same spot on the page:** a plain click follows the link; `Shift`+click with nothing selected selects the highlight instead of following the link; `Shift`+click with the highlight already selected extends the highlight and does not follow the link. Tested in Task 5.
5. **Loading a state file from before highlights existed, or one with malformed highlight ranges (`start >= end`):** old files deserialise with `highlights: vec![]`, and malformed ranges are dropped on load without panicking. Tested in Tasks 1 and 3.

---

### Task 1: Highlight model, hit-testing, and undo/redo (`noido-core`)

**Files:**
- Create: `crates/noido-core/src/highlights.rs`
- Modify: `crates/noido-core/src/selection.rs`
- Modify: `crates/noido-core/src/lib.rs`

**Interfaces:**
- Consumes: `Caret`, `DocPoint`, `noido_text::PageText`.
- Produces (re-exported from `noido_core`):
  ```rust
  pub const UNDO_LIMIT: usize = 100;

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum HighlightColour {
      Yellow,
      Green,
      Blue,
      Pink,
      Purple,
  }

  impl HighlightColour {
      pub const ALL: [HighlightColour; 5] = [
          Self::Yellow,
          Self::Green,
          Self::Blue,
          Self::Pink,
          Self::Purple,
      ];
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Highlight {
      pub start: Caret,
      pub end: Caret,
      pub colour: HighlightColour,
  }

  impl Highlight {
      pub fn on_page(&self, page: usize, char_count: usize) -> Option<Range<usize>>;
      pub fn pages(&self) -> RangeInclusive<usize>;
  }

  #[derive(Clone, Debug, Default, PartialEq, Eq)]
  pub struct Highlights { /* items, undo, redo */ }

  impl Highlights {
      pub fn from_items(items: Vec<Highlight>) -> Self;
      pub fn items(&self) -> &[Highlight];
      pub fn get(&self, index: usize) -> Option<Highlight>;
      pub fn add(&mut self, highlight: Highlight) -> Option<usize>;
      pub fn set_colour(&mut self, index: usize, colour: HighlightColour) -> bool;
      pub fn set_range(&mut self, index: usize, a: Caret, b: Caret) -> bool;
      pub fn remove(&mut self, index: usize) -> Option<Highlight>;
      pub fn undo(&mut self) -> bool;
      pub fn redo(&mut self) -> bool;
      pub fn at_point(&self, p: DocPoint, text: &PageText) -> Option<usize>;
  }
  ```
  Rules:
  - Share `pub(crate) fn range_on_page(start: Caret, end: Caret, page: usize, char_count: usize) -> Option<Range<usize>>` in `selection.rs` between `Selection::on_page` and `Highlight::on_page`.
  - `from_items` retains only items with `start < end`.
  - `add` rejects `start >= end` (`None`); otherwise pushes `items.clone()` to `undo` (trimming oldest beyond `UNDO_LIMIT`), clears `redo`, appends the highlight, and returns `Some(index)`.
  - `set_colour` and `set_range` (ordering `start = a.min(b)`, `end = a.max(b)`, requiring `start < end`) push to `undo` and clear `redo` only when the value actually changes.
  - `at_point` checks highlights with `on_page(p.page, text.char_count())`, testing whether any `r` in `text.rects(range)` contains `(p.x, p.y)` (`r.x <= p.x && p.x <= r.x + r.width && r.y <= p.y && p.y <= r.y + r.height`), and picks the match minimising `(h.end.page - h.start.page, h.end.char.saturating_sub(h.start.char), std::cmp::Reverse(index))`.

- [ ] **Step 1: Write the failing tests** in `crates/noido-core/src/highlights.rs`:
  - `add_and_on_page_across_pages`: highlight `(0, 4)..(2, 3)` gives `Some(4..11)` on page 0 (`char_count = 11`), `Some(0..5)` on page 1 (`char_count = 5`), `Some(0..3)` on page 2, `None` on page 3; `from_items` and `add` drop `start >= end`.
  - `set_colour_set_range_and_remove`: `set_colour` changes colour and returns `true` (`false` when unchanged); `set_range` with reversed carets `(0, 9), (0, 2)` stores `2..9`, and with equal carets `(0, 4), (0, 4)` returns `false` and leaves the highlight unchanged; `remove` deletes and returns it.
  - `undo_and_redo_lifecycle`: add, recolour, resize, remove → 4 `undo()` calls step back to empty, 4 `redo()` calls step forward; a new mutation after `undo()` clears `redo`; no-op `set_colour` / `set_range` adds no undo step.
  - `undo_limit_caps_history`: `UNDO_LIMIT + 10` adds allow at most `UNDO_LIMIT` undos.
  - `at_point_picks_shorter_then_newer`: on `noido_text::testing::page(&["the cat sat", "on the mat"])`, add `0..11` ("the cat sat") and `4..7` ("cat") → `at_point` on "cat" `(95.0, 95.0)` returns `Some(1)`, on "the" `(75.0, 95.0)` returns `Some(0)`, outside text `(300.0, 400.0)` returns `None`; two identical ranges `4..7` return the higher index.
- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo nextest run -p noido-core highlights`
  Expected: FAIL (module not defined).
- [ ] **Step 3: Implement `crates/noido-core/src/highlights.rs` and export from `lib.rs`**
- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo nextest run -p noido-core`
  Expected: PASS.
- [ ] **Step 5: Commit**
  Run: `jj commit -m "core: highlights model, hit-testing and undo/redo"`

### Task 2: Undo and Redo keys, actions, and menu (`noido-core`, `gui-egui`)

**Files:**
- Modify: `crates/noido-core/src/keymap.rs`
- Modify: `crates/noido-core/src/menu.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`

**Interfaces:**
- Produces:
  - `Key::Z`, `Key::Y` in `noido_core::Key`.
  - `Action::Undo`, `Action::Redo` in `noido_core::Action`.
  - Bindings in `Keymap::native()`:
    - `(command(Key::Z), Action::Undo)`
    - `(shift(command(Key::Z)), Action::Redo)`
    - `(command(Key::Y), Action::Redo)`
  - In `Keymap::action(press, editing)`: when `editing` is `true`, `Action::Undo` and `Action::Redo` are filtered out (`None`) so text fields keep their own undo/redo.
  - In `menu()`: `vec![entry(Action::Undo, "Undo"), entry(Action::Redo, "Redo")]` right after `vec![check(Action::ToggleSelectTool, "Select text", Check::SelectTool)]`.
  - In `crates/frontends/gui-egui/src/app.rs`: `egui::Key::Z => Some(Key::Z)` and `egui::Key::Y => Some(Key::Y)` in `core_key`, and `Action::Undo | Action::Redo => {}` stub arm in `apply` (wired in Task 4).

- [ ] **Step 1: Write the failing tests** in `crates/noido-core/src/keymap.rs` and `crates/noido-core/src/menu.rs`:
  - In `keymap.rs`: `undo_and_redo_keys_and_not_while_editing`: `command(Key::Z)` → `Some(Action::Undo)`, `shifted(Key::Z, true)` → `Some(Action::Redo)`, `press(Key::Y, true)` → `Some(Action::Redo)`; all three return `None` when `editing` is `true`; `key_for(Action::Redo)` is `shifted(Key::Z, true)` (`"Shift+Ctrl+Z"` on `Platform::Other`, `"⇧⌘Z"` on `Platform::Mac`).
  - In `menu.rs`: update `menu_sections` expected vector to include `vec!["Undo", "Redo"]` after `vec!["Select text"]`.
- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo nextest run -p noido-core`
  Expected: FAIL (`Key::Z`, `Action::Undo` missing).
- [ ] **Step 3: Implement in `keymap.rs`, `menu.rs`, and `app.rs`**
- [ ] **Step 4: Run tests to verify they pass**
  Run: `cargo nextest run --workspace`
  Expected: PASS.
- [ ] **Step 5: Commit**
  Run: `jj commit -m "core: Undo and Redo actions, shortcuts and menu entries"`

### Task 3: Saving and loading highlights (`noido-state`, `gui-egui`)

**Files:**
- Modify: `crates/noido-state/src/lib.rs`
- Modify: `crates/noido-state/tests/store.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`

**Interfaces:**
- Consumes: `Caret`, `Highlight`, `HighlightColour`, `Highlights` from `noido-core`.
- Produces:
  - `DocState` gains `pub highlights: Vec<Highlight>`.
  - `Entry` in `noido-state` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] highlights: Vec<StoredHighlight>`, where:
    ```rust
    #[derive(Serialize, Deserialize)]
    struct StoredHighlight {
        start_page: usize,
        start_char: usize,
        end_page: usize,
        end_char: usize,
        colour: StoredColour,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum StoredColour {
        Yellow,
        Green,
        Blue,
        Pink,
        Purple,
    }
    ```
  - `Entry::state(&self)` passes loaded highlights through `Highlights::from_items(...).items().to_vec()` (or filters `start < end`) so malformed entries are dropped.
  - `NoidoApp` gains `highlights: Highlights`, restored in `with_persistence` via `self.highlights = Highlights::from_items(state.highlights.clone())` and written in `doc_state()` via `highlights: self.highlights.items().to_vec()`.

- [ ] **Step 1: Write the failing tests** in `crates/noido-state/tests/store.rs`:
  - Update `state(page)` helper in `store.rs` (and `DocState` literals in `app.rs` tests) with `highlights: Vec::new()`.
  - `highlights_round_trip`: save a `DocState` with single-page and multi-page highlights across all five `HighlightColour` variants → `loaded(&store, &key)` matches, and the TOML file contains `colour = "yellow"`, `colour = "purple"`, etc.
  - `entries_without_highlights_load_empty`: save an empty-highlights `DocState`, verify the TOML has no `[[highlights]]`, and verify `loaded` has `highlights.is_empty()`.
  - `malformed_highlight_ranges_are_dropped_on_load`: append a `[[highlights]]` block with `start_page = 2, start_char = 5, end_page = 1, end_char = 0, colour = "yellow"` to a saved TOML file → `loaded` drops the bad entry and keeps valid ones.
- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo nextest run -p noido-state`
  Expected: FAIL (`DocState` has no field `highlights`).
- [ ] **Step 3: Implement in `crates/noido-state/src/lib.rs` and `crates/frontends/gui-egui/src/app.rs`**
- [ ] **Step 4: Run tests to verify they pass**
  Run: `cargo nextest run --workspace`
  Expected: PASS.
- [ ] **Step 5: Commit**
  Run: `jj commit -m "state: save and load document highlights"`

### Task 4: Making and drawing highlights from the selection bar (`gui-egui`)

**Files:**
- Modify: `crates/frontends/gui-egui/src/select.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`

**Interfaces:**
- Consumes: `Highlight`, `HighlightColour`, `Highlights`, `Action::{Undo, Redo}`.
- Produces in `crates/frontends/gui-egui/src/select.rs`:
  - `pub fn swatch_colour(colour: HighlightColour) -> Color32` (solid fill for the circular button in the bar).
  - `pub fn highlight_fill(colour: HighlightColour, dark: bool) -> Color32` (translucent fill drawn over page text).
  - On `select::State`: `pub swatches: [Option<Rect>; 5]` (where the 5 colour buttons were drawn last frame, cleared alongside `bar_rect`).
- Behaviour in `crates/frontends/gui-egui/src/app.rs`:
  - On each visible page, before drawing the active selection and search matches, draw every highlight in `self.highlights.items()` that has `Some(range) = highlight.on_page(page, text.char_count())`: fill each `area` in `text.rects(range)` with `select::highlight_fill(highlight.colour, ui.visuals().dark_mode)`.
  - In `selection_bar`: inside a horizontal layout in the popup frame, after the `"Copy"` button, draw five circular swatch buttons in `HighlightColour::ALL` order, storing their screen rects in `self.select.swatches`. Clicking swatch `colour` when `self.select.selection` is non-empty and has `carets() == Some((start, end))` calls `self.highlights.add(Highlight { start, end, colour })` and `self.select.clear()`.
  - In `apply`:
    - `Action::Undo` → `self.highlights.undo(); self.select.clear();`
    - `Action::Redo` → `self.highlights.redo(); self.select.clear();`

- [ ] **Step 1: Write the failing harness tests** in `crates/frontends/gui-egui/src/app.rs`:
  - `colour_swatch_in_bar_creates_highlight_and_clears_selection`: `select_the_cat(&mut h)`, run 1 frame, click `h.app.select.swatches[0].unwrap().center()` → `h.app.select.selection` is `None`, and `h.app.highlights.items()` is `[Highlight { start: Caret { page: 0, char: 0 }, end: Caret { page: 0, char: 7 }, colour: HighlightColour::Yellow }]`.
  - `highlight_is_drawn_and_follows_zoom`: after creating that yellow highlight, `filled_with(&h, select::highlight_fill(HighlightColour::Yellow, false))` contains `screen_pos(&h, 0, 80.0, 95.0)`; after `Mod+=` and 3 frames, it still contains the zoomed `screen_pos(&h, 0, 80.0, 95.0)` and not `(120.0, 95.0)`.
  - `undo_and_redo_highlight_creation`: after creating the highlight, press `Mod+Z` → `h.app.highlights.items().is_empty()`; press `Mod+Shift+Z` → the highlight is back; press `Mod+Z` then `Mod+Y` → the highlight is back.
  - `highlights_persist_in_doc_state`: create a highlight in an app with `with_persistence`, advance time past `SAVE_DELAY` → `store.load(key).1.unwrap().highlights` equals `h.app.highlights.items()`.
- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo nextest run -p gui-egui highlight`
  Expected: FAIL (`swatches` / `highlight_fill` missing).
- [ ] **Step 3: Implement in `select.rs` and `app.rs`**
- [ ] **Step 4: Run checks to verify they pass**
  Run: `just check`
  Expected: PASS.
- [ ] **Step 5: Commit**
  Run: `jj commit -m "egui: make and draw highlights from the selection bar"`

### Task 5: Selecting, adjusting, recolouring, copying, and deleting a highlight (`gui-egui`)

**Files:**
- Modify: `crates/frontends/gui-egui/src/select.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`

**Interfaces:**
- Produces on `select::State`:
  - `pub editing: Option<usize>`: index in `Highlights::items()` of the currently selected highlight (`None` for a fresh text selection or when nothing is selected). Reset to `None` in `clear(&mut self)` and when a new `DragKind::Select` drag starts (`drag.moved`) or a double/triple-click selects a word/line.
  - `pub delete_rect: Option<Rect>`: where the delete button in the bar was drawn last frame (only shown when `editing.is_some()`).
- Behaviour:
  - **Selecting by click**: in `select::State::update` (given `&mut Highlights` and whether a link was hovered/clicked):
    - On a single click (`click && !(drag.kind == DragKind::Select && (double || triple))` and not `drag.shift && self.selection.is_some()`):
      - If a link is under the pointer and `!drag.shift`: clear `selection` and `editing`, and let the link be followed as today.
      - Otherwise, if `point` is on a `Known(text)` page and `highlights.at_point(p, text)` is `Some(idx)` (with `h = highlights.get(idx)`): set `self.editing = Some(idx)`, `self.selection = Some(Selection { anchor: Spot::At(h.start), focus: Spot::At(h.end) })`, and suppress following any link under that `Shift`+click.
      - Otherwise: clear `self.selection = None` and `self.editing = None`.
  - **Adjusting ends**:
    - When `self.editing == Some(idx)` and a `DragKind::Handle` drag releases:
      - If `self.selection` is non-empty (`!s.is_empty(...)`) and `s.carets()` is `Some((start, end))`, call `highlights.set_range(idx, start, end)` and set `self.selection` to `Spot::At(start)..Spot::At(end)`.
      - Otherwise (collapsed to empty), restore `self.selection` from `highlights.get(idx)`.
    - When `self.editing == Some(idx)` and `Shift`+click extends `self.selection`:
      - If `self.selection` is non-empty and `carets()` is `Some((start, end))`, call `highlights.set_range(idx, start, end)`; otherwise restore `self.selection` from `highlights.get(idx)`.
  - **Page text & drawing while selected**:
    - While `self.editing == Some(idx)`, request text (`self.texts.want`) for all pages in `h.pages()`, skip drawing the blue `Palette::selection` fill, and draw highlight `idx` using `self.select.selection`'s `on_page` range (so handle drags resize the highlight live on screen).
  - **Bar and keys for a selected highlight**:
    - In `selection_bar`: when `self.select.editing == Some(idx)`, draw a ring around the swatch matching `highlights.get(idx).colour`, and draw a delete button after the swatches (recording its rect in `self.select.delete_rect`).
    - Clicking swatch `colour` when `editing == Some(idx)` calls `self.highlights.set_colour(idx, colour)` and `self.select.clear()`.
    - Clicking the delete button calls `self.highlights.remove(idx)` and `self.select.clear()`.
    - In `handle_input`: when `!editing && !self.select.widget_focused && !egui::Popup::is_any_open(ctx)` and `ctx.input(|i| i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete))`, if `let Some(idx) = self.select.editing`, call `self.highlights.remove(idx)` and `self.select.clear()`.
    - On `Esc` (same guard): call `self.select.clear()` (clearing both `selection` and `editing`).

- [ ] **Step 1: Write the failing harness tests** in `crates/frontends/gui-egui/src/app.rs`:
  - `click_highlight_selects_it_with_tool_off_and_shows_handles_and_delete`: create a highlight on "the cat", click on `(85.0, 95.0)` with tool off → `h.app.select.editing == Some(0)`, `h.app.select.handles[0].is_some()`, `h.app.select.handles[1].is_some()`, `h.app.select.delete_rect.is_some()`, and no blue selection fill is drawn; clicking empty page `(300.0, 400.0)` clears `editing` and `selection`.
  - `drag_highlight_handle_resizes_it_and_undoes_in_one_step`: select the highlight on `0..7`, drag its end handle right by 2 chars to `0..9` → `h.app.highlights.get(0).unwrap().end.char == 9` and `h.app.select.editing == Some(0)`; press `Mod+Z` → `end.char == 7` and `h.app.select.editing == None`.
  - `collapsing_highlight_handle_reverts_to_stored_range`: select the highlight on `0..7`, drag its end handle onto its start handle → on release, `h.app.highlights.get(0)` is still `0..7` and `selected_on(&h, 0) == Some(0..7)`.
  - `colour_swatch_recolours_selected_highlight_and_clears_selection`: select the yellow highlight, click swatch 2 (`Blue`) → `h.app.highlights.get(0).unwrap().colour == HighlightColour::Blue` and `h.app.select.editing == None`; `Mod+Z` restores `Yellow`.
  - `delete_button_and_backspace_remove_selected_highlight`: select the highlight, click `delete_rect` → `highlights.items().is_empty()`; `Mod+Z` brings it back; select it again, press `Backspace` → removed; `Mod+Z` brings it back; select it again, press `Delete` → removed.
  - `backspace_and_undo_in_search_field_leave_highlight_alone`: select the highlight, focus search, type `"cat"`, press `Backspace` and `Mod+Z` → `h.app.highlights.items().len() == 1`.
  - `copy_copies_selected_highlight_across_pages`: on a gated `TextEngine`, add a highlight from `(0, 0)` to `(1, 3)` and select it (`editing = Some(0)`), send `Event::Copy`, open the gate → `h.copied == vec!["the cat sat on the mat a d"]`.
  - `click_overlapping_highlights_picks_shorter_and_double_click_selects_word`: add `0..11` ("the cat sat") and `4..7` ("cat"); clicking "cat" `(95.0, 95.0)` selects index `1` (`4..7`), clicking "the" `(75.0, 95.0)` selects index `0` (`0..11`); double-clicking "sat" `(118.0, 95.0)` with `Shift` held sets `editing == None` and selects `8..11`.
  - `link_and_highlight_plain_click_follows_link_shift_click_selects_highlight`: on `linking_text()`, add a highlight covering the link on page 0 (`0..10`); a plain click on `(100.0, 96.0)` follows the link (`h.app.chosen == Some(5)`, `h.app.select.editing == None`); reset `chosen = None`, `Shift`+click `(100.0, 96.0)` with nothing selected → `h.app.select.editing == Some(0)` and `h.app.chosen == None`; `Shift`+click `(120.0, 96.0)` extends the highlight and does not follow the link.
- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo nextest run -p gui-egui highlight`
  Expected: FAIL (`editing` / `delete_rect` missing).
- [ ] **Step 3: Implement in `select.rs` and `app.rs`**
- [ ] **Step 4: Run checks to verify they pass**
  Run: `just check`
  Expected: PASS.
- [ ] **Step 5: Commit**
  Run: `jj commit -m "egui: select, resize, recolour and delete highlights on the page"`

---

After Task 5: build release (`just build`), dispatch a fresh review subagent (`opencode-go/gemini-4-argon:medium`) over the whole branch, fix Critical and Important findings test-first, and bring any minors to the user.
