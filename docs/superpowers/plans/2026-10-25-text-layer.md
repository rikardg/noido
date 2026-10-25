# Text layer and search engine — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pull each page's text out with glyph positions, make it searchable the way agreed (case, NFKC, accents, across lines and hyphens), and run searches on a thread of their own that streams hits per page.

**Architecture:** `noido-engine` gets a glyph type and `Document::text` (default: no glyphs); `engine-hayro` implements it with an interpreter `Device`, as the prototype does. A new pure crate `noido-text` assembles glyphs into a `PageText` (lines, spaces, combining marks, a char → glyph map) and matches folded queries against it, giving hits with highlight rectangles and a context line. `noido-render` gets a `TextSearcher` thread, shaped like the `Thumbnailer`, that extracts on request, caches page texts, and searches from a page with wrap-around, up to 5 000 hits.

**Tech Stack:** Rust 2024, hayro 0.8 (`hayro_interpret` `Device`), `unicode-normalization` (new dependency, workspace-level).

**Spec:** `docs/superpowers/specs/2026-10-25-text-layer-design.md`

## Global Constraints

- `unsafe_code = "forbid"`; clippy clean: `cargo clippy --workspace --all-targets -- -D warnings`.
- Page space everywhere: PDF points, origin top-left, y down, page rotation applied — the same space as `Document::page_size` and rendering.
- Matching: case ignored via `char::to_lowercase` (no smart case, no full case folding); NFKC; accents ignored unless the query's NFD has a combining mark; whitespace runs match any whitespace run; line-end hyphen (`-` or `¬`) after a letter, before a lowercase letter on the next line, is removed; no whole-word option; hits don't overlap.
- Spaces inserted on a gap wider than `0.1` em; new line on a cross-baseline jump over `0.5` em or a move back over `0.5` em.
- Glyph quads: advance wide (`0.5` em when unknown), from `0.2` em below the baseline to `0.8` em above.
- Context line: about 40 characters before, 80 after, cut at word boundaries, newlines as spaces.
- At most `5 000` hits per search; the search starts at `from_page` and wraps.
- Extraction never starts before `extract()` or `search()`; opening stays as cheap as today.
- Never send synthetic input to the desktop.

## Review Focus

1. **Glyphs with no text, or several characters** (a ToUnicode ligature mapping "fi" to one glyph): no text adds nothing; each character of a multi-char glyph maps to that glyph. Tests: `empty_glyph_text_is_skipped`, `multi_char_glyph_maps_each_char` (Task 2).
2. **Text on a rotated page** (`/Rotate 90`): glyph runs along x in content space run along y in page space; assembly must still see one line, not a line per glyph. Tests: `rotated_page_text` (Task 1), `vertical_line_is_one_line` (Task 2).
3. **A query that folds to nothing** (spaces only, or only characters NFKC drops): no search, no hits, no panic. Test: `blank_query_is_none` (Task 3); `blank_query_sends_nothing` (Task 4).
4. **The hit cap landing mid-page**, and `from_page` past the end or a document with zero pages. Tests: `cap_cuts_short_mid_page`, `from_page_past_end_clamps`, `empty_document_is_done_at_once` (Task 4).
5. **A query typed with a ligature or a capital accented letter** ("ﬁle", "GÖDEL"): folded like the text, so it matches. Test: `query_is_folded_like_text` (Task 3).

---

### Task 1: Glyphs from the engine

**Files:**
- Modify: `crates/noido-engine/src/types.rs`, `crates/noido-engine/src/engine.rs`
- Modify: `crates/engine-hayro/src/lib.rs` (add a `text.rs` module if it reads better)
- Modify: `fixtures/make_fixtures.py`; create `fixtures/text.pdf` by running it
- Create: `crates/engine-hayro/tests/text.rs`

**Interfaces:**
- Produces (in `noido-engine`, exported):
  - `pub struct Point { pub x: f32, pub y: f32 }` (Clone, Copy, Debug, PartialEq)
  - `pub struct Quad { pub corners: [Point; 4] }` — order: start-bottom, end-bottom, end-top, start-top, along the writing direction; bottom is the descent side. `fn bounds(&self) -> (Point, Point)` (min, max).
  - `pub struct Glyph { pub text: String, pub origin: Point, pub size: f32, pub quad: Quad, pub invisible: bool }` — `text` empty when the font gives no Unicode; `origin` on the baseline; `size` the em in points.
  - `Document::text(&self, page: usize) -> Result<Vec<Glyph>, EngineError>` with a default body returning `Ok(Vec::new())`, so test engines need no change.

