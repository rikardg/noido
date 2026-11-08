# Outline in the Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task, inline, as CLAUDE.md says, with a ledger in `.superpowers/sdd/outline-sidebar/progress.md`. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Printed page numbers wherever noido shows a page number, then the PDF's outline as a tree in a new sidebar tab, with sidebar tabs for thumbnails, outline and search results.

**Architecture:** `engine-hayro` reads `/PageLabels` and `/Outlines` from raw objects. The links thread answers both once per `Linker`. `noido-core` holds all the pure logic (number formats, flattened tree, folding, current entry, tab transitions), and `gui-egui` draws it.

**Tech Stack:** Rust 2024, hayro 0.9 (`hayro_syntax` objects), eframe/egui 0.37, serde/TOML, Python for fixtures, jj for commits.

**Spec:** `docs/superpowers/specs/2026-11-08-outline-sidebar-design.md`

## Global Constraints

- Always TDD: the failing test first, watched failing.
- Commit with `jj commit -m "<message>"`, title at most 72 characters, prefix the crate where the point is; the `main` bookmark is not moved.
- Tests and examples use only `fixtures/`; nothing refers to `../testfiles/`.
- `just lint` clean and `just fmt` applied before each commit; `just check` passes at the end of each task.
- Malformed PDF input never panics; panics on the links thread are caught.
- Every spread question goes through `spread_of` / `spread_pages` / `spread_count`.
- Parentheses always mean a file number; a printed number never has them.
- File numbers stay in use for `--page`, links into noido, saved state, jumps, progress text and error messages.
- Limits: outline depth 64, 100,000 entries; roman numerals above 3999 show as decimal; a `/St` below 1 counts as 1.
- Never send synthetic input to the desktop. GUI behaviour is tested through the `Harness` in `app.rs`.

## Review Focus

- A saved Outline tab reopening a document that has since lost its outline shows the empty-state text and doesn't fall over. Pinned in Task 7.
- A reload to a shorter document, while an entry is marked or has just been clicked: no out-of-range page, no stale mark. Pinned in Tasks 5 and 7.
- An outline target past the last page (a broken file): the row's number and the jump both use the last page, as links clamp. Pinned in Task 7.
- Typing a search with the sidebar closed from a document saved on the Outline tab, then Esc twice: the sidebar closes and the Outline tab is what returns next time. Pinned in Task 6.
- A long label or title at the narrowest sidebar (160 points): the file number and the chevron stay visible, with nothing painted outside the row. Pinned in Tasks 3 and 7.

---

### Task 1: Read `/PageLabels`

**Files:**
- Create: `crates/engine-hayro/src/labels.rs`, `crates/engine-hayro/src/pdf_string.rs`, `crates/engine-hayro/tests/labels.rs`
- Modify: `crates/noido-engine/src/engine.rs`, `crates/engine-hayro/src/lib.rs`, `crates/engine-hayro/src/properties.rs`, `crates/noido-render/src/links.rs`, `crates/noido-render/tests/linker.rs`, `fixtures/make_fixtures.py`

**Interfaces:**
- Produces: `Document::page_labels(&self) -> Vec<Option<String>>`, default empty. `Linker::request_labels(&self)`, answered by `LinkReply::Labels(Vec<Option<String>>)`. `pdf_string::decode_text(bytes: &[u8]) -> String` as `pub(crate)`, moved from `properties.rs` with `pdf_doc_char`.

- [ ] **Step 1: Add `page_labels()` to `make_fixtures.py`, documented in its header, writing `fixtures/page-labels.pdf`, and generate it**

12 blank pages. `/PageLabels` is an object, Root, `<< /Kids [K1 K2] >>`. K1: `/Limits [1 7]`, `/Nums [1 <</S/r>> 3 <</P(A-)/S/D/St 5>> 5 <</P(Cover)>> 7 <<>>]`. K2: `/Limits [8 11]`, `/Nums [8 <</S/R/St 4>> 9 <</S/a/St 27>> 10 <</S/A/St 0>> 11 <</S/D>>]`, plus `/Kids [Root]`, a cycle that must be skipped while K2's own `/Nums` still count.

