# The typesetter behind `Document` (Y3)

Date: 2026-11-17

Status: design, agreed in conversation. Builds on the block tree (Y2,
`2026-11-17-block-tree-design.md`), the unified locations (L1–L4,
`2026-11-15-unified-locations-design.md`) and the typesetting
investigation (`docs/investigations/2026-11-15-typesetting.md`), whose
prototype this turns into a crate.

## Intent

noido reads EPUB and markdown as well as PDF, and they should look like
a well-set book, close to TeX: justified text, good line breaks,
hyphenation, careful page breaks. They go through the same `Document`
trait as PDFs, so page modes, spreads, search, selection, highlights,
links and the outline work the same whatever the format.

Y3 is the typesetter: it takes a block tree and lays it out on virtual
paper, then answers `Document`'s questions about those pages. Its first
caller is markdown. A `.md` file opens through it, which makes it
something to try by hand before EPUB (E1) exists.

## Goals

- A markdown file opens in noido as typeset pages.
- Running text is set well: Knuth–Plass line breaking with hyphenation
  in the document's language, no single line of a paragraph left at the
  foot or head of a page, a heading never last on its page.
- Every drawn glyph knows where in the source it came from, so search,
  selection, copying and highlights work, and the locations map
  (`LocationMap`) is right.
- Headings make the outline; links work, internal (`#heading`) and web.
- The fonts are embedded in the binary; nothing is downloaded and the
  system's fonts are not used.

## Non-goals (Y3)

- **Other blocks set properly (Y3b):** lists with hanging markers,
  block quotes indented, code blocks in a frame, tables, rules,
  footnotes at the page foot, images. Until Y3b they get the simple
  fallback below, so every markdown file opens and reads.
- **Settings for paper, text size, font and margins (E3).** Y3 has one
  fixed paper; nothing lays the document out again while it's open.
- **Re-anchoring highlights after the file changes (M3, M4).**
  Highlights made on markdown save and draw through the locations map.
  When the file changes on disk, they stay at their saved locations, as
  today for text that reflows.
- **Raw markdown mode (M2)**, EPUB (E1, E2), micro-typography (hanging
  punctuation, font expansion), maths.

## Anti-goals

- Glyphs that can't be found again: a highlight or search hit that
  lands on the wrong characters is worse than none. The locations map
  and the page text must agree exactly (see Locations).
- Laying the document out once per thread. A window opens its document
  on several threads (render, thumbnails, text, links, properties);
  the layout is computed once and shared.
- A slow open for PDF users: the fonts are unpacked on the first
  typeset document, not at start-up.

## Approaches

Considered, and the one picked (by the user, 2026-11-17):

- **A. Lay out the whole document when it opens. Picked.** Page count,
  outline targets and the locations map are known at once, and nothing
  outside the typesetter changes shape. Cost: a wait on open for a very
  long book, measured at roughly 0.2 s for 300 pages in the prototype.
  The same wait will come with every paper change in E3.
- **B. Lay out a chapter at a time, as it's reached.** Instant open,
  but page count, page labels and the slider would need a "not known
  yet" state everywhere.
- **C. parley's own line breaking first, Knuth–Plass later.** Pages
  sooner, but greedy breaking leaves gaps in a third to half of the
  lines, and every location test would be redone when it changes.

The typesetting engine itself (parley + our own Knuth–Plass, not typst)
was decided in Y1.

## Decided in conversation

- **Paper:** B-format, 129 × 198 mm (365.7 × 561.3 pt), the European
  trade paperback. Text 11 pt on 14 pt lines.
