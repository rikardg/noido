# Typesetting EPUB and markdown: typst or parley

Date: 2026-11-15

Status: measured, with a prototype. **Decided (2026-11-15): parley with
our own Knuth–Plass line breaking and page breaking.** Micro-typography
(hanging punctuation, font expansion) comes later, as separate steps.
The plan this belongs to is at the end ("The plan").

## Question

EPUB and markdown should be typeset well, close to TeX, and shown
through the same `Document` trait as PDFs, so page modes, search,
links and notes work the same. Use typst as a library, or shape with
parley and break lines ourselves? What quality, weight and speed does
each give?

## Answer

- **Parley with our own Knuth–Plass line breaking and `hypher`
  hyphenation sets body text about as well as typst.** In the renders the
  two are hard to tell apart. Both are far better than parley's own
  greedy breaking, which leaves wide gaps in a third to a half of the
  lines.
- **typst costs about ten times the binary size.** A stripped binary
  with only the typst backend is 43 MB, 9.6 MB of which is bundled
  fonts. The parley backend is 4.0 MB. A binary with neither is 0.4 MB.
  typst also brings 144 crates noido doesn't have yet. Parley brings 8,
  because egui 0.37 already uses its shaper, font crates and most of ICU.
- **Both are fast enough.** Parley + Knuth–Plass laid out 16 pages in
  11 ms, about 0.7 ms a page. typst took 70 ms for the same 16 pages
  from cold, about 4.5 ms a page, and 1–9 ms to lay out again after a
  one-word edit (it is incremental).
- **What typst gives on top:** tables, footnotes, math, floats,
  page breaking that weighs widows and orphans, and a styling language.
  With parley, all of that is ours to write.

**Recommendation: parley + our own line and page breaking.** EPUBs and
essays are mostly running text, headings, lists, block quotes, images,
simple tables and footnotes. None of those needs typst, and the core
of the quality (Knuth–Plass and hyphenation) took about 300 lines of
prototype. typst would be worth its weight only if math, or complex
tables, became a must. Even then, math could be its own smaller
dependency.

## Setup

The prototype is in `2026-11-15-typesetting/`: `Cargo.toml.txt`
(rename it to `Cargo.toml` to build) and `src/`. It typesets the same
input three ways and writes every page as a PNG, plus side-by-side sheets
(`<name>-compare-…-pN.png`) of the first four pages:

1. **typst** 0.15.1 as a library: the blocks become typst markup
   (`typst_backend.rs`), typst lays out the pages and `typst-render`
   draws them.
2. **parley greedy**: parley 0.13 shapes and breaks lines itself, with
   `Alignment::Justify` (`parley_backend.rs`, `greedy`).
3. **parley + Knuth–Plass**: parley shapes each paragraph as one line.
   Its clusters' widths become Knuth–Plass boxes, glue and penalties
   (`kp.rs`), with hyphenation points from `hypher` and breaks after
   hyphens and dashes already in the text. The glyphs are then placed
   at the chosen breaks, with the spaces stretched or shrunk.

All three use Libertinus Serif at 11 pt on a 14 pt baseline grid, on a
5.5 × 8.5 in page with a 288 pt measure, and the same first-line
indent. Glyphs are drawn with skrifa outlines through tiny-skia, the
rasteriser `typst-render` uses, so the comparison is about layout.
Both parley modes share one simple paginator: a heading keeps two lines
with it, and a paragraph never leaves a single line at the foot or head
of a page.

The input is two public-domain chapters from Project Gutenberg, cut
into a tiny markdown subset by `extract.py`. One is dialogue-heavy
fiction (chapters I–III of a novel, 3,400 words, pg1342). The other is
dense essay prose with long paragraphs (the introduction of a
philosophical essay, 5,600 words, pg34901). They aren't checked in.
Download `https://www.gutenberg.org/cache/epub/1342/pg1342.txt` as
`pp.txt` and `…/34901/pg34901.txt` as `lib.txt`, then run
`python3 extract.py <dir>` and
`typeset <typst-assets>/files/fonts <out> pride.md liberty.md`.