- [ ] **Step 1: Fixture.** Add `text()` to `make_fixtures.py`, writing `fixtures/text.pdf`: page 0 Letter, `/F1` = Helvetica (Type1, WinAnsiEncoding, not embedded), `BT /F1 12 Tf 72 700 Td (Hello World) Tj 0 -20 Td (noido) Tj ET`; page 1 the same content with `/Rotate 90`. Document it in the module docstring. Run the script.
- [ ] **Step 2: Write the failing tests** in `engine-hayro/tests/text.rs`:
  - `text_glyphs_in_order`: page 0's glyph texts joined, with whitespace removed, == `"HelloWorldnoido"` (whether the space arrives as a glyph is hayro's business).
  - `text_glyph_positions`: first glyph origin ≈ (72, 792 − 700) within 0.5; size ≈ 12; quad bounds: min.y ≈ 92 − 9.6, max.y ≈ 92 + 2.4; glyph x origins increase along "Hello".
  - `text_glyphs_inside_page`: every quad's bounds within `page_size(0)`.
  - `rotated_page_text`: page 1 has the same characters; their origins all share one x (±0.5) and increase in y along "Hello" (the line runs down the page); every quad within `page_size(1)`.
  - `text_on_blank_page_is_empty`: `two-pages.pdf` page 1 → `Ok(vec![])`.
  - `text_page_out_of_range`: → `Err(PageOutOfRange)`.
- [ ] **Step 3: Run** `cargo test -p engine-hayro --test text` — Expected: compile error, no `text`.
- [ ] **Step 4: Implement** the types, the default method, and `HayroDocument::text`: the prototype's `TextDevice` (`examples/extract_text.rs`), building per glyph `t = props.transform * glyph.transform()` with glyph space 1000 units/em; origin `t * (0,0)`; size `|t*(0,1000) − origin|`; quad corners `t * (0,−200)`, `t * (adv,−200)`, `t * (adv,800)`, `t * (0,800)` with `adv` the advance width in glyph units (500 when `None` or Type 3); `invisible` from `DrawMode::Invisible`; text from `as_unicode()` (`BfString`), empty if `None`. Same `Context` setup as the prototype. Check that y-down puts "top" above "bottom" on screen (`start-top.y < start-bottom.y` on page 0); if not, the glyph space is y-down and the ±200/800 signs flip — the test pins which.
- [ ] **Step 5: Run** `cargo test -p engine-hayro` — Expected: all pass.
- [ ] **Step 6: Commit** `jj commit -m "Engine: glyphs with text and position per page"`

### Task 2: `noido-text` — page text assembly

**Files:**
- Create: `crates/noido-text/{Cargo.toml,src/lib.rs,src/page.rs}`; add to workspace members and `[workspace.dependencies]` (`noido-text = { path = … }`, `unicode-normalization = "0.1"`)

**Interfaces:**
- Consumes: `Glyph`, `Point`, `Quad` from Task 1.
- Produces:
  - `pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }` in page points.
  - `pub struct PageText` built by `PageText::new(glyphs: Vec<Glyph>) -> PageText`.
  - `fn text(&self) -> &str` — lines joined with `'\n'`, NFC.
  - `fn char_count(&self) -> usize`, `fn line_of(&self, char: usize) -> usize`, `fn glyph_bounds(&self, char: usize) -> Option<Rect>` (`None` for inserted spaces and newlines). Char indices count `char`s of `text()`. The internal representation is the builder's; Task 3, in the same crate, may use private fields.

Assembly, in stream order, comparing each glyph with the previous one in that glyph's writing direction (`dir` = unit vector start-bottom → end-bottom of the previous quad, `normal` perpendicular): `along = dot(origin − prev.origin, dir)`, `across = dot(…, normal)`, `gap = along − |prev advance|`. New line if `|across| > 0.5 em` or `along < −0.5 em` (em = max of both sizes, at least 1). Else a space if `gap > 0.1 em` and neither side already is whitespace. Glyphs with empty text add nothing (but don't become `prev`).