- **Fonts**, embedded deflate-compressed and unpacked once per process
  on the first typeset document:

  | Role | Font | Styles | Raw | Deflated |
  |---|---|---|---|---|
  | Body (house face) | Libron | regular, italic, bold, bold italic | 1.02 MB | 0.52 MB |
  | Fallback for scripts Libron lacks (Greek, Cyrillic, …) | Libertinus Serif | the same four | 1.22 MB | 0.93 MB |
  | Last fallback, "supports everything" | New Computer Modern 10 Book | Book, Book Italic, Bold, Bold Italic | 2.63 MB | 1.67 MB |
  | Headings | Libertinus Sans | regular, italic, bold | 0.86 MB | 0.65 MB |
  | Code | Monaspace Neon ("for now") | regular, italic, bold, bold italic | 1.47 MB | 1.01 MB |

  About 7.2 MB raw, 4.8 MB in the binary. Licences: SIL OFL for Libron,
  Libertinus and Monaspace; GUST Font License for New CM Book. The
  licence texts ship beside the fonts in the repo. No Type 1 stand-ins.
  Maths faces wait until something draws maths.

  Where the files come from (the versions measured above):

  | Font | Source | Version | Files |
  |---|---|---|---|
  | Libron | github.com/nicoverbruggen/libron, release | 0.32 | `Libron-{Regular,Italic,Bold,BoldItalic}.ttf` |
  | Libertinus | github.com/alerque/libertinus, release `Libertinus-7.051.zip` | 7.051 | `static/OTF/LibertinusSerif-{Regular,Italic,Bold,BoldItalic}.otf`, `LibertinusSans-{Regular,Italic,Bold}.otf` |
  | New Computer Modern | CTAN `newcomputermodern` | 8.1.1 | `otf/NewCM10-{Book,BookItalic,Bold,BoldItalic}.otf` |
  | Monaspace | github.com/githubnext/monaspace, `monaspace-static-v1.400.zip` | 1.400 | `Monaspace Neon/MonaspaceNeon-{Regular,Italic,Bold,BoldItalic}.otf` |

  **The fonts live in their own repo, `noido-fonts`** (decided
  2026-11-17), not in noido's:

  - It's a crate. noido depends on it through Cargo's git dependency,
    pinned to a tag, so it builds offline after the first fetch, and
    noido's history never holds font files.
  - The repo can collect more fonts than any one build uses: more
    families, more weights, new upstream versions. Adding or updating
    fonts is a new version of the crate, tagged by semver.
  - **Cargo features choose what goes into the binary,** one per family
    (`libron`, `libertinus-serif`, `libertinus-sans`, `newcm`,
    `monaspace-neon`, …). noido turns on the five above; a font not
    turned on costs nothing in the binary.
  - Each family keeps its upstream originals, its licence text, and a
    note of the release it came from. A build script deflates the
    chosen files, and the crate hands out the compressed bytes with
    each face's family, style and weight, plus the licence texts.
    noido-typeset unpacks them once per process.
  - Font files are plain git objects there, not LFS. Cargo's git
    dependencies don't fetch LFS objects, so LFS would leave the build
    with pointer files.
  - It stays a git dependency rather than a crates.io crate: crates.io
    caps a crate at 10 MB, which a growing collection outgrows.
- **Hyphenation** in the document's language, from the front matter's
  `lang` (or `language`), English when it's missing or unknown to
  `hypher`.
- **Split:** Y3 is running text; Y3b the other blocks.

## The design

### A new crate: `noido-typeset`

Commit prefix `typeset`. It depends on `noido-engine` and
`noido-blocks`, on `noido-fonts` (the fonts, above), and on parley
(shaping), `hypher` (hyphenation), skrifa (outlines), tiny-skia
(already in the workspace) and flate2 (already in the tree). It has no
GUI and no threads.

The pipeline:

1. **Blocks to paragraphs.** Walk the `Tree`. Each paragraph-like block
   becomes a run of styled text with its block's role (body, heading
   level, code, fallback).
2. **Shaping.** parley shapes each paragraph as one long line, with the
   font chosen per style and fallback per character (Libron →
   Libertinus Serif → New CM; Neon for code).
3. **Line breaking.** The shaped clusters become Knuth–Plass boxes,
   glue and penalties, as in the prototype's `kp.rs`: stretch half a
   space, shrink a third, tolerance 2, line penalty 10, 3000 demerits
   for consecutive hyphens, 100 for adjacent lines of different
   tightness. Hyphenation points come from `hypher` in the document's
   language; existing hyphens and dashes are break points too. A
   paragraph that can't be set within tolerance is set at the least bad
   breaks rather than failing.
4. **Page breaking.** Lines are stacked on the page's text block. A
   paragraph never leaves one line alone at the foot or head of a page
   (it moves a line over, or moves whole); a heading takes at least the
   first two lines of what follows with it. A page may run short;
   there's no vertical justification.
5. **The book.** The result is a value (`Book`, say) holding, per page,
   the placed glyphs, link areas, the lines for the locations map, and
   the destinations of headings. It is `Send + Sync`, and every
   `Document` method reads it.

### House style for Y3

Values the builder starts from. They can be tuned when the pages are
looked at by hand; E2 owns the house style after that.

- **Text block:** a 270 pt measure (about 63 characters of Libron at
  11 pt), centred across the page. 31 lines of 14 pt. Top margin 56 pt,
  the rest below.
- **Paragraphs:** justified, no space between them, the first line
  indented 1 em, except the first paragraph after a heading and the
  document's first.
- **Headings:** Libertinus Sans Bold, left-aligned, never hyphenated,
  never justified. Sizes 18, 14 and 12 pt for levels 1–3; 11 pt for
  4–6. Space above and below in half lines: one line above a level 2,
  half a line above levels 3–6, half a line below all. A level-1
  heading starts a new page, unless it's already the first thing on
  its page.
