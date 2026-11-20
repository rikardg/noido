# A page's text map and glyph rectangles: design

Date: 2026-11-20

## Problem

Issue #96, item 2. A page's text holds a 24-byte `CharInfo` per character
(`Option<usize>` and `usize`) and a 16-byte `Rect` per glyph, copied from
the engine's quads into the upright frame. On the 356-page book of the
2026-11-15 investigation that is 36.7 MB of the 37.8 MB the app keeps as
page text — and `Texts` now keeps up to 32 MB of it — against 24 bits of
information per character and a rectangle whose y and height are almost
always the line's.

This is item 2 only. The folded forms (item 1) and what hayro keeps (item
3) are not touched.

## Change

### The character map

`PageText` keeps one `u32` per character: the index of the glyph it came
from, `u32::MAX` for an inserted space or newline. A `Vec<u32>` of line
starts beside it says where each line begins, so `line_of` binary-searches
instead of reading a field.

Lines are contiguous and each has at least one character (a glyph, or the
newline that ends it), so `line_starts[i]` is the first character of line
`i` and `line_starts.len()` is the line count.

### The glyph rectangles

A glyph keeps only its horizontal span in the upright frame: `x` and
`width` as `i16` at a sixteenth of a point. The vertical band — `y` and
`height` — is kept once per line, as the union of its glyphs' exact
rectangles. `glyph_bounds` returns the glyph's span with its line's band;
`lines()` returns the bands.

The range is the price of the sixteenth: an `i16` covers -2048.0 to
2047.9375 points. Today `x` and `width` are `f32` — no practical bound,
about seven significant digits (0.001 pt at 10,000 pt). Compact spans are
1/16 pt (0.0625 pt, 0.022 mm), `x` and `width` each rounded to the nearest
sixteenth, so a span's left edge moves by at most 1/32 pt and its right
edge by at most 1/16 pt (0.08 px at 96 dpi, under a pixel at 8× zoom).
The band is `f32`, so vertical geometry is never quantised.

The 2048-point limit is on the upright frame's x, which is the unturned
page's width for upright text: A2 and ANSI C (1684 and 1584 pt) fit; a
landscape A1 (2384 pt), ANSI D (2448 pt), a 200-inch PDF page or a plan
does not. A span wider than 2047.9375 pt does not fit either, wherever it
sits. A page with any span that does not fit keeps `x` and `width` as
`f32` for that page instead — 8 bytes a glyph, still half of today, with
exactly today's geometry. So no page in the PDF spec's 14,400-unit range
loses selection, carets or hits: the limit chooses a page's storage, not
whether it can be read. The fit is checked once, over the page's
glyphs at build; it costs nothing per frame.

### Memory accounting

`memory_bytes` counts the new buffers: `u32` sources, `u32` line starts,
the bands, and the glyph spans (4 or 8 bytes each). `Forms::memory_bytes`
is unchanged.

### Reach

On the 356-page book a mean page holds 1,986 characters, 1,952 glyphs and
36 lines: 73 kB of character map, 30 kB of rectangles and 3 kB of text
today, 8, 8, 1 and 3 kB after — 106 kB against 20 kB, unsearched. The
32 MB budget therefore holds about 300 unsearched pages today and about
1,600 after; searched, the 80 kB of forms (item 1's) makes it about 170
today and 320 after, or 580 once item 1 lands. Today's 170 searched pages
fit in about 17 MB after this change, 9 MB with item 1.

Decided with these numbers: 16 MB now — about 835 unsearched or 160
searched pages of this book, against 300 and 170 at 32 MB before the
change — and 9 MB after item 1 restores today's searched reach.

## What changes for callers

The public API stays: `text`, `char_count`, `line_of`, `glyph_of`,
`glyph_bounds`, `rects`, `caret_at`, `word_at`, `line_range_at`,
`caret_rect`, `line_rect_of`, `line_near`, `bar_across`, `find`. Inside
the crate:

- `lines()` returns `&[Option<Rect>]` rather than a fresh `Vec`; callers
  index or iterate it.
- `rects`, `caret_rect` and `glyph_chars` read a line's character range
  from the starts rather than filter every character.
- `noido-typeset`'s `page_runs` keeps calling `line_of` and `glyph_of` per
  character; both answers are the same, the first through a binary search.

One visible difference: `glyph_bounds` on a line with glyphs of unequal
height returns the line's band rather than the glyph's own vertical
extent. Search hit rectangles already union a line's glyphs and selection
rectangles already use the line's band, so they now agree; a one-word hit
on a mixed-size line may be as tall as the line.

## Approaches considered

1. **4 bytes a glyph, band on the line** (this design). ~7.0 MB of page
   text on the 356-page book, from 37.8 MB. x/width quantised to 1/16 pt;
   mixed-size lines give up their per-glyph height for the line's band.
2. **4 bytes a glyph, `f32` x/width**. ~10.5 MB; no quantisation or range
   limit, but 8 bytes are spent on the two values instead of 4.
3. **Four `i16` a glyph**. ~11.0 MB; keeps each glyph's own y/height, at
   the same range limit and quantisation as 1. ~3 MB more than 1 for a
   vertical extent no caller draws.

Approach 1: the extra 3 MB is about a sixth of a reach-sized budget, a
sixteenth of a point is sub-pixel at reading zooms, and the fallback
keeps large pages exact. The line band is what selection already draws.

## What the reader sees

Nothing different in selection, caret, highlights, links, copy or search.
The folded forms, the hits and the engine's share are as before. A page
the budget lets go is read from the engine again when it is next shown —
1.2 ms a page in the measurement — on the text thread; the document stays
open, and a selection or copy that needs it waits while it is read.

## Tests

TDD: the memory test first, watched failing; then the geometry changes
against the existing suite.

- `noido-text`: a page of prose costs under 40 kB (fails now), and
  `memory_bytes` equals its parts, with the compact span size.
- A page whose spans fit i16/16 stores them compactly and returns them to
  within 1/16 pt; one that does not falls back and returns them exactly.
- A mixed-size line returns its band from `glyph_bounds`; `lines()` keeps
  its bounds for single-size lines, and a line without glyphs cannot arise.
- `line_of` is unchanged for glyphs, inserted spaces and newlines, and
  `line_range_at` still cuts the newline; `rects`, `caret_rect`,
  `caret_at`, `word_at`, `line_near`, `bar_across`, rotation and search
  keep their existing tests, exact where they were exact.
- `noido-typeset` and `noido-render` suites unchanged.

## Measurement

Before implementing, measure the current code on the three books of the
2026-11-15 investigation (a management history, a Git book, an RPG
rulebook) with a throwaway crate outside the repo, a counting allocator
and `memory_bytes`; measure again after. Report per page (mean, median),
per book, and the number of pages 32 MB now holds with and without a
search. The numbers go on #96.

What to do with `TEXT_BUDGET` is a separate decision from those numbers:
keep 32 MB for more reach, or lower it to hold today's reach in ~9 MB.

## Not changed

- The folded forms (item 1), the search worker, and what hayro keeps
  (item 3).
- `TEXT_BUDGET`, `Texts`, `MAX_HITS` and the hits' contexts.
- The public `PageText` signatures and the test documents: the repo's
  tests keep using `fixtures/`; the three books are measured outside it.
