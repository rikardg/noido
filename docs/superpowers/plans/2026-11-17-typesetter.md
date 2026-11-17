# The typesetter behind `Document` (Y3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A markdown file opens in noido as typeset pages (Knuth–Plass,
hyphenation, careful page breaks) behind the `Document` trait, with
glyphs, locations, links and outline that work like a PDF's.

**Architecture:** A new crate `noido-typeset` turns a `noido_blocks::Tree`
into a `Book` (pages of placed glyphs, location runs, links, headings)
once per file, shared by the window's threads, and implements
`Document` over it. Fonts come from a new external crate `noido-fonts`
(its own repo, one Cargo feature per family). Small changes in
`noido-engine` (location map with page offsets, layout key, markdown
format), `noido-text` (marked hyphens), `noido-render` and `gui-egui`
(wiring, dialogs).

**Tech Stack:** Rust 2024; parley 0.13 (shaping, fontique collection
without system fonts), hypher 0.1.8, skrifa (outlines), tiny-skia 0.12,
flate2.

**Spec:** `docs/superpowers/specs/2026-11-17-typesetter-design.md`

## Global Constraints

- TDD: every behaviour's test is written first and watched failing.
- Commits with `jj commit -m`, one per task unless a task says more;
  prefixes by crate: `fonts` (the noido-fonts repo), `engine`, `text`,
  `render`, `typeset`, `egui`, `Repo`. Titles ≤ 72 characters.
- `just check` (tests, clippy clean, fmt) passes at the end of every
  task in repo-2.
- Tests and examples use only `fixtures/` and inline strings, never
  `../testfiles/`.
- Paper: B-format, 129 × 198 mm = 365.67 × 561.26 pt. Text 11 pt on
  14 pt lines. Measure 270 pt, centred. 31 lines. Top margin 56 pt.
- Fonts: Libron 0.32 (regular, italic, bold, bold italic) body;
  Libertinus Serif 7.051 (same four) fallback; New Computer Modern 8.1.1
  `NewCM10-{Book,BookItalic,Bold,BoldItalic}` last fallback; Libertinus
  Sans 7.051 (regular, italic, bold) headings; Monaspace Neon 1.400
  (regular, italic, bold, bold italic) code. No system fonts.
- Knuth–Plass: stretch ½ space, shrink ⅓ space, tolerance 2, line
  penalty 10, 3000 demerits for consecutive hyphens, 100 for adjacent
  lines of different fitness.
- Locations: a markdown file is part 0; locations count source
  characters. A page's characters are `PageText::new`'s (NFC, newline
  between lines, a space inserted for a gap > 0.1 em). Every other gap
  on a line stays under 0.1 em; every space is a glyph `" "`.
- PDF behaviour (de-hyphenation, identity map, opening) is unchanged.
- Never move the `main` bookmark.

## Review Focus

1. An empty markdown file, or one with only front matter: it opens as
   one blank page, not zero pages and not an error.
2. A word or URL wider than the measure (no break point): the line
   overflows the measure; nothing panics and no characters are lost.
3. A paragraph longer than a page: it splits across pages, keeping the
   widow and orphan rule; locations stay in order across the split.
4. Characters no bundled font has (emoji, CJK): drawn as `.notdef`,
   kept in `text()` so search and copy see them.
5. CRLF line endings and a BOM: source character counts and runs stay
   right (the guarantee test passes on a CRLF copy of the sample).

---

## File Structure

**New repo `noido-fonts`** at `/Users/rikard/privat/hack/noido/noido-fonts`
(jj, colocated; remote `https://github.com/rikardg/noido-fonts`):

- `Cargo.toml`: crate `noido-fonts`, features per family.
- `build.rs`: deflates the enabled families' files into `OUT_DIR`.
- `src/lib.rs`: `Face`, `faces()`, `Licence`, `licences()`.
- `fonts/<family>/`: upstream originals, `LICENSE`/`OFL.txt`,
  `SOURCE.md` (URL, version, files).
- `README.md`.

**repo-2:**

- `crates/noido-engine/src/locations.rs`: `from_placed`, gap handling.
- `crates/noido-engine/src/engine.rs`: `Document::layout_key`.
- `crates/noido-engine/src/types.rs`: `Format::Markdown`.
- `crates/noido-text/src/page.rs`, `copy.rs`, `search.rs`: marked hyphens.
- `crates/noido-render/src/lib.rs`, `transformed.rs`, `text.rs`: layout
  key in `DocumentInfo`; marked hyphens for reflowing documents.