- **Inline styles:**
  - emphasis is Libron Italic, strong is Libron Bold, both are Bold
    Italic;
  - inline code is Monaspace Neon at 0.85 em, not hyphenated;
  - strikethrough is a line through the text;
  - links are in the text colour with a hairline underline;
  - a footnote reference is its number, smaller and raised;
  - a task box is ☐ or ☑;
  - an inline image is its alt text in italic.
- **The fallback for Y3b's blocks,** so a whole file reads:
  - each list item is a paragraph starting with its marker ("•", or
    its number);
  - a block quote's paragraphs are set as paragraphs;
  - a code block is Monaspace Neon at 0.85 em: not justified, not
    hyphenated, its lines kept as written, with a long line wrapping
    at the measure;
  - each table row is a paragraph, its cells separated by spaces;
  - a rule is a blank line;
  - footnotes are paragraphs where the source defines them, each
    starting with its number. Moving them breaks source order, which
    the locations map needs; placing them at the page foot is Y3b's
    problem;
  - an HTML block is not drawn (the block tree's rule: raw HTML isn't
    drawn in typeset mode);
  - front matter is not shown.

### Opening a markdown file

`AnyEngine` in `gui-egui/src/document.rs` picks the engine by the file's
bytes. After the PDF and ZIP checks, bytes that are valid UTF-8 are
typeset as markdown. Any plain text file is valid markdown, so a
`.txt` opens too. Anything else stays "not a readable document".
`Properties` gains `Format::Markdown`. Its title comes from the front
matter's `title`, or else the first heading; its author from the
front matter's `author`.

Front matter is read without a YAML parser. Y2's `FrontMatter` block
keeps no content, so the typesetter slices its source span and reads
top-level `key: value` lines: `title`, `author`, `lang` and `language`,
with surrounding quotes removed. Anything else in it (lists, nesting)
is ignored. Proper YAML parsing is a later decision point (see After
Y3).

The file dialogs (`gui-egui/src/open.rs`, `noido-macos/src/open_panel.rs`)
also offer `md`, `markdown` and `txt`. Registering noido with the
desktop as a markdown app (the desktop entry, Info.plist) waits until
markdown is more than a preview. From the command line any file opens.

`document.rs`'s test that `b"not a document"` fails to open changes:
that's valid UTF-8 now. It uses bytes that aren't.

The window opens its document on several threads, each through its
engine. The typeset engine lays a given file out once and hands the
same `Arc<Book>` to every `open` of those bytes. One engine instance
shared by the window's threads, memoising on the bytes, does this. The
builder chooses how.

### `Document`

- `page_count`, `page_size`: from the book; every page is the paper.
- `render_tile`: fills white, then draws each glyph in the tile with
  its skrifa outline through tiny-skia, with the text colour, and the
  underlines and strike lines. Outlines are cached per font, glyph and
  size, so a page's second draw doesn't parse the font again. The
  prototype's rasteriser, which parsed the font per glyph, is not the
  model here.
- `text`: one `Glyph` per cluster, in reading order. **Spaces are
  glyphs too** (text `" "`), so `noido-text` inserts no space of its
  own. A ligature is one glyph with all its characters, as for PDF.
- **Hyphens at a line end.** A hyphen the typesetter adds at a break is
  a glyph with no source. Copy and search must drop it, as they do for
  PDF. A hyphen that's in the source and happens to fall at a line end
  must stay: "well-known" copies as "well-known". Today `noido-text`
  can't tell the two apart. Before a lowercase letter it always drops
  the hyphen (`search.rs`, `copy.rs`). So an added hyphen is marked:
  for example, its glyph text is U+00AD (soft hyphen), which
  `noido-text` always drops at a line end, while on pages from a
  reflowing engine a plain `-` is kept. The mechanism is the
  builder's; the behaviour is the requirement, and has a test.
- `links`: the areas of each `Style::Link`, one rectangle per line it
  spans. A destination starting with `#` is internal: a heading's slug
  (GitHub's rules: lowercase, punctuation dropped, spaces to hyphens,
  `-1`, `-2` for repeats) or a footnote's label. Anything else is
  `LinkTarget::Web` as written. A footnote reference links to its
  footnote.
- `named_destination`: heading slugs and footnote labels, to their
  page and the top of their first line.
- `outline`: the headings as a tree by level, each targeting its page
  and the top of its line, all open.
- `page_labels`: none; pages are numbered.
- `properties`: as under Opening.
- `locations`: the map described next.

### Locations

A markdown file is one part, and locations count the characters of its
source (locations spec, decision 1). The map's lines must name, for
each page character, the source character it came from.

**What a page's characters are.** They are `noido-text`'s `PageText`
built from `text(page)`, which:

- takes the glyphs' text in order, NFC-normalised;
- inserts a newline between lines;
- inserts a space wherever two glyphs are more than 0.1 em apart and
  the first doesn't end in whitespace.

The typesetter emits a space glyph for every space, and keeps every
other gap on a line under 0.1 em, so no space is inserted. That
includes the padding around inline code and a raised footnote number.
It counts characters after NFC: where normalising changes the count (a
decomposed accent in the source), that stretch is handled like a
segment that isn't one-to-one. The typesetter counts the same way when
it builds the map, and a test holds the two together (below).

**Runs.** For each line, walk its glyphs' characters. Through Y2's
`Text`, each drawn character maps to a source character (`source_of`),
and a segment that is one-to-one maps a stretch of drawn characters to
a stretch of the source. Consecutive characters whose source characters
are consecutive form one `Run`. A character with no source breaks the
run and is left out of every run: an added hyphen, a list marker, a
newline. So does a jump in the source, where markers like `*` were
dropped. A segment that isn't one-to-one (`--` drawn as `–`, `&amp;`
as `&`) is a run of its drawn length starting at its source start,
never longer than its source.

**The map gets explicit page offsets.** Today
`LocationMap::from_lines` sums each page's runs to find where each
starts on the page. That assumes the runs cover every page character
back to back, which no longer holds: newlines and added hyphens sit
between them. The engine gives each run's offset on its page
instead, and `from_lines` keeps it (the shape of the change is the
builder's). The identity map, and PDF's behaviour, don't change.

**The guarantee, as a test:** for every page of the sample fixture,
and every run, the characters of `PageText::new(text(page))` at the
run's offset are the drawn characters whose source the run names.

**The layout key.** L3 stores a layout key with a reflowing document's
view, as an opaque string, and Y3 is its first giver. Nothing carries
a key from the engine to the window yet: the app passes `None`
(`app.rs`, where the view is restored and saved). Y3 adds the path, a
`Document` method with a `None` default, say, read beside
`locations()`. It gives a key that names the paper and type (for
example `"b-format 11/14"`). A view saved under another key is restored
through its anchor rather than its page, which is what E3's paper
changes will need.

### Errors

The engine opens whatever valid UTF-8 it's given: markdown never fails
to parse. A glyph no font has is drawn as the last fallback's
`.notdef`. It keeps its character in `text`, so search and copy still
see it.

## Testing

TDD throughout. The tests live mostly in `noido-typeset` and use only
`fixtures/` (the Y2 `sample.md`, and small inline strings).

- Knuth–Plass on fixed widths (no fonts): a paragraph breaks where TeX
  would on a known example; hyphenation is taken when it lowers
  demerits; a paragraph with no good breaks is still set.
- Page breaking: no single paragraph line at the foot or head; a
  heading moves with its first two lines; a level-1 heading starts a
  page.
- The locations guarantee above, on the sample fixture. Also a
  highlight's quote, taken through the map, equals the source text
  between its locations, markers aside.
- Hyphens: a word the typesetter hyphenates copies and searches whole;
  a source "well-known" broken at its hyphen copies as "well-known".
  PDF de-hyphenation is unchanged.
- Links, named destinations and the outline on the sample fixture.
- Rendering: a page renders non-white pixels inside the text block and
  white outside it; drawing it twice gives the same pixels.
- `AnyEngine`: UTF-8 bytes open as typeset pages, other bytes still
  fail; the window's threads share one layout.
- In the GUI harness: a markdown fixture opens, pages turn, a search
  finds a word, and a highlight saved and restored lands on the same
  characters.

By hand, after a release build: open a long markdown file from
`../testfiles/` (an essay, a novel converted to markdown), look at the
pages, time the open, search, highlight, follow links and the outline.

## Left to the builder

- The crate's internal structure and type names (`Book` is a
  placeholder).
- How the engine shares one layout among the window's threads.
- The exact shape of the `LocationMap` change.
- The `noido-fonts` crate's API and feature names, within what's
  above.
- The parley version: the current one. Note the investigation's
  `harfrust` duplication with egui's.
- Fine values of the house style, within what's above.

## After Y3

- **Y3b:** the other blocks set properly.
- **M1** (markdown typeset mode) is partly done by Y3. What's left of
  it is the markdown-specific house style.
- **E3** brings the paper settings, and with them the first relayout of
  an open document.
- **Front matter as YAML:** a decision point for later. Parse it
  properly (a YAML dependency) once something needs more than simple
  `key: value` lines: author lists, dates, EPUB-like metadata.
- **Alegreya Sans** stays a candidate sans. It can sit in `noido-fonts`
  from the start; noido turns its feature on when E2 or E3 gives the
  reader a choice of sans, not in Y3, which wouldn't draw it.
