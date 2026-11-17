# The block tree: one shape for EPUB and markdown, with source spans

Date: 2026-11-17 · Status: agreed 2026-11-17 · TODO Y2 (the session
list; no issue yet)

## Intent

EPUB and markdown are to be typeset by one typesetter (Y3) and shown
through `Document` like a PDF (see
`docs/investigations/2026-11-15-typesetting.md`, "The plan"). Both
formats are first parsed into one block tree, and the typesetter reads
only that tree. Every piece of text in the tree knows where it came
from in its part, so each glyph the typesetter places can carry a
`Location`, and search, selection, highlights and the reading place
work the same as on a PDF (see
`docs/superpowers/specs/2026-11-15-unified-locations-design.md`).

Y2 is the tree and its first producer, markdown. EPUB's XHTML is the
second producer, in E1.

## Goals

- One tree type for both formats: headings, paragraphs, lists (ordered,
  unordered, task lists), block quotes, code blocks, tables, thematic
  breaks, images, footnotes, and inline emphasis, strong, strikethrough,
  code spans and links.
- Source spans, in the terms of the locations spec: characters within
  one part. For markdown that is the source file as read (the one part);
  for EPUB, later, a chapter's text content.
- Text that the typesetter can hand straight to parley: per paragraph,
  one string with style ranges over it.
- A drawn character's source character found by one lookup, wherever
  drawn text and source differ in length.
- Raw markdown mode (M2) served by the same tree: the span of each
  whole construct, markers included, so it can draw the source with the
  markers kept.
- Markdown parsed into the tree.

## Non-goals

- Typesetting, themes, fonts, pages (Y3, E2, M1, M2).
- EPUB: container, spine, XHTML (E1).
- A `Document` for markdown, or opening `.md` files in the app (M1).
- Math, raw HTML rendering, definition lists, heading attributes.

## Anti-goals

- A drawn character with no way back to its source. Every character of
  the drawn text maps to a source character, or the location map would
  have holes where a highlight or the reading place could fall.
- A location inside a marker in typeset mode (`##`, `*`, `>`, the
  backslash of an escape). Markers are never drawn, so no drawn
  character may map onto one.
- Format-specific knowledge in the typesetter. It reads the tree, never
  markdown or XHTML.

## Approaches

Three shapes were weighed (2026-11-17):

- **A full syntax tree:** blocks and inlines both nest (a link holding
  emphasis holding text), each node with its span. Closest to the
  source, but the typesetter would flatten every paragraph again before
  shaping, since parley takes one string per paragraph with styles
  laid over ranges.
- **Events, like pulldown-cmark's:** a flat list of start and end tags
  and text, each with a span. Cheapest to produce, but every reader
  (the typesetter, the raw theme, search) rebuilds the structure.
- **Nested blocks, flat paragraphs (chosen):** blocks nest; a block
  holding text keeps it already flat, as one string, style ranges, and
  a short segment table from drawn offsets to source characters. The
  flattening happens once, in the parser, in the shape the typesetter
  and the location map both need.

## Design

### Crate

A new crate, `crates/noido-blocks` (commit prefix `blocks`): the tree,
and a `markdown` module that parses into it with `pulldown-cmark`.
EPUB's XHTML parser joins it in E1. It depends on nothing in noido;
offsets are plain `usize` character counts within a part, and whoever
builds a `Location` adds the part number.

### The tree

A sketch; the builder settles names and the exact derive set.

```rust
/// One part's blocks: for markdown, the whole file.
pub struct Tree {
    pub blocks: Vec<Block>,
}

pub struct Block {
    pub kind: BlockKind,
    /// The whole construct in the source, markers included
    /// (`## Title\n`, `> quoted\n> lines`), in characters of the part.
    pub span: Range<usize>,
}

pub enum BlockKind {
    Heading { level: u8, text: Text },
    Paragraph(Text),
    /// Its text as written: no styles, whitespace kept.
    Code { info: Option<String>, text: Text },
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Item> },
    Table { align: Vec<Align>, head: Vec<Cell>, rows: Vec<Vec<Cell>> },
    Rule,
    /// A footnote's body, where it was defined.
    Footnote { label: String, blocks: Vec<Block> },
    /// Raw HTML: not drawn in typeset mode; raw mode styles its span.
    Html,
    /// YAML front matter: not drawn in typeset mode; raw mode styles it.
    FrontMatter,
}

