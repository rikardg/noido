# EPUB as typeset pages (E1)

Date: 2026-11-19

Status: design, agreed in conversation. Builds on the block tree (Y2,
`2026-11-17-block-tree-design.md`), the typesetter (Y3,
`2026-11-17-typesetter-design.md`) and the unified locations (L1–L4,
`2026-11-15-unified-locations-design.md`).

## Intent

noido reads EPUB books as well as PDF, comics and markdown. An `.epub`
opens as typeset pages in the house style, through the same `Document`
trait, so page modes, spreads, search, selection, highlights, links and
the outline work as they do for markdown.

E1 reads the book: the container, the package (metadata and spine), the
table of contents, and each chapter's XHTML into the block tree. The
typesetter learns to set a book of many parts instead of one markdown
file.

Books looked at by hand while designing this (in `../testfiles/`, named
here by kind): a Swedish novel (EPUB 2, 137 chapter files, real heading
tags, almost no images), a figure-heavy non-fiction book (EPUB 2, 86
figures, 1100 endnote references linking into separate notes files, a
4600-entry index, headings as classed `<p>`s), a short-story collection
(EPUB 3, `epub:type` semantics, print page-break markers, scene breaks
as `<hr>`), and three novels with chapter-title images, maps and family
trees.

## Goals

- An `.epub` opens in noido as typeset pages.
- The text of every spine item is set, in spine order, each item
  starting on a new page.
- The outline is the book's table of contents (EPUB 3 nav, else EPUB 2
  NCX), nested as the book nests it.
- Links work: within a chapter, between chapters (endnotes, the index,
  the book's own contents page), and to the web.
- Locations follow the locations spec: a part is a spine item, its
  characters are its text content in document order, NFC. Highlights,
  the reading position and search work as for markdown.
- The book's title, author and language are its properties;
  hyphenation follows its language.
- Markdown keeps working unchanged: it is a book of one part.

## Non-goals (E1)

- **Images (later, raised in priority after E1):** an image is drawn as
  a stand-in (below). Covers, chapter-title images, maps and figures
  wait.
- **Publisher styling (E2, later):** the book's CSS is not read at all.
  E2 will keep a small subset (alignment, unindented openers, indented
  blocks, italic/bold/small caps set by class, `display: none`, forced
  page breaks) and nothing else.
- **Settings for paper, size, font, margins (E3).**
- **Laying out off the opening thread, lean memory (#120).** E1 makes it
  possible (parts are set one at a time) but does not do it.
- **Page labels from print page-break markers** (`epub:type="pagebreak"`):
  a later addition; E1 ignores the markers.
- **Footnotes as such:** EPUB 3 `noteref`/`footnote` are plain links and
  plain blocks where they stand. Nothing moves to the page foot.
- **Fixed-layout EPUBs** (`rendition:layout` `pre-paginated`): opened as
  reflowable text like any other; no special handling.
- **Registering noido as the desktop's EPUB app** (the desktop entry,
  Info.plist): waits, as for markdown. The file dialogs offer `epub`.

## Anti-goals

- A broken or odd chapter must not keep the book from opening: one
  unreadable spine item becomes a one-line notice in its place.
- No guessing headings from class names: a classed `<p>` is a
  paragraph. The outline carries the chapter titles.
- No network access, ever: remote resources in a book are ignored.

## Approaches

1. **One flow.** All chapters in a single block list with part
   boundaries; each block remembers its part, a page break at each
   boundary. Smallest change to the typesetter, but the book stays one
   indivisible layout.
2. **Part by part (chosen).** Each spine item is set on its own into its
   own pages; the book joins them. Each part's locations come from its
   own layout, markdown is a book of one part, and it sets up #120: parts
   can later be set one at a time off the opening thread, the first
   chapter shown while the rest are set.

The user chose 2: about the same cost now, and it pays off with #120.

## Decided in conversation

- Images and publisher CSS wait; images come back sooner than E2.
- Each chapter file starts on a new page.
- Parts in spine order; `linear="no"` ignored (my call, after the
  builder check).
- A cover with no text (an image only) shows the book's title instead
  of a blank page.
- The 8 MB limit counts the book's text, not the zip.
- Hyphenation in the book's `dc:language`.
- Part-by-part layout (approach 2).

## The design

### Reading the container: a new crate `noido-epub`

Pure apart from reading a ZIP held in memory: bytes in, a book's parts
out. Depends on `zip`, `roxmltree`, `percent-encoding` (all already in
the workspace) and `noido-blocks`.

```rust
pub struct Epub {
    pub meta: Meta,            // title, creator(s), language, publisher
    pub parts: Vec<Part>,      // the spine, in reading order
    pub toc: Vec<TocEntry>,    // nested
}
pub struct Part {
    pub path: String,          // its path in the container, for links
    pub tree: noido_blocks::Tree,
    pub ids: Vec<(String, usize)>, // element id → character of the part
    pub chars: usize,          // the part's length in characters
    pub kind: PartKind,        // Text, Cover, Unreadable(String)
}
pub struct TocEntry { pub title: String, pub href: String, pub children: Vec<TocEntry> }
pub fn open(bytes: &[u8]) -> Result<Epub, EpubError>;
```

- `META-INF/container.xml` names the package (the first rootfile with
  media type `application/oebps-package+xml`).
- The package gives the metadata, the manifest and the spine. A part's
  number is its place in the spine, as the locations spec has it, and
  parts are set in spine order. `linear="no"` is ignored (my call): moving
  those items would make part numbers differ from spine positions, which
  saved highlights rely on, and would send a non-linear cover to the end.
  Spine items
  that are not XHTML (an image straight in the spine) become a part of
  kind `Cover` if first, else a part holding one image stand-in.
- The table of contents: the EPUB 3 nav document's `epub:type="toc"`
  `<nav>`, else the NCX named by the spine's `toc` attribute, else none
  (the outline is then empty). Hrefs are resolved against the document
  they appear in, percent-decoded, and kept as `path#fragment`.
- **DRM:** a `META-INF/encryption.xml` that encrypts any spine item
  refuses the book: "this EPUB is DRM-protected". Font obfuscation
  alone (only fonts listed) is fine, since fonts aren't read.
- **Limits:** the total uncompressed size of the spine's XHTML is held
  to the typesetter's 8 MB (`MAX_SOURCE`), checked from the ZIP's
  entry sizes before inflating, and each entry is read with that cap so
  a lying header can't inflate past it. Images and fonts aren't read.
- **Errors** (as `EngineError::Open`): not a ZIP, no container, no
  package, an empty spine, DRM, too large. Each a plain sentence like
  the existing ones.

### XHTML into the block tree: `noido_blocks::xhtml`

The block tree spec reserved this place: the second producer, beside
`markdown`. Pure: a document's text in, `(Tree, ids, chars)` out.

**Parsing.** `roxmltree`. HTML's named entities (`&nbsp;`, `&mdash;`,
…) are not XML; before parsing, those outside XML's five are replaced
by their numeric references (the Swedish novel has `&nbsp;` in four
files). A file that still fails to parse becomes a part of kind
`Unreadable`, set as one paragraph: "This chapter could not be read."