Run: `python3 fixtures/make_fixtures.py`

- [ ] **Step 2: Write the failing tests**

```rust
// crates/engine-hayro/tests/labels.rs
#[test]
fn labels_of_every_range() {
    let labels = open("page-labels.pdf").page_labels();
    let expected = [
        None, s("i"), s("ii"), s("A-5"), s("A-6"), s("Cover"), s("Cover"),
        None, s("IV"), s("aa"), s("A"), s("1"),
    ];
    assert_eq!(labels, expected);
}
#[test]
fn no_labels_gives_none() {
    assert!(open("two-pages.pdf").page_labels().is_empty());
}
// crates/engine-hayro/src/labels.rs, unit tests
assert_eq!(roman(4), "iv");
assert_eq!(roman(3999), "mmmcmxcix");
assert_eq!(roman(4000), "4000");
assert_eq!(letters(1), "a");
assert_eq!(letters(26), "z");
assert_eq!(letters(27), "aa");
assert_eq!(letters(28), "bb");
assert_eq!(letters(53), "aaa");
// crates/noido-render/tests/linker.rs
fn labels_reply_comes_back();           // the engine's labels, in order
fn panicking_labels_give_none();        // LinkReply::Labels(vec![])
```

`LinkEngine` gets `labels: Vec<Option<String>>` and `panicking_labels: bool`.

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test -p engine-hayro --test labels; cargo test -p noido-render --test linker labels`
Expected: compile errors: no method `page_labels`, no `request_labels`.

- [ ] **Step 4: Implement `labels::page_labels(pdf: &Pdf) -> Vec<Option<String>>`, the engine method and the Linker request**

Collect `(first_page, Dict)` pairs through `/Nums` and `/Kids`, with a visited set by `obj_id()` and `MAX_TREE_DEPTH`; sort stably by page; ignore pairs past the page count. A page's number within a range is `St.max(1) + page - first`. Return an empty `Vec` when the catalog has no `/PageLabels`, or when every label is `None`. `/P` goes through `decode_text`. The Linker's reply goes through `crate::caught_none(|| Some(document.page_labels())).unwrap_or_default()`.

- [ ] **Step 5: Run the tests and `just check`**

Expected: PASS.

- [ ] **Step 6: Commit**

`jj commit -m "hayro: read the page labels a PDF declares"`

### Task 2: Number formats in core

**Files:**
- Create: `crates/noido-core/src/page_numbers.rs`
- Modify: `crates/noido-core/src/lib.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageNumbers { /* labels */ }

/// A page's number in two parts: the label may be elided, the file part not.
#[derive(Clone, Debug, PartialEq)]
pub struct PageNumber<'a> {
    pub label: Option<&'a str>,
    /// "(79)" beside a differing label or alone where a page has none;
    /// "12" when the document has no labels; None when the label equals it.
    pub file: Option<String>,
}