## Results

### Quality

The adjustment ratio is how far a line's spaces were stretched (or
shrunk), as a share of what they may: stretch is half a space, shrink a
third, as in TeX. Above 1 a line looks loose, above 2 it shows gaps.
Last lines of paragraphs are left out.

| | Lines | Mean \|ratio\| | Max | Loose (> 1) | Very loose (> 2) | Tight (< −0.5) | Hyphenated |
|---|---|---|---|---|---|---|---|
| Fiction, greedy | 254 | 1.12 | 5.30 | 113 (44%) | 40 (16%) | 0 | 0 |
| Fiction, Knuth–Plass | 245 | 0.44 | 1.73 | 11 (4%) | 0 | 31 | 35 (14%) |
| Essay, greedy | 535 | 1.41 | 6.43 | 289 (54%) | 132 (25%) | 0 | 0 |
| Essay, Knuth–Plass | 507 | 0.43 | 1.38 | 7 (1%) | 0 | 82 | 119 (23%) |

The prototype doesn't measure typst's lines. In the renders its spacing
looks like Knuth–Plass's. It hyphenates about as often (3 against 4
hyphens on the same essay page) and runs slightly looser where
Knuth–Plass shrinks.

What the renders show:

- **Greedy:** rivers and gappy lines all through the essay ("which the
  ruler should be suffered to exercise over the"). Parley can only
  stretch the spaces, and it never hyphenates.
- **Knuth–Plass and typst:** even colour on the page, and hyphens at
  sensible syllable breaks ("po-litical", "spe-cific", "magis-trates").
  Knuth–Plass hyphenates a little more often. That's tunable through the
  hyphen penalty and the demerits for consecutive hyphens.
- **Page breaks:** typst's are more careful. On the fiction's first page
  it moved a short paragraph to the next page rather than split it, and
  left the page two lines short. The prototype paginator only prevents
  single lines.

### Weight

| Binary (release, stripped) | Size |
|---|---|
| Neither backend (tiny-skia, std) | 0.4 MB |
| parley + skrifa + hypher | 4.0 MB |
| typst + typst-render + bundled fonts | 43.3 MB (33.7 MB without the 9.6 MB of fonts) |

typst's own code is about 129k lines, and it pulls in 269 crates.
Those include citations (hayagriva, citationberg, biblatex), a
WebAssembly interpreter for plugins (wasmi), syntect, image decoding,
SVG, YAML, TOML and HTML export. None of them can be turned off.
Parley is about 22k lines in 57 crates, and all but 8 of those are
already in noido through egui. There is one wrinkle: egui uses
`harfrust` 0.12 and parley 0.13 uses 0.15, so there would be two
copies of the shaper until egui catches up.

### Speed (release, on a Mac)

| | Fiction (11 pages) | Essay (16 pages) |
|---|---|---|
| typst, cold | 15–38 ms | 70 ms |
| typst, after a one-word edit | 1–2 ms | 9 ms |
| parley greedy | 4 ms | 6 ms |
| parley + Knuth–Plass | 6 ms | 11 ms |

Speed matters because every change of font size or window width lays
the whole document out again. For a 300-page book that's roughly
0.2 s with parley and 1.3 s with typst, unless it's done a chapter at a
time. typst's incremental relayout suits live markdown editing. Parley
can get a similar effect by caching shaped paragraphs and laying out
only the edited ones again.

The prototype's own rasteriser is slow (110–200 ms) because it parses
the font for every glyph and caches nothing. It's not a measure of what
noido would do.

## What we would build on parley

The prototype covers paragraphs, headings and italics. A full backend
would add:

- **Line breaking:** Knuth–Plass (done in the prototype), then tuning,
  hanging punctuation (margin protrusion) and, maybe, slight
  letter-width expansion (font expansion).
- **Page breaking:** widows, orphans, keep-with-next for headings,
  footnotes at the page foot, and images that fit or move.
- **Blocks:** lists, block quotes, code blocks (monospace, no
  justification), simple tables, rules and images.
- **Source spans:** every parley cluster has its `text_range`, so each
  glyph can carry its offset in the source. Search, selection and notes
  need that (see the locations spec).
- **Glyphs:** draw them through the same path as PDF pages, with a
  glyph cache, rather than with this prototype's rasteriser.

## The plan

The aim: EPUB for reading and taking notes, and markdown for the same
plus iterating on a text (an article, an essay) while seeing it
rendered. Markdown has two modes. **Raw** looks like `glow` does in a
terminal: the `##` and `*` markers stay, but tables are laid out as
tables. **Typeset** is the same careful typography as EPUB.

The shape agreed in conversation:

- **One block tree for both formats,** with source spans: headings,
  paragraphs, lists, block quotes, code, tables, images, footnotes, and
  inline emphasis and links. Markdown is parsed into it (pulldown-cmark
  or comrak). EPUB goes through the container, spine and nav, then each
  chapter's XHTML.
- **One typesetter,** with themes. Typeset markdown and EPUB use one house
  style, and the publisher's CSS is mostly ignored (a small subset is
  kept). Raw mode is the same tree under a source theme: monospace, the
  markers kept as literal text, tables laid out.