**The part's characters** (what locations count): the text of every
text node under `<body>`, in document order, after entity decoding and
NFC, whitespace included, except inside `<script>` and `<style>`.
`chars` is its length. Spans in the tree are offsets into it, so a
location is stable whatever the typesetter draws.

**Whitespace** collapses as CSS's `normal` does: runs of space, tab and
newline become one space, dropped at the start and end of a block and
after a hard break; kept as written inside `<pre>`. A dropped character
is simply not drawn; the segments map each drawn character to its
source, as for markdown.

**Blocks.**

| XHTML | Block |
|---|---|
| `h1`–`h6` | `Heading` of that level |
| `p`; a `div`, `li` or other container holding only inline content | `Paragraph` |
| `blockquote` | `Quote` |
| `ul`, `ol` (`start` read) with `li` | `List` |
| `pre` | `Code` |
| `table` (`thead`/`tbody`/`tfoot`, `tr`, `th`, `td`) | `Table` (first `tr` with `th`s is the head) |
| `hr` | `Rule` |
| `dl` | each `dt` a paragraph, each `dd` a paragraph |
| `figure`, `figcaption`, `section`, `article`, `aside`, `header`, `footer`, `nav`, `main`, `div`, `body` | their children, flattened |
| `head`, `script`, `style`, `svg`, `math` | nothing |

A container with inline content and blocks mixed (text directly in a
`div` beside `<p>`s) makes the loose inline runs paragraphs of their
own. Unknown elements: inline if they are inside a paragraph, else
their children are read as blocks.

**Inline styles.**

| XHTML | Style |
|---|---|
| `i`, `em`, `cite`, `dfn`, `var` | `Emphasis` |
| `b`, `strong` | `Strong` |
| `s`, `strike`, `del` | `Strikethrough` |
| `code`, `kbd`, `samp`, `tt` | `Code` |
| `a href` | `Link { dest }`, dest as written |
| `sup` | `Superscript` (new) |
| `sub` | `Subscript` (new) |
| `br` | a hard break, drawn `\n` |
| `img` | `Image { src, alt }` |
| `span`, `small`, `abbr`, `u`, `q`, `mark`, … | no style, text kept |

`Superscript` and `Subscript` are new `Style`s in `noido-blocks`. The
typesetter draws them as it draws footnote numbers (smaller, raised),
subscripts lowered by the same amount. Markdown never produces them.

**Ids.** Every element with an `id` (or a legacy `<a name>`) records
the part character where its text begins, for links and the outline.