impl PageNumbers {
    pub fn new(labels: Vec<Option<String>>) -> Self;
    pub fn both(&self, page: usize) -> PageNumber<'_>; // Pages tab, search cards
    pub fn short(&self, page: usize) -> String;        // outline
    pub fn title_bar(&self, page: usize, count: usize) -> String;
}
impl PageNumber<'_> { pub fn text(&self) -> String; } // parts joined by a space
```

- [ ] **Step 1: Write the failing tests: every row of the spec's table**

```rust
let n = PageNumbers::new(labels(&[(78, "61"), (4, "5")], 300)); // others None
assert_eq!(n.both(78).text(), "61 (79)");
assert_eq!(n.short(78), "61");
assert_eq!(n.title_bar(78, 300), "61 (79 / 300)");
assert_eq!(n.both(4).text(), "5");
assert_eq!(n.short(4), "5");
assert_eq!(n.title_bar(4, 300), "5 / 300");
assert_eq!(n.both(11).text(), "(12)");
assert_eq!(n.short(11), "(12)");
assert_eq!(n.title_bar(11, 300), "(12 / 300)");
let none = PageNumbers::default();
assert_eq!(none.both(11).text(), "12");
assert_eq!(none.short(11), "12");
assert_eq!(none.title_bar(11, 300), "12 / 300");
// All None behaves as no labels.
assert_eq!(PageNumbers::new(vec![None; 3]).short(1), "2");
// Past the end, as a broken target can be: treated as having no label.
assert_eq!(n.short(400), "(401)");
// The title bar elides a label past 24 characters; both() never does.
assert_eq!(long.title_bar(0, 3), "abcdefghijklmnopqrstuvw… (1 / 3)");
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p noido-core page_numbers`
Expected: compile error, no `PageNumbers`.

- [ ] **Step 3: Implement in `page_numbers.rs`, export from `lib.rs`**

- [ ] **Step 4: Run the tests and `just check`**

Expected: PASS.

- [ ] **Step 5: Commit**

`jj commit -m "core: format page numbers with their printed labels"`

### Task 3: Show printed numbers

**Files:**
- Modify: `crates/frontends/gui-egui/src/links.rs`, `app.rs:1105`, `titlebar.rs:94`, `sidebar.rs:451` and `:591`

**Interfaces:**
- Consumes: `Linker::request_labels`, `LinkReply::Labels`, `PageNumbers`.
- Produces: `Links.numbers: PageNumbers`, public. `sidebar::Input.numbers: &'a PageNumbers`. `titlebar::status_text(numbers: &PageNumbers, page: usize, count: usize, scale: f32) -> String`. `sidebar::elided_number(ui, number: &PageNumber, font, colour, max_width) -> Arc<Galley>`, with the label truncated by `TextWrapping::truncate` to what the file part leaves.

- [ ] **Step 1: Write the failing harness tests**

`TextEngine` gets `labels: Arc<Vec<Option<String>>>`. Using `h.frames` until the reply arrives:

```rust
fn title_bar_shows_the_printed_number();   // status text starts "61 (79 / 300)"
fn title_bar_shows_file_numbers_before_the_reply(); // with a link gate closed
fn thumbnail_label_shows_both_numbers();   // sidebar.thumb_label(78) == "61 (79)"
fn search_card_shows_both_numbers();       // header text == "Page 61 (79)"
fn no_labels_look_as_before();             // "12 / 300", thumbnail "12"
fn reload_keeps_labels_until_the_reply();  // old numbers, then the new ones
fn long_label_keeps_the_file_number_at_narrow_width(); // sidebar 160 wide, galley ends "(1)", rect within the column
```

The labels request needs the link gate to hold back too: gate `page_labels` in `TextEngine`, like `links`. Add small pure helpers where tests need the text (`sidebar::thumb_label_text`, `sidebar::card_header_text`), and update `status_text_counts_from_one` and the call at `app.rs:6249`.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p gui-egui --bin noido number`
Expected: compile errors or wrong text.

- [ ] **Step 3: Implement**

`Links::set_linker` keeps `numbers` across its reset and calls `request_labels`. `receive` stores the reply. Thumbnails and cards draw through `elided_number`. The title bar uses `status_text(numbers, …)`.

- [ ] **Step 4: Run the tests and `just check`**

Expected: PASS.

- [ ] **Step 5: Commit**

`jj commit -m "egui: show printed page numbers beside the file's"`

### Task 4: Read `/Outlines`

**Files:**
- Create: `crates/engine-hayro/src/outline.rs`, `crates/engine-hayro/tests/outline.rs`
- Modify: `crates/noido-engine/src/types.rs`, `engine.rs`, `crates/engine-hayro/src/lib.rs`, `links.rs` (`target` becomes `pub(crate)`), `crates/noido-render/src/links.rs`, `crates/noido-render/tests/linker.rs`, `fixtures/make_fixtures.py`

**Interfaces:**
- Produces: `noido_engine::OutlineEntry { title: String, target: Option<LinkTarget>, open: bool, children: Vec<OutlineEntry> }` (`Clone, Debug, PartialEq`). `Document::outline(&self) -> Vec<OutlineEntry>`, default empty. `Linker::request_outline(&self)`, answered by `LinkReply::Outline(Vec<OutlineEntry>)`.

- [ ] **Step 1: Add `outline()`, `outline_empty()` and `outline_deep()` to `make_fixtures.py`, documented in its header, and generate them**

`fixtures/outline.pdf`, 5 pages at 612x792:

```
Chapter 1      /Dest [p0 /XYZ 72 700 null], /Count 2
  Section 1.1  /A GoTo (sec) by the /Names tree, to [p1 /FitH 600], /Count 1
    Deep       /Dest [p2 /Fit]
  Section 1.2  /Dest [p0 /XYZ 72 700 null]