- `crates/noido-typeset/` (new):
  - `src/lib.rs`: module list, re-exports `TypesetEngine`, `typeset`,
    `Book`.
  - `src/house.rs`: the house style's constants.
  - `src/fonts.rs`: unpacking, parley collection, skrifa refs.
  - `src/paragraphs.rs`: tree → paragraphs, Y3b fallbacks, front matter.
  - `src/kp.rs`: Knuth–Plass (from the investigation's prototype).
  - `src/lines.rs`: shaping, items, hyphenation, set lines.
  - `src/pages.rs`: page breaking.
  - `src/runs.rs`: location runs for a line.
  - `src/book.rs`: `Book` assembly, glyphs, runs, links, outline.
  - `src/links.rs`: slugs and link targets.
  - `src/draw.rs`: rendering with an outline cache.
  - `src/engine.rs`: `TypesetEngine`, `TypesetDocument`.
  - `tests/sample.rs`: whole-pipeline checks on `fixtures/sample.md`.
- `crates/frontends/gui-egui/src/document.rs`, `open.rs`, `app.rs`;
  `crates/noido-macos/src/open_panel.rs`.
- `Cargo.toml` (workspace): member and dependencies.

---

### Task 1: The `noido-fonts` crate

**Files (in `/Users/rikard/privat/hack/noido/noido-fonts`):**
- Create: `Cargo.toml`, `build.rs`, `src/lib.rs`, `README.md`,
  `fonts/{libron,libertinus-serif,libertinus-sans,newcm,monaspace-neon,alegreya-sans}/…`

**Interfaces:**
- Produces:
  ```rust
  pub struct Face {
      pub family: &'static str,   // "Libron", "Libertinus Serif", "NewComputerModern", …, as the font names itself
      pub italic: bool,
      pub weight: u16,            // 400, 700, …
      pub deflated: &'static [u8] // zlib (flate2 ZlibEncoder) of the file
  }
  pub fn faces() -> &'static [Face];          // enabled families only
  pub struct Licence { pub family: &'static str, pub text: &'static str }
  pub fn licences() -> &'static [Licence];
  ```
  Features: `libron`, `libertinus-serif`, `libertinus-sans`, `newcm`,
  `monaspace-neon`, `alegreya-sans`; `default = []`.

- [ ] **Step 1: Make the checkout.** `jj git init --colocate` at the
  path above; `jj git remote add origin https://github.com/rikardg/noido-fonts`.
  Copy the upstream files from the scratchpad downloads
  (`…/scratchpad/libron/x/`, `…/fonts/libertinus/x/Libertinus-7.051/static/OTF/`,
  `…/fonts/newcm/x/newcomputermodern/otf/`,
  `…/fonts/monaspace/x/Static Fonts/Monaspace Neon/`, `…/fonts/gf/AlegreyaSans-*`)
  with each family's licence (Libron's from its repo `LICENSE`; Alegreya
  Sans's `OFL.txt` from google/fonts `ofl/alegreyasans/OFL.txt`) and a
  `SOURCE.md` giving URL, version and file list as in the spec's table
  (Alegreya Sans: google/fonts `ofl/alegreyasans`, regular, italic,
  bold).
- [ ] **Step 2: Write the failing tests** in `src/lib.rs`:
  ```rust
  #[test] #[cfg(feature = "libron")]
  fn libron_has_four_faces() {
      let f: Vec<_> = faces().iter().filter(|f| f.family == "Libron").collect();
      assert_eq!(f.len(), 4);
      assert!(f.iter().any(|f| f.italic && f.weight == 700));
  }
  #[test] #[cfg(feature = "libron")]
  fn a_face_inflates_to_a_font() {
      let face = faces().iter().find(|f| f.family == "Libron").unwrap();
      let bytes = inflate(face.deflated); // test helper with flate2
      assert!(bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"OTTO"));
  }
  #[test]
  fn without_features_there_are_no_faces() { /* cfg(not(any(feature = …))) */ }
  #[test] #[cfg(feature = "libron")]
  fn every_family_has_its_licence() {
      for face in faces() { assert!(licences().iter().any(|l| l.family == face.family)); }
  }
  ```
- [ ] **Step 3: Run** `cargo test --all-features` and `cargo test`.
  Expected: FAIL (functions missing).
- [ ] **Step 4: Implement.** `build.rs` reads `CARGO_FEATURE_*`, deflates
  each enabled family's files (flate2 as a build-dependency, level 9)
  into `OUT_DIR`, and writes a generated `faces.rs` with the table,
  `include_bytes!` of each deflated file and `include_str!` of the
  licences; `src/lib.rs` includes it. `family` is the font's own
  typographic family name (read it from the file in the build script or
  write it in the table; the test asserts it).
- [ ] **Step 5: Run** both test commands. Expected: PASS.
- [ ] **Step 6: Commit and publish.** `jj commit -m "fonts: six families, one feature each, deflated at build time"`,
  `jj bookmark create main -r @-`, `jj git push --bookmark main`, then
  tag `v0.1.0` on that commit and push the tag
  (`git tag v0.1.0 <commit> && git push origin v0.1.0`).

### Task 2: Location map with page offsets, layout key, markdown format

**Files:**
- Modify: `crates/noido-engine/src/locations.rs`, `engine.rs`, `types.rs`

**Interfaces:**
- Produces:
  ```rust
  impl LocationMap {
      /// Each page's runs with their offsets among the page's characters.
      pub fn from_placed(pages: Vec<Vec<(usize, Run)>>) -> Self;
  }
  // Document:
  fn layout_key(&self) -> Option<String> { None }
  // Format:
  Markdown,
  ```
  `from_lines` stays, delegating with summed offsets.

- [ ] **Step 1: Write the failing tests** in `locations.rs`:
  ```rust
  /// Page 0: "ab\ncd": a run of 2 at offset 0 (chars 10..12), a run of 2
  /// at offset 3 (chars 12..14); the newline at 2 has no source.
  fn with_gap() -> LocationMap {
      LocationMap::from_placed(vec![vec![(0, run(0, 10, 2, 36.0)), (3, run(0, 12, 2, 50.0))]])
  }
  #[test] fn a_placed_run_keeps_its_offset() {
      assert_eq!(with_gap().location(0, 4), Some(at(0, 13)));
      assert_eq!(with_gap().page_char(at(0, 13)), Some((0, 4)));
  }
  #[test] fn a_character_in_a_gap_is_the_next_runs_start() {
      assert_eq!(with_gap().location(0, 2), Some(at(0, 12)));
  }
  #[test] fn from_lines_still_sums() { assert_eq!(two_pages(), /* same runs via from_placed with offsets 0 and 40 */); }
  ```
  In `engine.rs`: `a_document_has_no_layout_key_by_default`
  (`assert_eq!(OnePage.layout_key(), None)`).
- [ ] **Step 2: Run** `cargo test -p noido-engine`. Expected: FAIL to
  compile (`from_placed`, `layout_key` missing).
- [ ] **Step 3: Implement.** `from_placed` stores the given offsets
  (debug-assert: offsets non-decreasing and runs not overlapping on the
  page, runs in source order as now). `location`: a `char` before a
  run's offset (in a gap) maps to that run's start. Add
  `Format::Markdown` and `layout_key`.
- [ ] **Step 4: Run** `cargo test -p noido-engine`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: runs placed at offsets, a layout key, markdown format`.

### Task 3: The layout key reaches the window

**Files:**
- Modify: `crates/noido-render/src/lib.rs` (`DocumentInfo`, `open`),
  `crates/noido-render/src/transformed.rs` (forward `layout_key`),
  `crates/frontends/gui-egui/src/app.rs` (restore at ~1257, save at
  ~1299, reload at ~1164)

**Interfaces:**
- Consumes: `Document::layout_key` (Task 2).
- Produces: `DocumentInfo { …, pub layout_key: Option<String> }`; the
  app holds `self.layout_key: Option<String>` beside `self.locations`.

- [ ] **Step 1: Write the failing test** in `app.rs`'s harness tests: a
  `FlowEngine` variant whose document returns
  `Some("paper-a".into())`; a view saved there and restored under an
  engine returning `Some("paper-b".into())` restores through its anchor:
  `text_that_reflows_restores_by_anchor_under_a_new_layout_key` — after
  scrolling to page 5 under `paper-a`, reopening under `paper-b` puts the
  anchor's character on screen (assert the view's first page is the
  page holding the saved anchor in the new map; use a map whose pages
  hold 50 characters instead of 100 so the page differs).
- [ ] **Step 2: Run** `cargo nextest run -p gui-egui text_that_reflows_restores_by_anchor`.
  Expected: FAIL.
- [ ] **Step 3: Implement** the field and pass `self.layout_key.as_deref()`
  where `None` is passed today.
- [ ] **Step 4: Run** `just test`. Expected: PASS.
- [ ] **Step 5: Commit** `render: carry the document's layout key to the window`.

### Task 4: Marked hyphens on reflowing pages

**Files:**
- Modify: `crates/noido-text/src/page.rs`, `copy.rs`, `search.rs`;
  `crates/noido-render/src/text.rs:620-635`

**Interfaces:**
- Produces: `PageText::with_marked_hyphens(glyphs: Vec<Glyph>) -> PageText`
  and `PageText::marked_hyphens(&self) -> bool`. On such a page a
  line-end `-` or `‐` belongs to its word; only `\u{ad}` (and `¬`)
  break one. Pages from `PageText::new` behave as today.
- The typesetter (Task 8) draws an added hyphen as a glyph whose text is
  `"\u{ad}"`.

- [ ] **Step 1: Write the failing tests** in `copy.rs` and `search.rs`:
  ```rust
  #[test] fn a_marked_page_keeps_a_source_hyphen_at_a_line_end() {
      let p = PageText::with_marked_hyphens(page(&["the well-", "known cat"]));
      assert_eq!(all(&p), "the well-known cat");
  }
  #[test] fn a_marked_page_drops_a_soft_hyphen_at_a_line_end() {
      let p = PageText::with_marked_hyphens(page(&["the po\u{ad}", "litical cat"]));
      assert_eq!(all(&p), "the political cat");
  }
  #[test] fn search_on_a_marked_page_finds_a_source_hyphenated_word() { /* "well-known" found, "wellknown" not */ }
  #[test] fn search_on_a_marked_page_finds_a_word_split_by_a_soft_hyphen() { /* "political" found */ }
  ```
  Existing PDF tests stay as they are.
- [ ] **Step 2: Run** `cargo test -p noido-text`. Expected: FAIL.
- [ ] **Step 3: Implement**: the flag on `PageText`, threaded into
  `line_end_hyphen` (and `join` across pages) so a marked page treats
  `-`/`‐` as `Hyphen::Belongs`. In `noido-render/src/text.rs`, the text
  thread builds pages with `with_marked_hyphens` when
  `document.locations().reflows()` (asked once when the thread opens the
  document).
- [ ] **Step 4: Run** `just test`. Expected: PASS.
- [ ] **Step 5: Commit** `text: on reflowing pages only soft hyphens break words`.

### Task 5: `noido-typeset` crate, house style, fonts

**Files:**
- Create: `crates/noido-typeset/Cargo.toml`, `src/lib.rs`, `src/house.rs`, `src/fonts.rs`
- Modify: workspace `Cargo.toml` (member; deps `parley = { version = "0.13", default-features = false, features = ["std"] }`,
  `hypher = "0.1.8"`, `skrifa` (the version parley 0.13 uses),
  `noido-fonts = { git = "https://github.com/rikardg/noido-fonts", tag = "v0.1.0" }`)

**Interfaces:**
- Produces:
  ```rust
  // house.rs: pub(crate) consts
  PAPER: Size = Size { width: 365.67, height: 561.26 };
  MEASURE: f32 = 270.0; LEFT: f32 = (PAPER.width - MEASURE) / 2.0;
  TOP: f32 = 56.0; LEADING: f32 = 14.0; LINES: usize = 31;
  BODY: f32 = 11.0; HEADING: [f32; 6] = [18.0, 14.0, 12.0, 11.0, 11.0, 11.0];
  CODE_SCALE: f32 = 0.85; INDENT_EM: f32 = 1.0;
  LAYOUT_KEY: &str = "b-format 11/14";
  // fonts.rs
  pub(crate) struct Fonts { /* parley FontContext (no system fonts), skrifa data per face */ }
  pub(crate) fn fonts() -> &'static Fonts;       // OnceLock; unpacked on first call
  pub(crate) enum Family { Body, Heading, Code } // → parley font stacks:
  //   Body: "Libron, Libertinus Serif, NewComputerModern"
  //   Heading: "Libertinus Sans, Libertinus Serif, NewComputerModern"
  //   Code: "Monaspace Neon, NewComputerModern"
  pub(crate) struct FaceId(pub u16);              // index into the unpacked faces
  impl Fonts { pub(crate) fn data(&self, face: FaceId) -> &[u8]; }
  ```
  noido-fonts features on: `libron`, `libertinus-serif`,
  `libertinus-sans`, `newcm`, `monaspace-neon`.

- [ ] **Step 1: Write the failing tests** in `fonts.rs`:
  `fonts_unpack_once` (`std::ptr::eq(fonts(), fonts())`),
  `every_family_stack_resolves_to_a_bundled_face` (shape "a" with each
  `Family` through parley; the run's font is one of ours),
  `no_system_font_is_seen` (shape "a" with stack "Helvetica"; it
  resolves to a bundled face or nothing, never a system font).
- [ ] **Step 2: Run** `cargo test -p noido-typeset`. Expected: FAIL.
- [ ] **Step 3: Implement** (fontique `Collection` with
  `system_fonts: false`; register each inflated face).
- [ ] **Step 4: Run** `cargo test -p noido-typeset`. Expected: PASS.
- [ ] **Step 5: Commit** `typeset: a crate for the typesetter, with its fonts`.

### Task 6: Knuth–Plass

**Files:**
- Create: `crates/noido-typeset/src/kp.rs` (port of
  `docs/investigations/2026-11-15-typesetting/src/kp.rs`)

**Interfaces:**
- Produces: `pub(crate) enum Item { Box{width, cluster: Option<usize>}, Glue{width, stretch, shrink, cluster: Option<usize>}, Penalty{width, penalty: i32, flagged: bool} }`,
  `pub(crate) struct Params` (Default = the Global Constraints' values),
  `pub(crate) fn breaks(items: &[Item], line_width: f32, params: &Params) -> Vec<(usize, f32)>`
  (break item indices and adjustment ratios; the last break is the
  paragraph's end), `pub(crate) const INFINITY: i32 = 10_000`.

- [ ] **Step 1: Write the failing tests** (fixed widths, no fonts):
  - `one_line_when_it_fits`: three boxes of 10 with glue 3/1.5/1 and the
    finishing glue + forced penalty, width 100 → one break at the end.
  - `breaks_balance_lines`: a sequence where greedy would leave a loose
    second line; assert the ratios are all within ±1.
  - `a_hyphen_is_taken_when_it_lowers_demerits`: a flagged penalty 50
    inside a long box pair; assert the chosen break is at it.
  - `two_hyphens_in_a_row_cost_more`: with two equal options, the one
    without consecutive flagged breaks wins.
  - `an_impossible_paragraph_is_still_set`: a box wider than the line →
    breaks returned, every item on some line, no panic.
- [ ] **Step 2: Run** `cargo test -p noido-typeset kp`. Expected: FAIL.
- [ ] **Step 3: Implement** by porting the prototype, adding the
  fallback: if no feasible break exists at `tolerance`, retry with
  tolerance ∞ (taking the least bad breaks).
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: Knuth–Plass line breaking`.

### Task 7: Paragraphs from the block tree

**Files:**
- Create: `crates/noido-typeset/src/paragraphs.rs`

**Interfaces:**
- Consumes: `noido_blocks::{Tree, Block, BlockKind, Text, Style}`.
- Produces:
  ```rust
  pub(crate) enum Role { Body { indent: bool }, Heading(u8), Code }
  pub(crate) struct Para {
      pub role: Role,
      pub text: Text,             // the block's own Text (source map intact)
      pub prefix: Option<String>, // drawn before the text, no source: "• ", "3. ", footnote "1 "
      pub new_page: bool,         // a level-1 heading
  }
  /// Footnote label → its number: the order of its first reference.
  pub(crate) struct Notes(pub std::collections::HashMap<String, usize>);
  pub(crate) fn paragraphs(tree: &Tree) -> (Vec<Para>, Notes);
  pub(crate) struct Front { pub title: Option<String>, pub author: Option<String>, pub lang: Option<String> }
  pub(crate) fn front_matter(tree: &Tree, source: &str) -> Front;
  ```
  Order is source order. Indent: `Body { indent: false }` for the first
  body paragraph and the first after a heading; true otherwise. Lists:
  each item's paragraphs, the first with its marker (`"• "` or
  `"{n}. "`, numbered from `start`); a task item's U+FFFC is left in
  `text` (Task 9 draws ☐/☑). Quote: its paragraphs as body. Code: one
  `Role::Code` para. Table: each cell is its own `Text`, and joining
  them would break the source map, so each cell is its own unindented
  body para, head first, rows in order (a deviation from the spec's
  "a paragraph per row"; ledger it). Rule: an empty body para
  (`Text::default()`). Footnote: its blocks' paras where defined, the
  first with prefix `"{n} "` from `Notes`. Html, FrontMatter: nothing.
  Front matter: slice `source` by the block's span (characters), read
  top-level `key: value` lines for `title`, `author`, `lang`,
  `language` (lang wins), quotes stripped.

- [ ] **Step 1: Write the failing tests**, each parsing an inline
  markdown string with `noido_blocks::markdown::parse`:
  - `paragraphs_come_in_source_order_with_book_indents`:
    `"# T\n\nOne.\n\nTwo.\n"` → roles `[Heading(1), Body{indent:false}, Body{indent:true}]`, heading `new_page`.
  - `list_items_carry_their_markers`: `"- a\n- b\n"` → prefixes
    `["• ", "• "]`; `"3. a\n4. b\n"` → `["3. ", "4. "]`.
  - `a_footnote_stays_where_it_is_defined`: `"A[^x].\n\n[^x]: Note.\n\nB.\n"`
    → three paras; the second is the note's (`text.string == "Note."`)
    with prefix `Some("1 ")`; `Notes` maps `"x"` to 1.
  - `html_and_front_matter_are_not_set`.
  - `front_matter_gives_title_author_and_lang`:
    `"---\ntitle: \"On Liberty\"\nauthor: J. S. Mill\nlang: sv\n---\n\nText.\n"`
    → `Front { title: Some("On Liberty"), author: Some("J. S. Mill"), lang: Some("sv") }`.
  - `nested_front_matter_is_ignored`: a `tags:\n  - a` list → no panic,
    title still read.
- [ ] **Step 2: Run** `cargo test -p noido-typeset paragraphs`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: the block tree as paragraphs, with simple fallbacks`.

### Task 8: Shaping and setting lines

**Files:**
- Create: `crates/noido-typeset/src/lines.rs`

**Interfaces:**
- Consumes: `Para`, `Role` (Task 7), `fonts()`, `Family`, `FaceId`
  (Task 5), `kp::*` (Task 6), house constants.
- Produces:
  ```rust
  pub(crate) struct SetGlyph {
      pub face: FaceId, pub id: u16, pub size: f32,
      pub x: f32,                  // from the line's left edge
      pub rise: f32,               // baseline shift (footnote numbers), 0 otherwise
      pub text: String,            // what it stands for; " " for a space; "\u{ad}" for an added hyphen
      pub drawn: Option<std::ops::Range<usize>>, // bytes of para.text.string; None for prefix and added hyphen
  }
  pub(crate) enum Decoration { Underline, Strike }
  pub(crate) struct SetLine {
      pub glyphs: Vec<SetGlyph>,
      pub decorations: Vec<(Decoration, f32, f32)>, // x0, x1
      pub links: Vec<(String, f32, f32)>,           // dest, x0, x1
      pub height: f32, pub ascent: f32,
  }
  pub(crate) fn set(para: &Para, notes: &Notes, lang: hypher::Lang) -> Vec<SetLine>;
  #[cfg(test)] pub(crate) fn set_at(para: &Para, notes: &Notes, lang: hypher::Lang, width: f32) -> Vec<SetLine>;
  ```
  Styles: Emphasis → italic, Strong → weight 700, Code → `Family::Code`
  at `CODE_SCALE` em; Strikethrough → `Decoration::Strike`; Link →
  `Decoration::Underline` (hairline) + `links`; FootnoteRef → its
  number from `Notes`, size × 0.7, rise 0.33 em; Image
  U+FFFC → its alt text in italic; TaskBox U+FFFC → "☐"/"☑". Those
  replacements have `drawn` = the U+FFFC's byte range. Body: justified
  Knuth–Plass with `hypher` points (not inside Code style), the first
  line indented 1 em when `indent`. Heading: `Family::Heading` bold at
  `HEADING[level-1]`, ragged, no hyphenation. Code role: Neon at
  `CODE_SCALE × BODY`, a line per source line, a long line wrapped at
  the measure at any character, no justification.

- [ ] **Step 1: Write the failing tests** (on `paragraphs(parse(…))`):
  - `a_long_paragraph_is_justified_to_the_measure`: every line but the
    last ends within 0.5 pt of `MEASURE` (last glyph's x + advance).
  - `spaces_are_glyphs_and_other_gaps_stay_under_a_tenth_em`: for every
    line, consecutive glyphs either include a `" "` glyph or their gap is
    < 0.1 × size.
  - `an_added_hyphen_is_a_soft_hyphen_without_source`: a narrow
    paragraph of long words (set with `set_at` at width 60) has a line ending in a glyph with text
    `"\u{ad}"` and `drawn: None`.
  - `inline_code_is_not_hyphenated`.
  - `a_word_wider_than_the_measure_overflows_without_loss` (Review
    Focus 2): every drawn byte of the text appears in some glyph's
    `drawn`.
  - `a_character_no_font_has_is_kept` (Review Focus 4): `"a 🙂 b"` →
    a glyph with text `"🙂"`.
  - `code_keeps_its_lines`: ```` "```\na\nbb\n```\n" ```` → two lines.
  - `a_footnote_reference_is_raised_and_smaller`.
- [ ] **Step 2: Run** `cargo test -p noido-typeset lines`. Expected: FAIL.
- [ ] **Step 3: Implement** (shape each paragraph as one parley line with
  styled ranges; clusters → items: box per non-space cluster run, glue
  per space, flagged penalty per hyphenation point with the hyphen's
  width, unflagged zero penalty after existing `-`/`–`/`—`; place
  glyphs at the chosen breaks, spaces stretched by the ratio).
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: shape paragraphs and set their lines`.

### Task 9: Page breaking

**Files:**
- Create: `crates/noido-typeset/src/pages.rs`

**Interfaces:**
- Produces (pure; no fonts):
  ```rust
  pub(crate) struct Flow { pub lines: Vec<f32>, pub before: f32, pub after: f32, pub heading: bool, pub new_page: bool }
  pub(crate) struct Placed { pub flow: usize, pub line: usize, pub page: usize, pub y: f32 } // y = line top on the page
  pub(crate) fn paginate(flows: &[Flow], top: f32, height: f32) -> (Vec<Placed>, usize /*page count*/);
  ```
  Body text height is `LINES × LEADING` = 434 pt from `TOP`. Space
  before: one line above a level-2 heading, half a line above levels
  3–6, half a line below all headings; dropped at a page's top.

- [ ] **Step 1: Write the failing tests** (lines of 14 on a 10-line page):
  - `no_paragraph_leaves_one_line_at_the_foot`: a 9-line paragraph,
    then a 3-line one → the 3-line paragraph goes whole to page 2
    (page 1: 9 lines; page 2: 3).
  - `no_paragraph_leaves_one_line_at_the_head`: an 8-line paragraph,
    then a 3-line one → only 1 of its lines moves would leave it alone
    at the head, so page 1 holds 8 + 1 and page 2 the last 2
    (page 1: 9 lines; page 2: 2).
  - `a_heading_takes_two_lines_with_it`: a heading at the foot with
    room for it and one line → heading moves to the next page.
  - `a_level_one_heading_starts_a_page` unless first on its page.
  - `a_paragraph_longer_than_a_page_splits` (Review Focus 3): 25
    lines → pages of 10, 10, 5, placements in order; and 21 lines →
    10, 9, 2 (no line alone at the head).
  - `nothing_gives_one_blank_page` (Review Focus 1): no flows → page
    count 1.
- [ ] **Step 2: Run** `cargo test -p noido-typeset pages`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: break the lines into pages`.

### Task 10: The book: glyphs and location runs

**Files:**
- Create: `crates/noido-typeset/src/runs.rs`, `src/book.rs`, `tests/sample.rs`

**Interfaces:**
- Consumes: Tasks 2, 7, 8, 9.
- Produces:
  ```rust
  // runs.rs, pure:
  /// A line's page characters in order, each with its source character
  /// (None: no source). Returns (offset in the line, Run) pieces.
  pub(crate) fn runs(chars: &[Option<usize>], top: f32, bottom: f32) -> Vec<(usize, Run)>;
  // book.rs:
  pub struct Book { /* pages */ }
  pub fn typeset(source: &str) -> Book;
  impl Book {
      pub fn page_count(&self) -> usize;
      pub fn glyphs(&self, page: usize) -> Vec<noido_engine::Glyph>;
      pub fn locations(&self) -> LocationMap;   // from_placed, part 0
  }
  ```
  Glyph per `SetGlyph`, in reading order; `origin` at the baseline, `size`
  the glyph's, `quad` its advance box from descent to ascent. Page
  characters count as `PageText` does (NFC; a newline between lines);
  a line's chars map to source through `Text::source_of` of each
  glyph's `drawn` start, per character for one-to-one segments.
  A non-one-to-one stretch is one run of its drawn length starting at
  its source start, capped at its source length; NFC changes are
  treated the same way. Prefix glyphs and added hyphens have `None`.
  The parser's language: `Front.lang` through `hypher::Lang::from_iso`
  (first two letters), else English.

- [ ] **Step 1: Write the failing tests.** In `runs.rs`:
  `consecutive_sources_make_one_run` (`[Some(5),Some(6),Some(7)]` → one
  run at offset 0, 5, 3 chars), `a_marker_gap_starts_a_new_run`
  (`[Some(5),Some(6),Some(9)]` → runs (0,5,2), (2,9,1)),
  `characters_without_source_are_left_out` (`[None,Some(3),None]` → one
  run (1,3,1)). In `tests/sample.rs` on `fixtures/sample.md`:
  ```rust
  #[test] fn page_text_and_runs_agree() {
      let source = std::fs::read_to_string(fixture("sample.md")).unwrap();
      let book = typeset(&source);
      let map = book.locations();
      for page in 0..book.page_count() {
          let text = PageText::new(book.glyphs(page));
          for (offset, run) in placed_runs(&map, page) {  // helper via map.location/page_char
              let on_page: String = text.text().chars().skip(offset).take(run.chars).collect();
              let from_source: String = drawn_for(&source, run); // via the tree's Text::drawn_of
              assert_eq!(on_page, from_source, "page {page}, run {run:?}");
          }
      }
  }
  #[test] fn page_text_and_runs_agree_with_crlf_and_bom() { /* same on "\u{feff}" + source with "\n" → "\r\n" */ }
  #[test] fn an_empty_file_is_one_blank_page() { assert_eq!(typeset("").page_count(), 1); }
  #[test] fn front_matter_alone_is_one_blank_page() { assert_eq!(typeset("---\ntitle: x\n---\n").page_count(), 1); }
  #[test] fn locations_come_in_source_order() { /* walking pages, run starts strictly increase */ }
  ```
  (If the sample's spans make `drawn_for` hard, compare against the
  concatenated drawn text of the blocks instead; the assertion is that
  page characters equal drawn characters for each run.)
  In `book.rs`: `an_added_hyphen_copies_away` (a narrow-measure book
  isn't available through `typeset`; instead build a page's glyphs from
  `set_at` lines and check `copy` of `PageText::with_marked_hyphens`
  gives the word whole).
- [ ] **Step 2: Run** `cargo test -p noido-typeset`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: a book of pages, its glyphs and locations`.

### Task 11: Links, named destinations, outline, properties

**Files:**
- Create: `crates/noido-typeset/src/links.rs`
- Modify: `src/book.rs`

**Interfaces:**
- Produces:
  ```rust
  pub(crate) struct Slugs;  // GitHub rules: lowercase, drop punctuation except '-' and '_', spaces → '-', repeats get -1, -2
  impl Slugs { pub fn new() -> Self; pub fn slug(&mut self, heading: &str) -> String; }
  impl Book {
      pub fn links(&self, page: usize) -> Vec<Link>;
      pub fn named_destination(&self, name: &str) -> Option<Destination>;
      pub fn outline(&self) -> Vec<OutlineEntry>;
      pub fn properties(&self) -> Properties; // format Markdown, title (front or first heading), author
  }
  ```
  Link areas: one `Rect` per line the link spans. `#name` →
  `LinkTarget::Internal` (heading slug or footnote label), footnote
  references → their footnote; anything else → `LinkTarget::Web(dest)`.
  Destinations: page and the top of the heading's (or footnote's) first
  line, `x: None`. Outline: headings nested by level, all `open: true`.

- [ ] **Step 1: Write the failing tests**:
  `slugs_follow_github` (`"Hello, World!"` → `"hello-world"`, again →
  `"hello-world-1"`), `an_internal_link_goes_to_its_heading`
  (`"# A\n\n[go](#b)\n\n# B\n"` → the link targets page 1, B's top),
  `a_web_link_is_kept_as_written`, `a_footnote_reference_links_to_its_note`,
  `a_link_over_two_lines_has_two_areas`,
  `the_outline_nests_headings_by_level`
  (`"# A\n## B\n# C\n"` → [A[B], C]),
  `the_title_comes_from_front_matter_or_the_first_heading`.
- [ ] **Step 2: Run** `cargo test -p noido-typeset links`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: links, destinations, outline and properties`.

### Task 12: Drawing

**Files:**
- Create: `crates/noido-typeset/src/draw.rs`
- Modify: `src/book.rs`

**Interfaces:**
- Produces: `impl Book { pub fn render_tile(&self, page: usize, scale: f32, rect: PixelRect) -> Result<Rgba8Image, EngineError>; }`
  White background; glyphs in text colour `#1b1b1b`; decorations as
  hairlines (max(0.5 pt × scale, 1 px)). Outlines via skrifa, cached per
  `(FaceId, glyph id)` as unit-size paths (a `Mutex<HashMap>` in the
  `Book` or a `thread_local!`), scaled at draw time. A tile larger than
  noido's limits returns `EngineError::TooLarge` as hayro does.

- [ ] **Step 1: Write the failing tests**:
  `a_page_has_ink_inside_the_text_block_only` (render page 0 of a
  one-paragraph book at scale 1: some non-white pixel inside
  `LEFT..LEFT+MEASURE × TOP..TOP+434`, none outside),
  `drawing_twice_gives_the_same_pixels`,
  `a_tile_is_the_matching_part_of_the_page` (tile at (100,100,50,50)
  equals that crop of the whole-page render at the same scale).
- [ ] **Step 2: Run** `cargo test -p noido-typeset draw`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `typeset: draw pages with cached outlines`.

### Task 13: The engine, and opening markdown in noido

**Files:**
- Create: `crates/noido-typeset/src/engine.rs`
- Modify: `crates/frontends/gui-egui/src/document.rs` (`AnyEngine`,
  test at ~241), `open.rs:38`, `crates/noido-macos/src/open_panel.rs:67`,
  `gui-egui/Cargo.toml`

**Interfaces:**
- Consumes: `Book` (Tasks 10–12).
- Produces:
  ```rust
  pub struct TypesetEngine { /* Mutex<Option<(PdfBytes or its hash, Arc<Book>)>> */ }
  impl TypesetEngine { pub fn new() -> Self; }
  impl PdfEngine for TypesetEngine { fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError>; }
  // TypesetDocument(Arc<Book>) implements every Document method through Book;
  // page_size = PAPER for every page; page_labels empty;
  // layout_key = Some(LAYOUT_KEY.into()).
  ```
  `open`: invalid UTF-8 → `EngineError::Open("not a readable document")`.
  The same bytes opened again (by content) return the cached `Arc<Book>`
  without laying out again. `AnyEngine` becomes a struct holding one
  `TypesetEngine` behind an `Arc`, shared by every `Arc::new(AnyEngine)`
  site in `document.rs` (build one `AnyEngine` and clone the `Arc`):
  after the PDF and ZIP checks, valid UTF-8 goes to the typeset engine.
  Dialogs: rfd filter `"PDF, comic book or markdown"` with
  `["pdf", "cbz", "md", "markdown", "txt"]`; macOS adds
  `UTType::typeWithFilenameExtension` for `md`, `markdown`, and the
  `public.plain-text` type.

- [ ] **Step 1: Write the failing tests**:
  - in `engine.rs`: `opening_the_same_bytes_twice_lays_out_once`
    (a counter in the engine, or `Arc::ptr_eq` on the books),
    `invalid_utf8_is_not_a_readable_document`,
    `a_document_answers_from_its_book` (page count, page size =
    `PAPER`, layout key).
  - in `document.rs`: `utf8_text_opens_as_typeset_pages`
    (`AnyEngine.open(b"# Hi\n\nThere.\n")` → 1 page,
    `properties().format == Format::Markdown`); change the existing
    not-a-document test to bytes `[0xff, 0xfe, 0x00]`.
  - in `app.rs` harness: `a_markdown_fixture_opens_and_searches`
    (open `fixtures/sample.md` through the real `AnyEngine`, search a
    word known to be in it, a hit is found) and
    `a_highlight_on_markdown_comes_back_on_the_same_characters` (make a
    highlight over a word, save, reopen, the highlight's locations and
    the selected text are the same).
- [ ] **Step 2: Run** `just test`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `just check`. Expected: PASS.
- [ ] **Step 5: Commit** `egui: open markdown and plain text as typeset pages`
  (the engine itself may be its own commit first:
  `typeset: an engine that lays a file out once for all its threads`).
- [ ] **Step 6: By hand.** `just build`; open a long markdown file
  from `../testfiles/` with `just run <file>`; time `typeset` on it
  with a throwaway scratchpad program (not committed); note what the
  user should check: pages look right, search, highlight, outline,
  links.

---

## Self-review notes

- Spec coverage: fonts repo (T1); locations offsets and layout key (T2,
  T3); hyphens (T4); crate and fonts (T5); Knuth–Plass (T6); paragraphs,
  fallbacks, front matter (T7); inline styles, hyphenation, code (T8);
  page breaking (T9); glyphs, NFC, runs, guarantee (T10); links,
  outline, properties (T11); drawing (T12); engine sharing, opening,
  dialogs, harness tests (T13). YAML and Alegreya are out (later).
- Table rows: the spec says "a paragraph per row, cells separated by
  spaces". A `Text` can't join cells without breaking the source map,
  so T7 sets each cell as its own short paragraph, rows in order. It's
  a fallback until Y3b; noted here as a deviation for the executor to
  ledger.
