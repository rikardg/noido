# Locations for PDF (L2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Highlights are kept as locations with a quote, are saved under
part-based names, and find their text again when a PDF changes under
them (edited in place, or rebuilt and reloaded).

**Architecture:** A `Location` type in `noido-engine`, used by core's
`Highlight` in place of `Caret`. For PDF a location is a caret, so
conversion is the identity. Quotes are taken, and highlights
re-anchored, on the text thread. It checks each highlight on its own
pages first, and walks the document only for quotes that no longer
match, holding a few pages at a time, as search does since text
retention. Pure pieces in `noido-core` do the work. The window applies
the answer when its highlights haven't changed since it asked.

**Tech Stack:** Rust 2024, serde/TOML, the existing `noido-text`
folding.

**Spec:** `docs/superpowers/specs/2026-11-15-unified-locations-design.md`

## Base

Start from main once these two tracks have landed:

- **Text retention** (`lnylwpul`, spec `2026-11-15-text-retention-design.md`):
  the text thread keeps only pages the window asked for, and a walk drops
  each page after use. Task 5 builds on its `wanted`/`failed`/`release`.
- **CBZ support** (repo-2): it changes `noido-state` (the ZIP's key),
  `noido-engine/src/types.rs` and `app.rs`, which Tasks 1, 2 and 6
  touch too.

If either changed shape on the way in, adjust Tasks 5 and 6 before
starting.

## Scope against the spec

L2 does the highlight half of the spec. Two parts move to L3, where
they're first needed:

- **`Place` for the view, history, pins and the pagelet.** For PDF a
  `Place` is the `DocPoint` and page number saved today, so moving them
  changes nothing until text reflows.
- **The page ↔ location mapping on `Document`.** `Document` isn't `Send`
  and lives on the render and text threads, so the window can't call
  `location()`/`page_char()` on it. L3 makes the mapping a value the
  engine hands over (a per-page list of character runs, each starting at
  a `Location`), held by the window like the page sizes. The fake
  reflowable engine test goes with it. Task 1 amends the spec to say so.

CBZ has no text, so it has no highlights. Task 7 checks the spec's CBZ
row: view, history and pins unchanged.

## Global Constraints

- Saved names: `start_part`, `start_char`, `end_part`, `end_char`;
  `start_page`/`end_page` read as serde aliases; the new names are
  always written.
- New optional fields: `quote`, `prefix`, `suffix` (strings, written only
  when there is a quote) and `orphaned` (written only when true).
- Prefix and suffix: up to 32 characters each.
- Matching: exact after search folding (`noido-text`'s `Query` folding:
  case, NFKC, accents unless the quote has some, whitespace runs,
  line-end hyphens). No fuzzy matching (M4).
- Orphaned highlights: kept in state, not drawn, not hit by clicks, not
  shown anywhere.
- TDD for every step; `just check` clean before each commit.
- Commits: `jj commit -m "<crate>: …"`; never move `main`.

## Review Focus

1. **A page whose text can't be read** (engine error, panic) on the text
   thread isn't evidence that the text is gone. When any page's text
   failed, highlights keep their positions and nothing is orphaned.
   Test in Task 5.
2. **The document shrank on reload** and a highlight points past the
   last page. It mustn't panic in drawing or anchoring. It re-anchors
   or is orphaned. Tests in Tasks 4 and 6.
3. **The reader changes highlights while an anchoring answer is in
   flight.** The stale answer is dropped and the current list is sent
   again, so no change of theirs is lost. Test in Task 6.
4. **Undo after a relocation** mustn't move a highlight back to the old
   version's offsets. Relocation clears undo and redo. Test in Task 4.
5. **A very short quote** (one letter, "a") that occurs hundreds of
   times picks the occurrence by context, then by distance. It doesn't
   orphan, and it stays fast on a long document. Test in Task 4.
6. **Memory on a long book:** anchoring must not hold the book's text.
   Opening a book unchanged reads only the highlights' pages, and a walk
   keeps a window of pages. Tests in Task 5.

---

### Task 1: Location in the engine; highlights in core hold locations, a quote and an orphaned flag

**Files:**
- Modify: `crates/noido-engine/src/types.rs` (add `Location`), `crates/noido-engine/src/lib.rs` if it re-exports by name
- Modify: `crates/noido-core/src/highlights.rs`, `crates/noido-core/src/lib.rs` (re-export `Location`, `Quote`)
- Modify: call sites in `crates/frontends/gui-egui/src/app.rs`, `crates/noido-state/src/lib.rs`
- Modify: `docs/superpowers/specs/2026-11-15-unified-locations-design.md` ("Mapping, on the engine trait": the mapping becomes a value the window holds, in L3; the `Place` work moves to L3)

**Interfaces:**
- Produces:
  - `noido_engine::Location { pub part: usize, pub char: usize }`, deriving `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash`.
  - `impl From<Caret> for Location` and `impl From<Location> for Caret` in core (the PDF identity; L3 replaces the use sites with the mapping).
  - `noido_core::Quote { pub exact: String, pub prefix: String, pub suffix: String }`, `Clone, Debug, PartialEq, Eq`.
  - `Highlight { pub start: Location, pub end: Location, pub colour: HighlightColour, pub quote: Option<Quote>, pub orphaned: bool }`, `Clone` (no longer `Copy`), `Debug, PartialEq, Eq`.
  - `Highlight::new(start: Caret, end: Caret, colour) -> Highlight` (no quote, not orphaned).
  - `Highlights::get(index) -> Option<&Highlight>`.
  - `Highlights::anchored(&mut self, items: Vec<Highlight>, relocated: bool)`: replaces the list without an undo step. With `relocated`, it clears undo and redo.
  - `Highlights::set_range` clears the highlight's quote (it no longer describes the text).

- [ ] **Step 1: Write the failing tests** in `highlights.rs`'s tests:
  - `orphaned_highlight_is_not_on_any_page`: an orphaned highlight from (0,0) to (0,7). `on_page(0, 20)` is `None`, and `pages()` still covers page 0.
  - `orphaned_highlight_is_not_hit`: an orphaned highlight over the point that a live test (`at_point_picks_shorter_then_newer`) hits. `at_point` gives `None`.
  - `set_range_drops_the_quote`: a highlight with a quote, resized. Its `quote` is `None`.
  - `anchored_keeps_history_unless_relocated`: add two highlights, `anchored(items_with_quotes, false)`, then `undo()` is `true`. On another list, `anchored(moved, true)`, then `undo()` is `false` and `redo()` is `false`.
  - Update the existing tests to build highlights with `Highlight::new`.
- [ ] **Step 2: Run** `cargo nextest run -p noido-core highlights`. Expect it to fail to compile (no `Location`, `quote`, `orphaned`).
- [ ] **Step 3: Implement** the types above. `on_page` and `at_point` skip orphaned highlights. Keep `on_page`'s signature; convert `self.start`/`self.end` to `Caret` inside. Fix the call sites in `app.rs` (`Highlight { start, end, colour }` → `Highlight::new`, `.get(..)` now gives a reference) and in the state crate (map through `Caret` for now; Task 2 changes the saved form).
- [ ] **Step 4: Run** `just check`. Everything passes, including the harness's highlight tests, with their expected `Highlight` values updated to `Highlight::new(..)`.
- [ ] **Step 5: Amend the spec** as listed under Files. Squash that edit into the spec commit `xltuvrrx` with `jj squash --into xltuvrrx <spec path> -u`.
- [ ] **Step 6: Commit** `jj commit -m "core: keep highlights as locations, with a quote and an orphaned flag"`.

### Task 2: Saved form with part names, quotes and orphans

**Files:**
- Modify: `crates/noido-state/src/lib.rs` (`StoredHighlight`)

**Interfaces:**
- Consumes: Task 1's `Highlight`, `Quote`, `Location`.

- [ ] **Step 1: Write the failing tests** in the state crate's tests:
  - `highlights_from_before_parts_still_load`: a TOML entry with `start_page = 3`, `start_char = 120`, `end_page = 3`, `end_char = 164`, `colour = "yellow"` loads as `Location { part: 3, char: 120 }`..`{3, 164}`, with no quote, not orphaned.
  - `highlights_save_with_part_names`: saving that state writes `start_part = 3` and `end_part = 3`, no `start_page`, and no `quote`, `prefix`, `suffix` or `orphaned` keys.
  - `quote_and_orphan_round_trip`: a highlight with `Quote { exact: "the nature", prefix: "Liberty: ", suffix: " of power" }` and `orphaned: true` saves and loads equal. The file contains `quote = "the nature"` and `orphaned = true`.
- [ ] **Step 2: Run** `cargo nextest run -p noido-state`. The new tests fail.
- [ ] **Step 3: Implement.** In `StoredHighlight`, rename to `start_part`/`end_part`, with `#[serde(alias = "start_page")]`/`alias = "end_page"`. Add `quote`, `prefix`, `suffix` as `Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. A quote loads only when `quote` is present, with a missing prefix or suffix read as empty. Add `orphaned: bool` with `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`. Add a doc comment on the aliases: "Read until TODO M5; the old names went out with L2."
- [ ] **Step 4: Run** `just check`. Passes.
- [ ] **Step 5: Commit** `jj commit -m "state: save highlights by part, with their quote"`.

### Task 3: Finding text in a run of pages, and taking a quote

**Files:**
- Create: `crates/noido-text/src/across.rs` (re-exported from `lib.rs`)
- Modify: `crates/noido-text/src/search.rs` (make `fold` and `Query`'s fields reachable from `across.rs`, crate-private)

**Interfaces:**
- Produces:
  - `pub struct Joined`: consecutive pages' text as one, for a window of a few pages, never the whole book. `Joined::new(first: usize, pages: &[&PageText]) -> Joined`, where `first` is the document page number of `pages[0]`. Pages are joined with `'\n'`, so a hyphen at a page's end joins a word the way a line-end hyphen does. Its folded forms are built here and dropped with it; they aren't stored in the `PageText`s.
  - `Joined::find(&self, query: &Query) -> Vec<Span>`, with `pub struct Span { pub start: (usize, usize), pub end: (usize, usize) }` as (document page, char): `end` is one past the last character, on that character's page.
  - `Joined::context(&self, span: Span, prefix: &str, suffix: &str) -> usize`: how many folded characters before the match agree with the folded prefix, counting back from the match, plus how many after it agree with the suffix, counting forward. It stops at the window's edges.
  - `Joined::text_at(&self, span: Span) -> Option<String>`: `copy_text` over the span, or `None` when the span isn't inside the window.
  - `Joined::quote(&self, span: Span) -> (String, String, String)`: `exact` is `text_at(span)`. `prefix` is the 32 characters of the joined text before the span, and `suffix` the 32 after it, each with whitespace runs collapsed to one space. Both cross page boundaries within the window.
  - `pub fn same_text(a: &str, b: &str) -> bool`: equal after search folding.
  - `pub fn folded_len(s: &str) -> usize`: the length of `s` after search folding, so a caller can tell a full `context` score.

- [ ] **Step 1: Write the failing tests** in `across.rs`, building pages with `testing::page`:
  - `finds_text_across_a_page_break`: page 0 ends "the nature and", and page 1 starts "limits of power". `find(Query::new("and limits"))` gives `start (0, i)` and `end (1, 6)`.
  - `window_reports_document_pages`: the same two pages as `Joined::new(7, …)`. The span is `(7, i)..(8, 6)`.
  - `finds_a_word_hyphenated_across_pages`: page 0 ends "magis-", and page 1 starts "trates". "magistrates" is found, spanning both pages.
  - `finds_with_search_folding`: "Ligature ﬁne" is found by `Query::new("ligature fine")`.
  - `context_prefers_matching_neighbours`: "the cat sat. the cat ran." With prefix "sat. " and suffix " ran", the second "the cat" scores higher than the first, and its score equals `folded_len("sat. ") + folded_len(" ran")`.
  - `quote_takes_32_characters_each_side`: a 100-character line with the range 40..50. `prefix` has 32 characters ending at 40, and `suffix` 32 starting at 50. On the window's first page, with the range at 0.., the prefix is empty.
  - `quote_neighbours_cross_pages`: a span at the very start of page 1. Its prefix is the end of page 0.
  - `same_text_folds`: `same_text("Magis-\ntrates", "magistrates")` is `true`.
- [ ] **Step 2: Run** `cargo nextest run -p noido-text across`. Expect a compile failure.
- [ ] **Step 3: Implement** with the existing `fold` (dehyphenating) over the joined characters. Keep a map from each joined character back to (page, char) in each folded form, so `find` reuses `PageText::find`'s scan on the joined folded chars.
- [ ] **Step 4: Run** `just check`. Passes.
- [ ] **Step 5: Commit** `jj commit -m "text: find text across a few pages, and take a quote with its neighbours"`.

### Task 4: Anchoring, as a check and a walk

**Files:**
- Create: `crates/noido-core/src/anchor.rs` (re-exported from `lib.rs`)

**Interfaces:**
- Consumes: Task 1's `Highlight`/`Quote`/`Location`, and Task 3's `Joined`, `Span`, `same_text`, `folded_len`.
- Produces, all pure, so the text thread can drive them a page at a time:
  - `Highlight::window(&self, page_count: usize) -> Option<RangeInclusive<usize>>`: the pages a check reads, from the page before its start to the page after its end, clamped to the document. `None` when its start is past the last page.
  - `pub fn check(h: &Highlight, window: &Joined, fill_missing: bool) -> Check`, with `pub enum Check { Keep(Highlight), Search }`.
    - **Without a quote:** `Keep`. With `fill_missing`, it takes a quote with `Joined::quote`.
    - **With a quote,** where the text at its range is `same_text` as `exact` and `context` is full (the `folded_len` of prefix plus suffix): `Keep`, un-orphaned.
    - **Otherwise:** `Search`.
  - `pub struct Walk` collects the candidates for the highlights that need searching:
    - `Walk::new(pending: Vec<(usize, Highlight)>)`, where `usize` is the highlight's index in the list.
    - `Walk::span(&self) -> usize`: how many pages the window must hold past the page being walked. That is the most pages any pending highlight's old range covers, plus one, so a quote that crosses pages and its suffix fit.
    - `Walk::page(&mut self, page: usize, window: &Joined)`: records, for each pending quote, the matches that *start* on `page`, with their `context`. The window holds `page - 1` through `page + span()`.
    - `Walk::finish(self, char_counts: &[usize]) -> Vec<(usize, Highlight)>`. Each pending highlight goes to its best candidate: highest `context`, ties to the smallest distance in characters from the old start, using `char_counts` (each page's character count, recorded by the walk) to measure across pages. It's un-orphaned. With no candidate, it's orphaned at its old position.
  - `pub fn anchor(highlights: &[Highlight], pages: &[&PageText], fill_missing: bool) -> Anchored`: drives `check` and, if anything needs searching, `Walk` over pages already in memory. `pub struct Anchored { pub items: Vec<Highlight>, pub relocated: bool }`, where `relocated` is true when any highlight's position or orphaned flag changed. Tests use it, and it's the reference the text thread's driver must agree with.

- [ ] **Step 1: Write the failing tests** in `anchor.rs`, through `anchor` unless named otherwise, with `testing::page` documents:
  - `unchanged_text_changes_nothing`: the same pages. The output equals the input, and `relocated` is false.
  - `fills_a_missing_quote_only_when_asked`: a highlight with no quote over "the cat". With `fill_missing`, the quote is "the cat" plus its neighbours. Without it, there's still no quote.
  - `follows_text_moved_on_its_page`: the quote was "the cat" at 0..7, and the new page reads "so the cat". The highlight is now 3..10, and `relocated` is set.
  - `follows_text_moved_to_another_page`: the text is only on page 1 now. The highlight moves to page 1.
  - `deleted_text_orphans_and_comes_back`: the text is gone, so the highlight is orphaned at its old position. Anchoring that against the original pages un-orphans it, at the matching position.
  - `repeats_resolve_by_context`: the page reads "the cat sat. the cat ran.", the quote is "the cat" with suffix " ran", and the old offset is 0. It picks the second occurrence.
  - `moved_to_next_page_beats_a_repeat_on_the_old_page`: page 0 had "…sat. the cat ran." and now reads "the cat slept." plus a line pushing "sat. the cat ran." to page 1. The quote "the cat" with prefix "sat. " and suffix " ran" moves to page 1, not to page 0's "the cat slept".
  - `repeat_at_the_old_offset_does_not_hold_it`: the old offsets now hold "the cat" in another sentence (other neighbours), and the real one is later on the page. The highlight moves to the real one.
  - `now_straddling_a_page_break`: a quote that was within page 0 now runs from the end of page 0 into page 1. The highlight spans both pages.
  - `across_a_page_break_moves_as_a_whole`: a highlight from (0, k) to (1, 6). After a line is inserted at the top of page 0, both ends shift together.
  - `long_highlight_over_three_pages_is_found`: a quote covering the end of page 1, all of page 2 and the start of page 3, moved by one line. `Walk::span` is 3, and the walk finds it.
  - `past_the_last_page_reanchors_or_orphans`: a highlight on page 5 of a document that is now 2 pages. `window` is `None`, `anchor` doesn't panic, and the highlight moves to its text if found, or else is orphaned.
  - `short_quote_many_times`: a 300-page document of repeated "a a a …" with quote "a", prefix "…x ", and the matching "x a" once on page 150. It picks page 150, and runs in under 100 ms in a debug test (assert on `Instant`).
  - `relocation_clears_undo` (in `highlights.rs`): `Highlights::anchored(result.items, result.relocated)` after a relocation, then `undo()` is `false`.
- [ ] **Step 2: Run** `cargo nextest run -p noido-core anchor`. Expect a compile failure.
- [ ] **Step 3: Implement.** One `Query` per distinct `exact` in `Walk`, so the walk searches each window once per distinct quote, not once per highlight.
- [ ] **Step 4: Run** `just check`. Passes.
- [ ] **Step 5: Commit** `jj commit -m "core: find a highlight's text again by its quote"`.

### Task 5: Anchoring on the text thread, keeping only a window of pages

**Files:**
- Modify: `crates/noido-render/src/text.rs`
- Test: `crates/noido-render/tests/searcher.rs`

**Interfaces:**
- Consumes: Task 4's `check`, `Walk`, `Anchored`, `Highlight::window`; the retention rules already in `text.rs` (`wanted`, `failed`, `release`).
- Produces:
  - `TextSearcher::anchor(&self, highlights: Vec<Highlight>, fill_missing: bool)`.
  - `SearchUpdate::Anchored { asked: Vec<Highlight>, answer: Anchored }`, where `asked` is the list sent, which the window compares with its current list.

How the worker runs it, as a job beside the search job, one page a step so commands still cut in:

1. **Check:** for each highlight, read its `window` pages, `check` it, and `release` the pages.
2. **Walk,** only if some check said `Search`: go through every page in order, holding page `k - 1` through `k + span()` and nothing more. Release each page as it leaves the window, record its character count, and call `Walk::page`. Then `finish`.
3. **Answer** once.

Further rules:

- If any page's text failed (the `failed` flag) during either phase, the highlights that needed searching keep their positions, unchanged and not orphaned.
- A newer `Anchor` replaces one under way.
- Requests from the window (`Command::Text`) come first, then the search job, then anchoring, then the counting pass.

- [ ] **Step 1: Write the failing tests** in `tests/searcher.rs`, with its `TextEngine`:
  - `anchor_answers_with_quotes_filled`: pages "the cat sat" and "a dog ran", with a quote-less highlight over "the cat" and `fill_missing`. One `Anchored` arrives, its `asked` equal to the list sent and its answer's highlight quoted "the cat".
  - `unchanged_highlights_read_only_their_pages`: 50 pages, one quoted highlight on page 20 whose text is unchanged. The answer arrives after at most 3 page reads (`calls`).
  - `anchoring_keeps_no_page_it_was_not_asked_for`: a walk over 50 pages for a moved quote. Afterwards, asking for a page not requested before reads it again (`calls` goes up), as with a search.
  - `moved_text_is_found_by_the_walk`: the quote's text is now on page 40 of 50. The answer moves it there.
  - `unreadable_page_moves_nothing`: `failing: Some(1)`, with a highlight whose quote isn't where it was. The answer equals the input: nothing orphaned, and `relocated` false.
  - `newer_anchor_replaces_one_under_way`: a gated engine and two `anchor` calls. Exactly one `Anchored` arrives, for the second list.
- [ ] **Step 2: Run** `cargo nextest run -p noido-render --test searcher anchor`. Expect failures.
- [ ] **Step 3: Implement** as above.
- [ ] **Step 4: Run** `just check`. Passes.
- [ ] **Step 5: Commit** `jj commit -m "render: anchor highlights on the text thread, a few pages at a time"`.

### Task 6: The window anchors on open, on reload and when a highlight changes

**Files:**
- Modify: `crates/noido-state/src/lib.rs` (`Slot` gains `pub edited: bool`)
- Modify: `crates/frontends/gui-egui/src/app.rs`, and the place that opens a document and passes the `Slot` (`document.rs` / `shell.rs`)

**Interfaces:**
- Consumes: Task 5's `TextSearcher::anchor` and `SearchUpdate::Anchored`; Task 1's `Highlights::anchored`.
- Produces: `Slot::edited`, true when `Store::load` found the state by path under another hash.

The window's rules:

- **When anchoring runs:**
  - when a document opens with saved highlights, with `fill_missing = !slot.edited`;
  - after `reload`, with `fill_missing = false`;
  - after a highlight is added or resized, with `fill_missing = true`.
- **Applying the answer:** the window keeps the list it sent. When `Anchored` arrives, the answer is applied with `Highlights::anchored(answer.items, answer.relocated)`, but only if `asked` equals the current `highlights.items()`. Otherwise the current list is sent again, with the same `fill_missing` as the dropped request.

- [ ] **Step 1: Write the failing tests:**
  - State: `load_by_path_is_edited`: a state saved under one key, loaded with another hash and the same path, gives `slot.edited == true`. Loading by the same key gives `false`.
  - Harness, with its `TextEngine`, the `selecting()` fixture and `highlight_the_cat`:
    - `made_highlight_gains_a_quote`: after `highlight_the_cat` and a few frames, `items()[0].quote` has `exact == "the cat"`.
    - `reload_moves_a_highlight_with_its_text`: highlight "the cat", then `reload` with an engine whose page 0 reads "so the cat …". After frames, the highlight starts at char 3.
    - `reload_orphans_a_highlight_whose_text_went`: the reloaded page lacks "the cat". The highlight is orphaned, not drawn (no highlight fill on the page), and still in `doc_state().highlights`.
    - `reload_with_fewer_pages_keeps_drawing`: a highlight on page 2, reloaded as a 1-page document. Frames run without a panic, and the highlight is orphaned or moved.
    - `change_during_anchoring_is_kept`: a gated engine. Make a highlight, make a second one before the gate opens, then open the gate. Both highlights end up with quotes, and the second isn't lost.
- [ ] **Step 2: Run** `cargo nextest run -p noido-state -p noido`. The new tests fail.
- [ ] **Step 3: Implement** the rules above. Drawing and `at_point` already skip orphans (Task 1).
- [ ] **Step 4: Run** `just check`. Passes.
- [ ] **Step 5: Commit** `jj commit -m "egui: re-anchor highlights when the document changes"`.

### Task 7: Hand-check and wrap-up

- [ ] **Step 1:** `just build`. Open a CBZ and check that its view, history and pins behave as before. Then, with a scratch `XDG_STATE_HOME`, highlight two phrases in a PDF made by a markdown → pandoc chain. Insert a paragraph above them in the source, rebuild the PDF while it's open, and see both highlights follow. Delete one phrase, rebuild, and see that highlight disappear. Restore the phrase, rebuild, and see it come back. Check that the state file has `start_part` and `quote`.
- [ ] **Step 2:** A final review of the whole branch by a fresh subagent on the most capable model. Fix Critical and Important findings test-first, as fixups squashed into their commits. Minors go to the user.
- [ ] **Step 3:** Tell the user to `just install`, because older copies drop highlights from files written in the new form. Mark L2 done.