Chapter 2      /A URI https://example.org/, /Count -1
  Hidden       /Dest [p3 /Fit]
Part           no target, /Count 1
  Unresolved   /Dest (nowhere)
<FEFF 00C5 0073 0061> ("Åsa")   /Dest [p4 /Fit]
()             no target
(Two\n  lines) /A << /S /GoToR >>
Cycle A        /Dest [p0 /Fit], no /Count, /Next Cycle B
Cycle B        /Next Cycle A (a /Next cycle), /First Chapter 1 (a /First cycle)
```

`fixtures/outline-empty.pdf`: `/Outlines` with no `/First`. `fixtures/outline-deep.pdf`: 70 items, each the only child of the one before.

- [ ] **Step 2: Write the failing tests**

```rust
fn whole_tree();          // compares titles, depth and targets of the tree above:
                          // "Untitled", "Two lines", "Åsa"; Part and Unresolved have target None;
                          // Chapter 2's is Web("https://example.org/");
                          // Section 1.1 is page 1 with y from the /FitH
fn open_flags();          // Chapter 1 and Part open; Chapter 2 and Cycle A closed
fn cycles_end();          // the root's last entries are Cycle A and Cycle B, each with no children
fn no_outline_is_empty(); // two-pages.pdf
fn empty_outline_is_empty(); // outline-empty.pdf
fn depth_is_capped();     // outline-deep.pdf: chain length 64
// unit test in src/outline.rs, walking with a smaller limit:
fn entries_are_capped();  // read(pdf, index, 3) gives 3 entries in all
// crates/noido-render/tests/linker.rs
fn outline_reply_comes_back();
fn panicking_outline_gives_none();
```

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test -p engine-hayro --test outline; cargo test -p noido-render --test linker outline`
Expected: compile errors, no `outline`.

- [ ] **Step 4: Implement `outline::outline(pdf: &Pdf, index: &PageIndex) -> Vec<OutlineEntry>`, over `read(pdf, index, MAX_ENTRIES)`**

Walks as in spec §1. The visited set is shared over the whole walk, checked with the item's `obj_id()` before reading it. Targets come from `links::target`. Title whitespace is collapsed with `split_whitespace`.

- [ ] **Step 5: Run the tests and `just check`**

Expected: PASS.

- [ ] **Step 6: Commit**

`jj commit -m "hayro: read the document's outline"`

### Task 5: The tree's logic and the reader's position

**Files:**
- Create: `crates/noido-core/src/outline.rs`
- Modify: `crates/noido-core/src/lib.rs`, `crates/noido-core/src/view.rs`

**Interfaces:**
- Consumes: `noido_engine::{OutlineEntry, LinkTarget}`, `DocPoint`.
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineRow { pub entry: usize, pub depth: usize, pub has_children: bool, pub open: bool }