pub struct Item {
    pub span: Range<usize>,
    /// `Some` for a task list item: checked or not.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

pub struct Cell { pub span: Range<usize>, pub text: Text }
```

### Text

```rust
pub struct Text {
    /// What is drawn, as parley will shape it.
    pub string: String,
    pub styles: Vec<Styled>,
    /// From drawn offsets to source characters; see below.
    pub segments: Vec<Segment>,
}

pub struct Styled {
    pub style: Style,
    /// Bytes of `string`, as parley's ranges are.
    pub drawn: Range<usize>,
    /// The whole construct in the source, markers included (`*so*`),
    /// for raw mode.
    pub span: Range<usize>,
}

pub enum Style {
    Emphasis,
    Strong,
    Strikethrough,
    Code,
    Link { dest: String },
    /// A reference to a footnote, drawn as its label.
    FootnoteRef { label: String },
    /// An image, drawn as one object character (U+FFFC). Its segment
    /// is not one-to-one and covers the whole `![alt](src)`, so a
    /// highlight over the image takes all of it. Whether extracted text
    /// (search, copying, quotes) reads the alt text in its place is
    /// Y3's to decide, with the typesetter's text extraction.
    Image { src: String, alt: String },
    /// A task list item's box, drawn as one object character (U+FFFC);
    /// its segment covers `[ ]` or `[x]`.
    TaskBox { checked: bool },
}
```

Styles nest by range overlap, as parley's ranged styles do: `***both***`
gives an `Emphasis` and a `Strong` over the same drawn range.

### Segments: from drawn text to the source

The drawn string is cut into segments, in order, covering it without
gaps:

```rust
pub struct Segment {
    /// Where it starts in `string`, in bytes.
    pub drawn: usize,
    /// Where its source starts and ends, in characters of the part.
    pub source: Range<usize>,
}
```

A segment is **one-to-one** when its drawn characters and its source
characters are the same count; then drawn character *i* of the segment
is source character `source.start + i`. Otherwise (an escape `\*`
drawn as `*`, an entity `&amp;` drawn as `&`, a hard break, smart
punctuation `--` drawn as `–`, and in EPUB collapsed whitespace) every
drawn character of the segment maps to `source.start`, and the
segment's end maps to `source.end`. A highlight over such a character
therefore takes the whole of its source, which is what copying from
the source would give.

`Text` answers `source_of(drawn: usize) -> usize` (drawn byte to source
character, the end of the string included) and
`drawn_of(source: usize) -> Option<usize>` (the other way, `None` for a
source character that is only a marker). Consecutive one-to-one runs
that are contiguous in the source are one segment, up to 256
characters, so a lookup is a binary search over few segments and then
a count of at most 256 characters, however long the paragraph or code
block.

Line endings inside a paragraph (soft breaks) are drawn as one space;
`\n` and `\r\n` are both source characters, so `\n` is one-to-one and
`\r\n` is not. One-to-one means the same count, not the same
characters: a soft break (`\n` drawn as a space) and a smart quote
(`"` drawn as `“`) are one-to-one.

### Markers

A **marker** is source that is syntax rather than text the reader
reads. Every source character is either reached from drawn text or is
a marker, and markers are exactly:

- block markers: `#` and the space after it, setext underlines, `>`,
  list bullets and numbers, code fences and info strings, the
  indentation of indented code, table pipes and the alignment row,
  thematic breaks;
- inline markers: emphasis and strikethrough delimiters, code span
  backticks, a link's brackets, destination and title, a footnote
  reference's `[^` and `]` (its label is drawn), the backslash of an
  escape;
- whole constructs not drawn in typeset mode: raw HTML, front matter,
  link reference definitions, footnote definitions' `[^label]:`;
- blank lines between blocks.

An image's `![alt](src)` and a task box's `[x]` are not markers: each
is drawn as one object character whose segment covers it.

Blocks' spans tile the source except blank lines and link reference
definitions, so raw mode (M2) can style every construct from the tree.
Inline raw HTML is a gap between segments: raw mode draws it from the
source between them.

### Markdown

Parsed with `pulldown-cmark`'s offset iterator, with these extensions:
tables, footnotes, strikethrough, task lists, smart punctuation, and
YAML metadata blocks (front matter is skipped, not drawn). pulldown-cmark
reports byte ranges; the parser converts them to characters once, from
an index built over the source.

- Raw HTML, blocks and inline, is not drawn in typeset mode; raw mode
  shows it as source anyway.
- A code block's text is its content lines as written, so it is
  one-to-one (indentation removed by the container is a gap between
  segments, not drawn).
- Characters counted are those of the file as read (decoded as UTF-8),
  not normalised, as the locations spec says for markdown.

### What it is checked against

- Every drawn character maps into the source, and the segments are in
  order without overlap (a debug check where the tree is built, and a
  test over a mixed document).
- For one-to-one segments, the drawn character equals the source
  character, except for soft breaks and smart punctuation.
- No drawn character maps onto a marker: in a fixture using every
  construct, the source characters reached from drawn text equal a set
  written out by hand in the test from the list under Markers, not one
  derived from the parser.
- `span` of each block and style covers its markers.

A fixture `fixtures/sample.md`, generated by `make_fixtures.py` like the
others, uses every construct listed under Goals.

## Left to the builder

- Names, derives, and whether `Text` keeps a precomputed index.
- How the byte-to-character index is built.
- Test names and how the fixture is laid out.
- Whether `Footnote` stays in place or is gathered at the end; the
  typesetter decides where footnotes are drawn, so either works if the
  label links them.