Combining marks: a glyph whose text is a single spacing diacritic (`¨ ´ ` ˆ ^ ˜ ¸ ˇ ˚ ¯ ˘ ˙ ˝ ˛` → U+0308 0301 0300 0302 0302 0303 0327 030C 030A 0304 0306 0307 030B 0328) or a combining mark, whose bounds' horizontal centre (in writing direction) lies within the previous or the next glyph's span on the same line, becomes that combining mark after the base's text; `ı`/`ȷ` with a mark above become `i`/`j` first. The mark maps to the base glyph. Text is NFC-composed per base + marks.

- [ ] **Step 1: Write the failing tests** in `page.rs`, with a local helper `line(text, x, y, size) -> Vec<Glyph>` that lays characters out left to right at 0.5 em advance, quads as Task 1 defines:
  - `one_line_text`: `"Hello"` → `text() == "Hello"`.
  - `gap_becomes_space`; `tight_gap_is_no_space` (gap 0.05 em); `space_glyph_not_doubled`.
  - `drop_starts_new_line`: two lines 14 pt apart → `"ab\ncd"`.
  - `move_left_starts_new_line`: second column top after first column bottom (y jumps up, x right) → still a new line; next line in the same column (x back left) → new line.
  - `vertical_line_is_one_line`: glyphs whose direction is (0, 1) with origins stepping down by 0.5 em → one line.
  - `empty_glyph_text_is_skipped`.
  - `multi_char_glyph_maps_each_char`: one glyph with text `"fi"` → both chars' `glyph_bounds` equal that glyph's bounds.
  - `separate_diaeresis_joins_base`: `"na"`, `"ı"`, `"¨"` over the `ı`, `"ve"` → `"naïve"`, and the `ï` maps to the `ı` glyph.
  - `diaeresis_before_base_joins_base`: mark drawn before its base in the stream.
  - `inserted_space_has_no_glyph`.
- [ ] **Step 2: Run** `cargo test -p noido-text` — Expected: compile errors.
- [ ] **Step 3: Implement** the crate as above.
- [ ] **Step 4: Run** `cargo test -p noido-text` — Expected: pass.
- [ ] **Step 5: Commit** `jj commit -m "Text: assemble glyphs into lines and words"`

### Task 3: `noido-text` — matching

**Files:**
- Create: `crates/noido-text/src/search.rs`

**Interfaces:**
- Consumes: `PageText` (Task 2).
- Produces:
  - `pub struct Query` by `Query::new(text: &str) -> Option<Query>` — `None` if it folds to empty or whitespace only. Holds the folded query and whether it has accents.
  - `PageText::find(&self, query: &Query) -> Vec<TextHit>` in text order, non-overlapping.
  - `pub struct TextHit { pub range: Range<usize> /* chars of text() */, pub rects: Vec<Rect>, pub context: Context }`
  - `pub struct Context { pub text: String, pub bold: Range<usize> /* chars of text */ }`

Search forms built lazily once per `PageText` (`OnceCell`): per page-text char, in order — skip a removed line-end hyphen and its newline; collapse whitespace runs to one `' '` (mapped to the run's first char); else NFKC the char, lowercase each result char; for the unaccented form, NFD each and drop combining marks. Each folded char records its source char index. A query is folded by the same per-char function (whitespace trimmed at both ends). Hit `range` = source index of its first folded char .. source index of its last + 1 (extend to include a whole multi-char source when NFKC expanded one).

Rects: per line touched, the union of `glyph_bounds` of the range's chars on that line; lines with no glyph-backed char give none.

- [ ] **Step 1: Write the failing tests** in `search.rs`, building pages with Task 2's helper (move it to a `#[cfg(test)] mod testing` shared by both modules):
  - `case_is_ignored`: "Kant" in "kant KANT" → 2 hits; `"kant"` in "Kant" → 1.
  - `ligature_matches_letters`: text glyph `"ﬁ"` + `"le"` found by `"file"`; `query_is_folded_like_text`: `"ﬁle"` finds `"file"`, `"GÖDEL"` finds `"Gödel"`.
  - `accents_ignored_without_accent_in_query`: `"godel"` finds `"Gödel"` and `"Godel"`; `accents_kept_with_accent_in_query`: `"gödel"` finds `"Gödel"`, not `"Godel"`.
  - `matches_across_line_break`: `"black cat"` in `"black\ncat"`.
  - `hyphen_at_line_end_joins_word`: `"phenom-\nenology"` found by `"phenomenology"`; `"¬"` likewise; `capital_after_hyphen_keeps_it`: `"Anglo-\nSaxon"` found by `"anglo-saxon"`, not `"anglosaxon"`; `mid_line_hyphen_stays`: `"self-induced"` found by `"self-induced"`.
  - `whitespace_runs_match_one_space`: `"a  b"` (two space glyphs) found by `"a b"`; query `"a   b"` finds `"a b"`.
  - `hits_do_not_overlap`: `"aa"` in `"aaaa"` → 2 hits.
  - `blank_query_is_none`: `Query::new("  ")` and `Query::new("")` → `None`.
  - `hit_rects_one_per_line`: a hit across a line break → 2 rects, each within its line's glyph bounds; a hit on one line → 1 rect spanning its glyphs.
  - `context_has_bold_hit`: hit in a long line; `context.text[bold]` (as chars) equals the matched text; context cut at word boundaries, ≤ ~130 chars; `context_at_page_start` and `context_with_newline_is_one_line`.
- [ ] **Step 2: Run** `cargo test -p noido-text` — Expected: compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p noido-text` — Expected: pass.
- [ ] **Step 5: Commit** `jj commit -m "Text: search with folding, accents and de-hyphenation"`

### Task 4: `TextSearcher` thread

**Files:**
- Create: `crates/noido-render/src/text.rs` (export from `lib.rs`); add `noido-text` dependency
- Create: `crates/noido-render/tests/searcher.rs`
- Create: `crates/noido-render/examples/search_text.rs`

**Interfaces:**
- Consumes: `Document::text` (Task 1), `PageText`, `Query`, `TextHit` (Tasks 2–3).
- Produces:
  - `pub const MAX_HITS: usize = 5000;`
  - `pub enum SearchUpdate { Page { generation: u64, page: usize, hits: Vec<TextHit> }, Done { generation: u64, cut_short: bool } }`
  - `pub struct TextSearcher` with `start(engine: Arc<dyn PdfEngine>, bytes: PdfBytes, waker: Box<dyn Fn() + Send>) -> TextSearcher` (non-blocking, like `Thumbnailer::start`), `extract(&self)`, `search(&mut self, query: &str, from_page: usize) -> u64` (returns the new generation; an empty/blank query cancels and sends nothing), `try_recv(&self) -> Option<SearchUpdate>`.

Thread: opens its own document; waits for commands. `Extract` sets "extract in background": while no search runs, extract the next unextracted page in order, checking for commands between pages. `Search` (newest wins; drain commands between pages): from `min(from_page, count − 1)`, every page once with wrap-around; extract if needed; send `Page` (also with no hits) and wake; stop at `MAX_HITS` total (truncate that page's hits), then `Done { cut_short: true }`; else `Done { cut_short: false }`. Zero pages → `Done` at once. A page whose `text()` fails counts as empty (eprintln `noido: text of page N: {e}`). Page texts cached for the thread's life. Dropping the handle ends the thread.

- [ ] **Step 1: Write the failing tests** in `tests/searcher.rs` with a fake engine: pages from a `Vec<&str>` laid out as one line each (a local glyph helper), a shared `AtomicUsize` counting `text()` calls, optional failing page:
  - `nothing_extracted_until_asked`: start, wait 100 ms → 0 calls.
  - `extract_reads_every_page_in_background`: `extract()` → calls reach the page count.
  - `search_starts_at_from_page_and_wraps`: 5 pages, from 3 → `Page` updates in order 3, 4, 0, 1, 2, then `Done { cut_short: false }`.
  - `search_finds_hits_per_page`: "cat" on pages 1 and 3 → those `Page` updates carry 1 hit each, others 0.
  - `newer_search_drops_older`: two searches back to back → every update after the first update of generation 2 has generation 2, and generation 2 ends with `Done`.
  - `blank_query_sends_nothing`: `search("  ", 0)` → no update within 100 ms.
  - `failing_page_has_no_hits`: page 2 fails → `Page { page: 2, hits: [] }`, search completes.
  - `cap_cuts_short_mid_page`: 3 pages each with 3 000 "a" hits (`"a a a …"`) → total hits delivered == `MAX_HITS`, `Done { cut_short: true }`.
  - `from_page_past_end_clamps`; `empty_document_is_done_at_once`.
  - `text_cached_across_searches`: two searches over 3 pages → `text()` calls == 3.
  - `wakes_per_update`.
- [ ] **Step 2: Run** `cargo test -p noido-render --test searcher` — Expected: compile error.
- [ ] **Step 3: Implement** `text.rs`.
- [ ] **Step 4: Run** `cargo test -p noido-render` — Expected: pass.
- [ ] **Step 5: Example** `search_text <file.pdf> <query>…`: starts a `TextSearcher` on `HayroEngine`, runs each query from page 0, prints per query the hit count, the first three context lines (bold part in `[…]`), and the time to `Done`. Run it on `../testfiles/tracemonkey.pdf` with `trace`, and on the Dennett and Vaesen books with a common word; Expected: the first search completes within a few times the investigation's extraction timings (Dennett ~0.16 s, Vaesen ~0.33 s, release build), later ones much faster. Note the timings in the ledger.
- [ ] **Step 6: Clippy and workspace tests:** `cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` — Expected: clean, all pass.
- [ ] **Step 7: Commit** `jj commit -m "Render: text search thread with per-page hits"`