pub struct OutlineTree { /* entries flattened in tree order, rows, sorted positions, source */ }
impl OutlineTree {
    pub fn new(entries: Vec<OutlineEntry>) -> Self;
    pub fn is_empty(&self) -> bool;
    pub fn rows(&self) -> &[OutlineRow];         // the visible rows
    pub fn title(&self, entry: usize) -> &str;
    pub fn target(&self, entry: usize) -> Option<&LinkTarget>;
    pub fn toggle(&mut self, entry: usize);
    pub fn current(&self, at: DocPoint) -> Option<usize>;
    pub fn marked_row(&self, current: usize) -> Option<usize>; // index into rows()
    /// Takes in a new version: keeps the folding if it is the same tree,
    /// and returns whether it was.
    pub fn replace(&mut self, entries: Vec<OutlineEntry>) -> bool;
}
/// The offset that brings a row into view, moving as little as possible.
pub fn reveal_row(row: usize, row_height: f32, offset: f32, view_height: f32) -> Option<f32>;
// on View:
pub fn reading_point(&self, chosen: Option<usize>) -> Option<DocPoint>;
```

`current` must be cheap per frame. Hold `(page, y, entry)` sorted stably by page then `y.total_cmp`, in tree order among ties, and use `partition_point`, taking the last at or before the point. A missing y is 0.0 (the top; y runs down the page).

- [ ] **Step 1: Write the failing tests**

```rust
// outline.rs. A tree: A p1 y100 { B p1 y100, C p2 y50 { D p3 } }, E p5; web entry W; no-target N
fn rows_follow_folding();          // starts from `open`; toggle hides and shows; child folding survives
fn current_before_first_is_none(); // DocPoint { page: 0, .. }
fn current_between_headings();     // p2 y400 is C
fn current_on_a_page_with_two();   // p1 y50 is none of them; p1 y100 is B (tie: later)
fn current_without_y_is_page_top();// p3 y0 is D
fn current_out_of_order();         // entries listed p5 then p1; p2 gives the p1 entry
fn web_and_no_target_are_skipped();
fn marked_row_in_folded_branch();  // D current, C folded → C's row; A folded → A's row
fn replace_same_keeps_folding();   // returns true
fn replace_different_starts_over();// returns false, flags from the new tree, a shorter tree in range
fn reveal_row_minimal();           // None when visible; up and down by the least
// view.rs
fn reading_point_single_is_place();
fn reading_point_dual_is_last_page_of_spread();    // spread 3|4 centred: page 4, same content height
fn reading_point_gapless_dual();                   // same answer as with a gap, whichever side rounding picks
fn reading_point_paged_dual();
fn reading_point_odd_left_lone_first_page();       // odd_left false, page 0 alone: page 0
fn reading_point_last_lone_page();                 // odd page count: the last page itself
fn reading_point_chosen_same_spread_is_unchanged();
fn reading_point_chosen_other_spread_is_its_top(); // DocPoint { page, x: 0.0, y: 0.0 }
fn reading_point_no_pages_is_none();
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p noido-core outline; cargo test -p noido-core reading_point`
Expected: compile errors.

- [ ] **Step 3: Implement**

`reading_point`: take `place()`. Find its spread by `spread_of`, the last page by `spread_pages`. If `chosen` is on another spread, return that page's top. If the last page is `place().page`, return `place()`. Otherwise convert with `layout.content_point(place)` and `page_rect(last)`: `y = (content_y - rect.y) / layout.scale()`, `x` likewise.

- [ ] **Step 4: Run the tests and `just check`**

Expected: PASS.

- [ ] **Step 5: Commit**

`jj commit -m "core: an outline tree with folding and a current entry"`

### Task 6: Tabs

**Files:**
- Modify: `crates/noido-core/src/sidebar.rs`, `crates/noido-state/src/lib.rs`, `crates/noido-state/tests/store.rs`, `crates/frontends/gui-egui/src/sidebar.rs`, `app.rs:497-501`, `app.rs:1144`, `app.rs:2071`, the `SidebarState { … }` literals (add `..Default::default()`)

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab { #[default] Thumbnails, Outline, Results }

// SidebarState gains: pub tab: SidebarTab, pub last: SidebarTab
impl SidebarState {
    pub fn choose(&mut self, tab: SidebarTab);
    pub fn search_started(&mut self);
    pub fn search_cleared(&mut self);
}
```

Saved: `Entry.sidebar_tab: String` (`#[serde(default)]`), `"outline"` for `Outline`, otherwise `"thumbnails"`. Restored into both `tab` and `last`. `SidebarView::tab_rects() -> &[(SidebarTab, Rect)]` serves the harness.