**The image stand-in.** An image is drawn as its alt text in italic
(Y3's fallback), unless the alt text is empty or is just the file's
name (alt equal to the `src` file stem, ignoring case, as in
`alt="f0159-01"`); then it is drawn as `[image]`, in italic.

### The typesetter: a book of parts

`typeset(source)` for markdown becomes a thin wrapper: one part.

```rust
pub struct PartIn<'a> { tree: &'a Tree, ids: &'a [(String, usize)], path: &'a str, kind: PartKind }
pub fn typeset_book(parts: &[PartIn], meta: &BookMeta) -> Book;
```

- Each part is set alone: its paragraphs, lines and pages, as Y3 sets a
  markdown file now, its runs carrying its part number (today they are
  always part 0). A part starts on a new page; its pages follow the
  previous part's.
- Within a part, Y3's rules hold (a level-1 heading starts a page,
  unless the page is still empty).
- **A cover part** (the first spine item, when it has no characters and
  holds an image; a spine item marked as the cover by the package's
  `cover-image` / `<meta name="cover">` or `epub:type="cover"` counts only
  when it too has no characters, so no text is ever left undrawn) is set as
  a title page: the title as a level-1 heading, the author below as a
  paragraph. These glyphs have no source, so no location falls on them;
  the page has one, at the part's start: the map gets a zero-length run
  there (an unreadable part's notice likewise), so `page_start` answers.
- **Links.** A dest `path#id` (relative hrefs resolved against the
  part's own path and percent-decoded) goes to the part with that path,
  at the character its id records; a bare `path` to the part's start; a
  bare `#id` within the part. The destination is the page and line that
  character is set on (through the locations map). A dest with a scheme
  (`http:`, `https:`, `mailto:`) is a web link as now. Anything that
  doesn't resolve keeps its look and isn't clickable, as for markdown.
- **The outline** comes from the table of contents, each entry resolved
  as a link; an entry that doesn't resolve stays, without a target.
  Markdown's outline (from headings) is unchanged.
- **Properties:** `Format::Epub` (new), title, author (creators joined
  with ", "), subject from `dc:subject`, creator left empty.
- **Language:** `dc:language`'s first two letters, as markdown's front
  matter (English when unknown).
- **Memory and size:** the 8 MB cap counts the spine's XHTML (the check
  in `TypesetEngine::open` moves behind the EPUB test, so a zip is never
  measured as text); nothing
  else changes (#120 stays open).

### Opening an EPUB

- `AnyEngine` in `gui-egui/src/document.rs`: bytes starting `PK\x03\x04`
  go to the typesetter when the ZIP holds `META-INF/container.xml` or
  its first entry is `mimetype` reading `application/epub+zip`;
  otherwise to the comic engine as now.
- `TypesetEngine::open` accepts both: an EPUB by the same test, else
  UTF-8 text. The fingerprint and the shared `Arc<Book>` work as now.
- The file dialogs (`gui-egui/src/open.rs`, `noido-macos/src/open_panel.rs`)
  offer `epub`.
- `layout_key` stays the house style's: an EPUB's saved reading position
  and highlights are in locations, valid across layouts.

### `Document` and locations

Nothing in `Document` changes. `locations()` now has runs in many parts;
`LocationMap` already supports that. A quote's search across parts is
already part of re-anchoring (L4).

## Testing

- **Fixtures:** small EPUBs generated by `make_fixtures.py` into
  `fixtures/`, written by hand-built XML so the tests stand alone:
  - `book.epub` (EPUB 3): a cover image item, two chapters with
    headings, emphasis, a list, a table, a `<pre>`, `sup`/`sub`, a
    cross-chapter link to an id, an `<hr>`, an image with a file-name alt,
    a nav document with nesting, `dc:language` `sv`.
  - `book2.epub` (EPUB 2): an NCX, `&nbsp;` in a chapter, a `linear="no"`
    item (set in place), a spine item that isn't well-formed.
  - `drm.epub`: an `encryption.xml` covering a chapter.
- `noido-blocks::xhtml`: each table row above; whitespace collapsing and
  `<pre>`; ids; the character count (whitespace, entities, NFC) equals
  the drawn text's source map; segments stay in order.
- `noido-epub`: container, spine order (`linear="no"` in place), nav vs NCX,
  href resolution (relative, percent-encoded, fragment-only), DRM,
  size limit, missing pieces.
- `noido-typeset`: each part starts a page; runs carry the right part;
  a cross-part link and a TOC entry land on the right page and line;
  the cover page; markdown unchanged (its existing tests pass as they
  are).
- `gui-egui`: `book.epub` opens through `AnyEngine`, a CBZ still opens
  as a comic; the harness turns pages and follows a cross-chapter link.
- By hand: the books in `../testfiles/` open, read, search and link.

## Left to the builder

- The crate's internal split and names beyond the interfaces above.
- How `typeset_book` stitches pages and maps (as long as a part's
  layout doesn't depend on the parts before it, for #120).
- Whether markdown's slug and footnote-label lookups also apply to
  EPUB link fragments (ids are exact; slugs are not needed).
- The entity table (HTML's full list or the common few hundred).
- How `AnyEngine` peeks into the ZIP cheaply.
