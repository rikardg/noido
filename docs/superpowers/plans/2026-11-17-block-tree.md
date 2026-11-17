# Block tree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `noido-blocks` with the block tree (nested blocks,
flat text with style ranges and a drawn-to-source segment table) and a
markdown parser into it.

**Architecture:** `text.rs` holds `Text`, its segments and the two
lookups; `tree.rs` the block types; `markdown.rs` walks
pulldown-cmark's offset iterator, converts byte ranges to characters
and builds the tree. No other noido crate depends on it yet.

**Tech Stack:** Rust 2024, pulldown-cmark 0.13.6
(`default-features = false`).

**Spec:** `docs/superpowers/specs/2026-11-17-block-tree-design.md`

## Global Constraints

- Offsets in the tree are characters of the part (the markdown source
  as read, not normalised), as `usize`; only `Styled::drawn` and
  `Segment::drawn` are bytes of `Text::string`.
- A segment is one-to-one when its drawn character count equals its
  source character count (soft breaks and smart quotes included).
  Otherwise every drawn character of it maps to `source.start`.
- No drawn character maps onto a marker (the spec's "Markers" list).
- pulldown-cmark options: tables, footnotes, strikethrough, task lists,
  smart punctuation, YAML-style metadata blocks. Nothing else.
- Commit prefix `blocks` (`Plan` for this file). Titles ≤ 72
  characters, bodies wrapped at 72. jj: `jj commit -m`.
- TDD: each test is run and seen failing before the code it tests.
- `just check` (tests, clippy, fmt) clean at the end of every task.

## What pulldown-cmark reports (probed with 0.13.6)

So the implementer doesn't rediscover it:

| Source | Event and range |
|---|---|
| `\*b` | `Text("*b")`, range starts after the backslash (one-to-one; the backslash is a gap) |
| `&amp;` | `Text("&")` over all 5 bytes |
| `"q"`, `x's` | `Text("“")`, `Text("’")` over 1 byte each |
| `--` | `Text("–")` over 2 bytes |
| `  \n`, `\\\n` | `HardBreak` over the whole break |
| `\n` in a paragraph | `SoftBreak` over it; `> ` of a quote's next line is a gap |
| `` `co de` `` | `Code("co de")` over the backticks too |
| `[^n]` | `FootnoteReference("n")` over all of `[^n]` |
| `[x]` | `TaskListMarker(true)` over `[x]` |
| `![alt *e*](i.png)` | `Start(Image)` … alt as Text/Emphasis events … `End(Image)`, over the whole construct |
| `<br>` | `InlineHtml` (not drawn) |
| front matter | `Start(MetadataBlock)` over `---…---`, its text inside |
| `<div>…` block | `Start(HtmlBlock)`, `Html` |
| `[r]: http://r` | no events |
| table | cells' ranges include their padding spaces; the alignment row is a gap |

## Review Focus

1. Non-ASCII text (é, CJK, emoji) — byte ranges must become character
   offsets; a wrong conversion puts every later location off. Test in
   Task 2.
2. CRLF files — a soft break `\r\n` is drawn as one space and is not
   one-to-one; code block lines keep their `\r`. Test in Task 2.
3. Empty and whitespace-only documents, and unclosed constructs (an
   unclosed `*`, a fence running to the end) — an empty tree or a
   plain paragraph, never a panic. Test in Task 3.
4. Nested inlines (a link inside strong inside emphasis, `***x***`) —
   styles overlap by range, each with its own span. Test in Task 2.
5. Nested containers (a quote in a list item in a list, three deep) —
   spans nest and drawn text still maps. Test in Task 3.

---

### Task 1: The crate, and `Text` with its segments

**Files:**
- Create: `crates/noido-blocks/Cargo.toml`, `crates/noido-blocks/src/lib.rs`, `crates/noido-blocks/src/text.rs`
- Modify: `Cargo.toml` (workspace members and `[workspace.dependencies]`: `noido-blocks`, `pulldown-cmark = { version = "0.13.6", default-features = false }`)

**Interfaces:**
- Produces, in `noido_blocks` (re-exported from `lib.rs`):
  - `pub struct Text { pub string: String, pub styles: Vec<Styled>, pub segments: Vec<Segment> }` (`Default`, `Clone`, `Debug`, `PartialEq`)
  - `pub struct Segment { pub drawn: usize, pub source: Range<usize> }`
  - `pub struct Styled { pub style: Style, pub drawn: Range<usize>, pub span: Range<usize> }`
  - `pub enum Style { Emphasis, Strong, Strikethrough, Code, Link { dest: String }, FootnoteRef { label: String }, Image { src: String, alt: String }, TaskBox { checked: bool } }`
  - `impl Text { pub fn push(&mut self, drawn: &str, source: Range<usize>); pub fn source_of(&self, drawn: usize) -> usize; pub fn drawn_of(&self, source: usize) -> Option<usize> }`

`push` appends `drawn` to `string` and a segment for it; an empty
`drawn` is ignored. When both the new piece and the last segment are
one-to-one and contiguous in the source, the last segment grows
instead. A debug assertion refuses a source range starting before the
last segment's end.

`source_of(d)`: in the segment holding byte `d`, one-to-one gives
`source.start` + characters from the segment's start to `d`, otherwise
`source.start`; `d == string.len()` gives the last segment's
`source.end` (0 for an empty `Text`).

`drawn_of(s)`: inside a segment's source (`start <= s < end`):
one-to-one gives the byte of that character, otherwise the segment's
`drawn`. Else, if some segment's `source.end == s`, the drawn end of
that segment (so the end of a highlight just before a closing `*`
lands after the last letter). Else `None` (a marker).

- [ ] **Step 1: Write the failing tests** in `text.rs`:

```rust
#[test]
fn one_to_one_text_maps_each_character() {
    let mut t = Text::default();
    t.push("héllo", 10..15);
    assert_eq!(t.segments.len(), 1);
    assert_eq!(t.source_of(0), 10);
    assert_eq!(t.source_of(3), 12); // 'l', after the two-byte 'é'
    assert_eq!(t.source_of(t.string.len()), 15);
    assert_eq!(t.drawn_of(12), Some(3));
    assert_eq!(t.drawn_of(15), Some(t.string.len()));
}

#[test]
fn contiguous_pieces_are_one_segment() {
    let mut t = Text::default();
    t.push("ab", 0..2);
    t.push("cd", 2..4);
    assert_eq!(t.segments, vec![Segment { drawn: 0, source: 0..4 }]);
}

#[test]
fn a_gap_in_the_source_starts_a_segment_and_is_a_marker() {
    let mut t = Text::default();
    t.push("A ", 0..2);
    t.push("*b", 3..5); // `\*b`: the backslash at 2 is a gap
    assert_eq!(t.segments.len(), 2);
    assert_eq!(t.source_of(2), 3);
    assert_eq!(t.drawn_of(2), Some(2)); // end of "A "
    assert_eq!(t.drawn_of(3), Some(2));
}

#[test]
fn a_piece_of_another_length_maps_to_its_start() {
    let mut t = Text::default();
    t.push("x ", 0..2);
    t.push("&", 2..7); // `&amp;`
    t.push(" y", 7..9);
    assert_eq!(t.source_of(2), 2);
    assert_eq!(t.source_of(3), 7);
    assert_eq!(t.drawn_of(4), Some(2)); // inside the entity
    assert_eq!(t.drawn_of(7), Some(3));
}

#[test]
fn a_source_character_outside_every_segment_has_no_drawn_place() {
    let mut t = Text::default();
    t.push("a", 1..2);
    t.push("b", 4..5);
    assert_eq!(t.drawn_of(0), None);
    assert_eq!(t.drawn_of(3), None);
}
```

- [ ] **Step 2:** `cargo test -p noido-blocks` — Expected: fails to compile (`Text` not defined); after the types exist with `todo!()` bodies, the tests fail.
- [ ] **Step 3:** Implement the types and the three methods in `text.rs`.
- [ ] **Step 4:** `cargo test -p noido-blocks` — Expected: 5 passed. `just check` clean.
- [ ] **Step 5:** Commit: `blocks: text with style ranges and a map back to its source`.

### Task 2: Markdown paragraphs and headings, with inline styles

**Files:**
- Create: `crates/noido-blocks/src/tree.rs`, `crates/noido-blocks/src/markdown.rs`
- Modify: `crates/noido-blocks/src/lib.rs`, `crates/noido-blocks/Cargo.toml` (pulldown-cmark)

**Interfaces:**
- Consumes: Task 1's `Text`, `Styled`, `Style`, `push`.
- Produces:
  - `tree.rs`: `Tree`, `Block`, `BlockKind`, `Item`, `Cell`, `Align { None, Left, Center, Right }`, exactly as the spec's sketch (`Clone, Debug, PartialEq`). All variants are declared here; Task 3 fills the parsing of the containers.
  - `pub fn noido_blocks::markdown::parse(source: &str) -> Tree`.

Byte ranges become characters through an index built once over the
source (the implementer's choice of structure). Inline rules:

- `Text(s)` over range `r`: `push(s, r)`.
- `SoftBreak` → `push(" ", r)`; `HardBreak` → `push("\n", r)`.
- `Code(s)`: find `s` inside `r`'s source text; if found, push it
  over that sub-range and a `Code` style; if not (line endings folded),
  push it over the range inside the backticks.
- `FootnoteReference(l)`: push `l` over its position inside `[^l]`,
  styled `FootnoteRef { label: l }`.
- `InlineHtml`: nothing drawn.
- Emphasis, strong, strikethrough, link: a `Styled` whose `drawn` runs
  from the string's length at `Start` to its length at `End`, and whose
  `span` is the event range. Styles are pushed in the order they close.
- Image: draw `"\u{FFFC}"` over the whole construct with
  `Image { src, alt }`, `alt` being the text events inside, concatenated;
  nothing inside is drawn.

- [ ] **Step 1: Write the failing tests** in `markdown.rs` (a helper
  `fn para(src) -> Text` taking the first block's text is fine):

```rust
#[test]
fn a_heading_has_its_level_text_and_whole_span() {
    let tree = parse("## Hi there\n");
    let Block { kind: BlockKind::Heading { level, text }, span } = &tree.blocks[0] else { panic!() };
    assert_eq!((*level, span.clone()), (2, 0..12));
    assert_eq!(text.string, "Hi there");
    assert_eq!(text.source_of(0), 3);
}

#[test]
fn emphasis_is_a_style_over_the_drawn_text_with_its_markers_in_the_span() {
    let t = para("a *bc* d");
    assert_eq!(t.string, "a bc d");
    assert_eq!(t.styles, vec![Styled { style: Style::Emphasis, drawn: 2..4, span: 2..6 }]);
    assert_eq!(t.drawn_of(3), Some(2));
    assert_eq!(t.drawn_of(5), Some(4)); // the closing `*`: just after "bc"
}

#[test]
fn escapes_entities_and_smart_punctuation_map_back() {
    let t = para("\\*x &amp; \"q\" a--b");
    assert_eq!(t.string, "*x & “q” a–b");
    assert_eq!(t.source_of(0), 1); // `*`, after the backslash
    assert_eq!(t.source_of(t.string.find('&').unwrap()), 4);
    assert_eq!(t.drawn_of(1), Some(0));
}

#[test]
fn breaks_are_drawn_as_space_and_newline() {
    let t = para("a\nb  \nc");
    assert_eq!(t.string, "a b\nc");
}

#[test]
fn code_spans_footnote_refs_and_images_draw_their_inside() {
    let t = para("`co de` x[^n] ![alt *e*](i.png)");
    assert!(t.string.starts_with("co de x"));
    assert_eq!(t.source_of(0), 1);
    assert!(t.styles.iter().any(|s| s.style == Style::FootnoteRef { label: "n".into() }));
    let img = t.styles.iter().find(|s| matches!(s.style, Style::Image { .. })).unwrap();
    assert_eq!(img.style, Style::Image { src: "i.png".into(), alt: "alt e".into() });
    assert_eq!(&t.string[img.drawn.clone()], "\u{FFFC}");
    assert_eq!(t.source_of(img.drawn.start), img.span.start);
}

#[test]
fn inline_html_is_not_drawn() {
    assert_eq!(para("a<br>b").string, "ab");
}

#[test]
fn offsets_are_characters_not_bytes() {
    let t = para("é 😀 *ü*");
    let em = &t.styles[0];
    assert_eq!(em.span, 4..7);
    assert_eq!(t.source_of(em.drawn.start), 5);
}

#[test]
fn crlf_soft_breaks_are_one_space_over_two_characters() {
    let t = para("a\r\nb");
    assert_eq!(t.string, "a b");
    assert_eq!(t.source_of(2), 3);
}

#[test]
fn nested_styles_overlap_with_their_own_spans() {
    let t = para("***x*** *a **[l](u)***");
    let styles: Vec<_> = t.styles.iter().map(|s| (&s.style, s.drawn.clone())).collect();
    assert!(styles.contains(&(&Style::Emphasis, 0..1)));
    assert!(styles.contains(&(&Style::Strong, 0..1)));
    assert!(styles.contains(&(&Style::Link { dest: "u".into() }, 4..5)));
}
```

- [ ] **Step 2:** `cargo test -p noido-blocks` — Expected: the new tests fail (`parse` returns an empty tree from a `todo!()`-free stub, or doesn't compile until the stub exists).
- [ ] **Step 3:** Implement `tree.rs` and the paragraph/heading/inline part of `parse`. Blocks other than paragraphs and headings may be skipped for now (Task 3).
- [ ] **Step 4:** `cargo test -p noido-blocks` — Expected: all pass. `just check` clean.
- [ ] **Step 5:** Commit: `blocks: parse markdown paragraphs and headings into the tree`.

### Task 3: Markdown containers and the rest of the blocks

**Files:**
- Modify: `crates/noido-blocks/src/markdown.rs`

**Interfaces:**
- Consumes: Task 2's `parse`, `Tree`, `BlockKind`, `Item`, `Cell`, `Align`.
- Produces: `parse` covering every `BlockKind`.

Rules: a block's `span` is pulldown-cmark's range for it. In a tight
list pulldown-cmark sends an item's text with no `Paragraph` around
it; that text becomes a `Paragraph` block whose span runs from its
first to its last event. Lists:
`start` is `List(Some(n))`; an item's `TaskListMarker(c)` draws
`"\u{FFFC}"` over `[x]` with `TaskBox { checked: c }` at the start of
the item's first text, then the space after `[x]` (one-to-one), and
sets `Item::task`. Code blocks: `info` is
the fence's info string (`None` when empty or indented); text pushed
as given, final newline included. Tables: `align` from `Table(..)`,
`head` from `TableHead`'s cells, `rows` from `TableRow`s. Footnote
definitions stay in place as `Footnote { label, blocks }`.
`HtmlBlock` → `Html`, `MetadataBlock` → `FrontMatter`, with spans.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_task_list_item_has_its_box() {
    let tree = parse("- [x] done\n- two\n");
    let BlockKind::List { start: None, items } = &tree.blocks[0].kind else { panic!() };
    assert_eq!(items[0].task, Some(true));
    assert_eq!(items[1].task, None);
    let BlockKind::Paragraph(t) = &items[0].blocks[0].kind else { panic!() };
    assert_eq!(t.string, "\u{FFFC} done");
    assert_eq!(t.source_of(0), 2);
}

#[test]
fn an_ordered_list_keeps_its_start() {
    let BlockKind::List { start, .. } = &parse("3. a\n4. b\n").blocks[0].kind else { panic!() };
    assert_eq!(*start, Some(3));
}

#[test]
fn a_quote_holds_blocks_and_skips_its_markers() {
    let tree = parse("> one\n> two\n");
    let BlockKind::Quote(inner) = &tree.blocks[0].kind else { panic!() };
    let BlockKind::Paragraph(t) = &inner[0].kind else { panic!() };
    assert_eq!(t.string, "one two");
    assert_eq!(t.source_of(4), 8);
    assert_eq!(t.drawn_of(7), None); // the space after the second `>`
}

#[test]
fn a_fenced_code_block_keeps_its_info_and_text() {
    let tree = parse("```rust\nfn x() {}\n```\n");
    let BlockKind::Code { info, text } = &tree.blocks[0].kind else { panic!() };
    assert_eq!(info.as_deref(), Some("rust"));
    assert_eq!(text.string, "fn x() {}\n");
    assert_eq!(text.source_of(0), 8);
}

#[test]
fn a_table_has_alignment_head_and_rows() {
    let tree = parse("| a | b |\n|---|:-:|\n| c | d |\n");
    let BlockKind::Table { align, head, rows } = &tree.blocks[0].kind else { panic!() };
    assert_eq!(align, &vec![Align::None, Align::Center]);
    assert_eq!(head[1].text.string, "b");
    assert_eq!(rows[0][0].text.source_of(0), 22);
}

#[test]
fn rules_footnotes_html_and_front_matter_are_blocks() {
    let tree = parse("---\nt: x\n---\n\n***\n\n[^n]: Note.\n\n<div>h</div>\n");
    let kinds: Vec<_> = tree.blocks.iter().map(|b| std::mem::discriminant(&b.kind)).collect();
    assert_eq!(kinds.len(), 4);
    assert!(matches!(tree.blocks[0].kind, BlockKind::FrontMatter));
    assert_eq!(tree.blocks[0].span, 0..12);
    assert!(matches!(tree.blocks[1].kind, BlockKind::Rule));
    assert!(matches!(&tree.blocks[2].kind, BlockKind::Footnote { label, .. } if label == "n"));
    assert!(matches!(tree.blocks[3].kind, BlockKind::Html));
}

#[test]
fn empty_and_unclosed_input_parses() {
    assert!(parse("").blocks.is_empty());
    assert!(parse("  \n\n").blocks.is_empty());
    assert_eq!(para("a *b").string, "a *b");
    assert!(matches!(parse("```\ncode").blocks[0].kind, BlockKind::Code { .. }));
}

#[test]
fn containers_nest_three_deep() {
    let tree = parse("- a\n  - > q *e*\n");
    let BlockKind::List { items, .. } = &tree.blocks[0].kind else { panic!() };
    let BlockKind::List { items: inner, .. } = &items[0].blocks[1].kind else { panic!() };
    let BlockKind::Quote(q) = &inner[0].blocks[0].kind else { panic!() };
    let BlockKind::Paragraph(t) = &q[0].kind else { panic!() };
    assert_eq!(t.string, "q e");
    assert_eq!(t.source_of(2), 13);
}
```

- [ ] **Step 2:** `cargo test -p noido-blocks` — Expected: the new tests fail.
- [ ] **Step 3:** Implement the containers in `parse` (a stack of open containers is the natural shape).
- [ ] **Step 4:** `cargo test -p noido-blocks` — Expected: all pass. `just check` clean.
- [ ] **Step 5:** Commit: `blocks: parse markdown lists, quotes, code, tables and footnotes`.

### Task 4: The sample fixture and the whole-tree checks

**Files:**
- Modify: `fixtures/make_fixtures.py` (a `sample_md() -> str` and a line in `__main__` writing `sample.md`)
- Create: `fixtures/sample.md` (generated), `crates/noido-blocks/tests/sample.rs`

**Interfaces:**
- Consumes: `parse`, `Tree`, `Text::{source_of, segments}`.

`sample_md()` returns a short document using every construct in the
spec's Goals and Markers lists: front matter, ATX and setext headings,
emphasis, strong, strikethrough, a code span, a link with a title, an
image, an escape, an entity, smart quotes and a dash, soft and hard
breaks, a footnote reference and definition, a task list and an
ordered list, a nested quote, fenced and indented code, a table with
alignment, a rule, an HTML block, inline HTML, a link reference
definition, and non-ASCII letters.

The test walks every `Text` in the tree (headings, paragraphs, code,
cells, nested blocks) with a helper `fn texts(&Tree) -> Vec<&Text>`.

- [ ] **Step 1: Write the failing tests** in `tests/sample.rs`, reading
  `../../fixtures/sample.md` via `env!("CARGO_MANIFEST_DIR")`:
  - `every_drawn_character_maps_into_the_source`: for each text, the
    segments' `drawn` strictly increase and start at 0, sources don't
    overlap and increase, and `source_of` of every char boundary is
    within `0..=source.chars().count()`.
  - `one_to_one_segments_draw_their_source`: in each one-to-one
    segment, each drawn character equals its source character, except
    a drawn `' '` for a source `'\n'` and the curly quotes `“ ” ‘ ’`
    for `"` and `'`.
  - `exactly_the_text_is_reached`: the set of source characters reached
    by `source_of` over all drawn characters, plus every character inside
    non-one-to-one segments' `source`, equals an expected set written in
    the test by hand as source ranges (the non-marker text of
    `sample.md`, per the spec's Markers list). Write the ranges as
    `(line, column range)` pairs or with `str::find` on literal
    snippets, whichever reads more clearly; never derive them from the
    parser.
  - `spans_cover_their_markers`: each heading's span starts at its `#`,
    each style's `span` starts and ends at a delimiter (`*`, `~`, `` ` ``,
    `[`, `!`), and each block's span covers all of its texts' sources.
- [ ] **Step 2:** `cargo test -p noido-blocks --test sample` — Expected:
  fails first for the missing fixture; after `python3 fixtures/make_fixtures.py`,
  any failure is a parser bug to fix test-first in `markdown.rs` (each
  such fix gets its own unit test there, watched failing).
- [ ] **Step 3:** Fix what the checks find.
- [ ] **Step 4:** `just check` — Expected: all tests pass, clippy and fmt clean.
- [ ] **Step 5:** Commit: `blocks: a sample markdown fixture, checked against its markers`.