- [ ] **Step 1: Write the failing tests**

```rust
// noido-core
fn choosing_results_keeps_last();
fn search_started_goes_to_results();
fn search_cleared_returns_to_last();             // Outline → started → cleared → Outline
fn search_cleared_elsewhere_changes_nothing();   // started, choose Thumbnails, cleared → Thumbnails
// noido-state: store.rs
fn sidebar_tab_round_trips();                    // Outline saved with tab Results, last Outline
fn old_files_and_unknown_tabs_give_thumbnails();
// harness
fn tabs_show_labels();                           // "Pages", "Outline", "Search"
fn clicking_a_tab_switches();
fn search_goes_to_results_and_clearing_returns();
fn new_query_from_another_tab_goes_to_results();
fn stepping_hits_from_another_tab_stays();       // and the hit still changes
fn search_stays_while_on_another_tab();          // hits and highlights intact
fn reload_rerunning_the_search_does_not_switch();
fn results_tab_without_a_search_says_so();       // "No search"
fn no_sharp_renders_off_the_thumbnails_tab();    // renderer's keep is 0..0
fn search_with_closed_sidebar_then_esc_twice();  // saved on Outline: results show,
                                                 // closed after two Escs, tab Outline
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p noido-core sidebar; cargo test -p noido-state; cargo test -p gui-egui --bin noido tab`
Expected: compile errors.

- [ ] **Step 3: Implement**

The strip is a row at the top of the panel, inside `theme::scope(Role::Sidebar)`, with the active tab in the selection fill. `show` matches on `state.tab`: `Thumbnails` → `scrolled`; `Results` → `results`, or the muted text when `input.results` is `None`; `Outline` → a placeholder until Task 7. It calls `self.closed()` whenever the tab isn't `Thumbnails`, and clears `result_rows` and `followed_hit` whenever it isn't `Results`.

In `app.rs`, after `search_state.start` at line 500: `search_started()` if `searched()` is now `Some`, otherwise `search_cleared()`. After `search_state.clear()` at line 1145: `search_cleared()`. No other call site: reload doesn't pass through these.

- [ ] **Step 4: Run the tests and `just check`**

Expected: PASS, including every existing test that assumes results replace thumbnails.

- [ ] **Step 5: Commit**

`jj commit -m "egui: tabs in the sidebar for pages, outline and search"`

### Task 7: The outline tab

**Files:**
- Modify: `crates/frontends/gui-egui/src/links.rs`, `sidebar.rs`, `app.rs:112`, `:702`, `:721`, `:777`, `:1947`, `:1953`, `:2071-2097`; `docs/backlog.md` (remove "Outline in the sidebar")

**Interfaces:**
- Consumes: `Linker::request_outline`, `LinkReply::Outline`, `OutlineTree`, `reveal_row`, `View::reading_point`, `PageNumbers::short`, `SidebarTab::Outline`.
- Produces: `Links.outline: Option<OutlineTree>`, `None` until the first reply. `sidebar::Input.outline: Option<&'a mut OutlineTree>` and `Input.current_entry: Option<usize>`. `sidebar::Output.outline_entry: Option<usize>`. `SidebarView::outline_rows() -> &[OutlineRowRects]`, `{ entry, row: Rect, chevron: Option<Rect>, title: String, number: String }`, for the harness. `NoidoApp::follow_target(&mut self, ctx, target: LinkTarget) -> ()`, split out of `follow`; `NoidoApp::choose_page(&mut self, page)`, which sets `chosen` and clears `clicked_entry`.

- [ ] **Step 1: Write the failing harness tests**

`TextEngine` gets `outline: Arc<Vec<OutlineEntry>>`, gated like `links`.