- **Behind `Document`:** the typesetter cuts its output into pages of a
  given size and implements the engine trait. Page modes (continuous,
  dual, odd-left), spreads, render threads, search, links and the
  outline then work the same as for PDF, whatever the format.
- **Virtual paper:** text is set on a page size, text size and font, and
  margins, and the pages then behave like a PDF's (resizing and zooming
  magnify them). The layout is locked to that paper, or unlocked with
  pages as wide as the window. EPUB and typeset markdown are locked by
  default, raw markdown unlocked.
- **Reflow changes page counts:** changing the paper, or resizing an
  unlocked layout, lays the document out again. Everything kept is
  anchored to a location in the source, not to a page. See
  `docs/superpowers/specs/2026-11-15-unified-locations-design.md`.
- **Markdown iteration:** reload when the file changes and keep the place
  (noido already watches files), open the source in an editor at the
  line, and re-anchor notes by their quote after edits.

The steps:

| ID | Step |
|---|---|
| L1 | Spec: unified locations (agreed) |
| L2 | PDF onto the location type, same behaviour and saved form |
| L3 | Core: a reflow event; the first character in view keeps its height on screen |
| Y1 | This investigation (done) |
| Y2 | The shared block tree with source spans |
| Y3 | The typesetter behind `Document`: pages of a given size, glyphs carrying source spans |
| E1 | EPUB: container, spine and nav into parts and outline; XHTML into the block tree |
| E2 | EPUB: house style, a small subset of publisher CSS |
| E3 | Later: settings for paper size, text size and font, margins (defaults until then) |
| M1 | Markdown: typeset mode |
| M2 | Markdown: raw mode (source theme, markers kept, tables laid out), unlocked by default, with a lock toggle |
| M3 | Markdown: reload keeping the place; open the source in an editor at the line |
| M4 | Notes re-anchored by quote after edits (fuzzy), for markdown and rebuilt PDFs; revisit showing orphans |
| M5 | Later: stop reading the old highlight field names, some time after L2 lands |
| — | Later: micro-typography (hanging punctuation, then maybe font expansion) |

## Notes for later

- An EPUB is a ZIP, like a CBZ, so detection by magic bytes alone can't
  tell them apart. An EPUB's first entry is an uncompressed `mimetype`
  file reading `application/epub+zip`, which shows up at a fixed offset
  in the file.
- typst's styling language is appealing for user-written themes. That's
  a reason to keep the block tree independent of the typesetter, so
  typst could come back as a second backend later.