```rust
fn row_shows_title_and_number();              // short form: "61"; a row with no internal target: none
fn click_jumps_flashes_and_adds_history();    // then Alt+Left returns
fn click_on_chevron_folds_without_jumping();  // rows change, page and history don't
fn click_on_targetless_parent_toggles();
fn click_on_targetless_leaf_does_nothing();
fn drag_scrolls_without_jumping();
fn web_entry_opens_only_allowed_schemes();    // https sets open_url, javascript: doesn't
fn mark_follows_the_pages_without_unfolding();// scroll on, the marked row changes, nothing opens
fn tree_scrolls_to_the_marked_row();          // long outline: the row comes into view
fn folding_does_not_scroll();
fn reader_scrolling_stands_until_the_entry_changes();
fn mark_lands_on_visible_ancestor();
fn clicked_entry_stays_marked_near_the_end(); // last page, target clamped
fn clicked_entry_mark_ends_when_the_reader_moves();
fn dual_mode_marks_a_right_hand_chapter();
fn empty_state_after_the_reply_not_before();  // link gate closed: no text; opened: "This document has no outline"
fn saved_outline_tab_with_no_outline_shows_the_empty_state();
fn reload_same_outline_keeps_folding();
fn reload_shorter_outline_drops_stale_mark();
fn target_past_the_end_goes_to_the_last_page(); // number and jump both on the last page
fn narrow_sidebar_keeps_chevron_and_number();   // 160 wide, long title and label: rects inside the row
fn hundred_thousand_rows_draw_only_the_visible(); // outline_rows() len under 100
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p gui-egui --bin noido outline`
Expected: compile errors.

- [ ] **Step 3: Implement**

`Links::set_linker` keeps `outline` and calls `request_outline`; `receive` builds or `replace`s the tree, and `Links` reports a changed tree so the app clears `clicked_entry`.

The app's `current_entry()`: `clicked_entry` when `chosen` is `Some`, otherwise `outline.current(view.reading_point(chosen)?)`. All four `self.chosen = Some(…)` sites go through `choose_page`. Output's entry runs `follow_target`, then sets `clicked_entry`, with the page clamped by `follow_target` as now.

The tab itself, as spec §6: `ScrollArea::show_rows` with fixed row height, `DragScroll::Always`, `rubber_band = false`, fade off, and solid bars as `scrolled` sets them up. Chevron painted as two line segments: `>` folded, `v` unfolded. The number comes from `elided_number` with `short`, at most about a third of the row, in `weak_text_color`. Title truncated to the rest, with a tooltip when elided. The marked row uses `visuals.selection.bg_fill` and `selection.stroke.color`. `followed_entry` as `followed` is for thumbnails: reveal through `reveal_row` when the current entry, the tab shown or the width changes; set it to the clicked entry on a click, so no scroll follows. Pointing hand only on rows that act.

Update `sidebar.rs`'s header comment, and remove the backlog entry.

- [ ] **Step 4: Run the tests and `just check`**

Expected: PASS.

- [ ] **Step 5: Measure on the test files**

`just build`. Time `page_labels()` and `outline()` on the largest outline in `../testfiles/` with a throwaway `eprintln!` of elapsed time, removed afterwards. Report the figures. Stop and raise it if a link hover would visibly wait.

- [ ] **Step 6: Commit**

`jj commit -m "egui: show the outline in its sidebar tab"`

### Task 8: Review and hand-over

- [ ] **Step 1: Run a review by a fresh subagent on the most capable model, over all commits since the spec commit**

Give it the spec and this plan's Review Focus. Fix Critical and Important findings test-first, as `fixup: <crate>: …` commits, then squash them into the commits they correct. Present minors with a recommendation each, for the user to pick: fix now, the review-minors issue, or drop.

- [ ] **Step 2: Rebuild release and hand over**

`just build`. Tell the user to run `target/release/noido`, and what to check by hand:
- A book in two-page mode: the mark as chapters start on right-hand pages.
- A paged view: a chapter starting low on a page.
- `doingmathwithpython.pdf`: numbers against the Okular screenshot (`vii`, `xv`, `1`, `61`).
- A file with text labels: `Cover Page`, `p. 829` at the narrowest sidebar.
- A document with no outline.

- [ ] **Step 3: File the deferred work and clean up**

Open an `enhancement` issue for keys to switch tabs and move through the tree, with the user's go-ahead. Delete `.superpowers/sdd/outline-sidebar/`.
