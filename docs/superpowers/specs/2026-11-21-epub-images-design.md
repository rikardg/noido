# EPUB images (E4)

## Why

E1 sets an EPUB as typeset pages, but draws every image as a stand-in:
its alt text in italic, or `[image]`. Real books lose real content that
way: the cover, chapter-title art, maps, family trees, charts. E4 draws
the images. It comes before E2 (house style) because E2's choices are
easier to judge with the images in place.

## What the test books hold

Measured on the six books in `../testfiles/` (about 170 images):

- Nearly all JPEG; some GIF (a fantasy novel's chapter-title art), a
  few PNG. No SVG image files. The `image` crate (already a workspace
  dependency, used by `engine-comic`) reads all of them.
- Most images stand alone in their block: `<p class="image"><img/></p>`
  (a non-fiction book's charts), `<div><img alt="TITLE"/></div>` (a
  story collection's title art), `<figure><img/></figure>` (a historical
  novel's family trees). Some are wrapped in a link (`<a><img/></a>`,
  publisher ads).
- A few sit in running text: a non-fiction book uses a small image as a
  bullet at the start of paragraphs.
- Sizes range from 50 × 50 px logos to 2300 × 1737 px maps; one book's
  images total 19 MB.
- Images are sized by publisher CSS classes (`img40`, `img90`), which
  are not read before E2; a few carry `width`/`height` attributes.

## Goals

- **Figures.** An image is a figure, drawn as a block of its own and
  centred on its paragraph's measure (indented in a list or quote),
  when either:
  - it is its paragraph's only drawn content (whitespace and empty id
    anchors aside; a wrapping link allowed), or
  - its natural height (below) is more than two lines (28 pt): a chart
    with its caption in the same `<p>`, or two images side by side.
    The paragraph is split around it: the text before, the figure, the
    text after, each piece keeping its source map; images side by side
    are stacked. (My call, 2026-11-21.)
- **A heading holding only an image** (chapter-title art as
  `<h1><img alt="Chapter One"/></h1>`) is a figure that keeps the
  heading's rules: a level-1 one starts a page, it keeps with what
  follows, and ids and TOC entries on it still land there.
- **Inline images.** Any other image (natural height up to two lines)
  is drawn in the line, no taller than the text.
- **The cover.** A cover part with an image is drawn as that image,
  filling the page. The E1 title page is dropped when there is an image
  to draw; it stays for a cover without one (user, 2026-11-21).
- **An image straight in the spine** (not first) is a part holding that
  image as a figure, no longer an unreadable notice.
- **SVG wrappers.** An `<svg>` holding an `<image>` (outside the cover)
  is that image, as an `<img>` would be (moves #124's svg-figure item
  here).
- **Locations are unchanged.** As in E1, a part's characters are its
  text nodes only: an image is drawn as an object character (U+FFFC)
  with a zero-length source span, so it has no location of its own and
  no saved highlight or position moves. A highlight can't start or end
  on an image; one across a figure works through the text either side.
  A part that holds only an image gets a zero-length run at its start,
  as E1's cover does, so `page_start` answers. Search and copy work as
  for E1's stand-in: the image's glyph carries the stand-in text (alt
  text, or `[image]`).
- **Pages move, places don't.** Figures make illustrated chapters
  longer, so page numbers change. `LAYOUT_KEY` changes (to
  `"b-format 11/14 images"`), so a saved reading position restores by
  its anchor rather than its place on the old page, as for any reflow.
- **Zoom shows detail.** Pages are drawn at the zoom being viewed, so an
  image is drawn from its full resolution: a map shrunk to the page
  shows its detail when zoomed in.

## Non-goals

- Publisher CSS sizing (E2), floats, text wrapping around images.
- Filling the gap a figure leaves at the foot of a page when it moves
  to the next (no float placement); the gap stays.
- Keeping a caption with its image: `figcaption` stays an ordinary
  paragraph, as in E1.
- SVG image files and inline SVG drawings (no SVG renderer is added);
  they keep the stand-in.
- Images in markdown files: the engine gets the file's bytes only, not
  its directory, so `![alt](src)` keeps its stand-in (a later M item if
  wanted).
- Opening an image on its own, full window (a later idea).

## Approaches

- **A. Every image an inline box in its line** (parley's inline boxes).
  One path, but a lone image becomes a 400 pt "line" running through
  rules made for text (widows and orphans, line spacing), with no
  natural place for "shrink to fit or move on".
- **B. Figures as blocks, other images inline at text height.** Two
  paths, each with rules that suit it. **Chosen** (user, 2026-11-21).
- **C. Figures only, inline images kept as stand-ins.** Cheapest, but
  the bullet images would read `[image]`. Dropped.

## Design

### Size

- **Natural size:** the image's pixel size as CSS pixels, 1 px = 0.75 pt.
  When the `<img>` has `width` and/or `height` attributes in plain
  numbers (CSS pixels), they win; one alone keeps the aspect ratio.
  Percentages and units are ignored.
- **A figure** is its natural size, shrunk (aspect kept) to fit the
  measure (270 pt) and the text block's height (31 × 14 pt), never
  enlarged. Centred on the measure.
- **An inline image** is its natural size, shrunk (aspect kept, never
  enlarged) to the line's ascent: the ascent of the font of the text
  it sits in. It sits on the baseline.
- **The cover** is shrunk or enlarged to fit the whole page (the paper,
  margins ignored), aspect kept, centred; the rest is white.

### Pages

- A figure is a flow of one line, the image's height, not splittable,
  with a line's space before and after (`LEADING / 2` each, dropped at a
  page's top as space before is now). The paginator already moves an
  unsplittable flow that doesn't fit to the next page and places it on
  an empty page; since a figure is never taller than the text block, it
  always fits there.
- A heading keeps the figure that follows it with it, as it keeps a
  paragraph's first lines (the figure counts as one line).
- A figure has one glyph: the image's rectangle, its one character the
  image's object character, carrying the stand-in text and no source
  (as E1's stand-in glyphs have none).
- A link around a figure or inline image makes the image's rectangle
  the link's area, as for text.

### Reading the images (noido-epub)

- `Epub` keeps what it needs to read images later: the book's bytes
  (`PdfBytes`, already mapped or shared; no copy) and nothing decoded.
  It gains `fn image(&self, path: &str) -> Option<ImageSource>` or an
  equivalent that reads one entry, bounded.
- `PartKind::Cover` gains the cover image's path, when there is one:
  the spine item itself when it is an image, else its first `<img src>`
  or svg `<image href>` (`xlink:href` too), else the package's
  `cover-image` / `<meta name="cover">` item. A cover with none is set
  as the E1 title page.
- An image's `src` is resolved against its part's path with
  `noido_epub::resolve`, percent-decoded (`src` decoding moves from
  #124 here).
- Only an image's header is read when the book is laid out (its pixel
  size); the image is decoded when a page showing it is drawn.

### Drawing (noido-typeset)

- An image whose header can't be read (missing from the archive, not
  JPEG/PNG/GIF/WebP/BMP/TIFF, corrupt, or over the pixel limit) is
  drawn as E1's stand-in text, in the line, as before.
- An image whose header reads but whose decode fails at draw time is
  drawn as a thin grey frame of its laid-out size, so the page doesn't
  change shape after layout.
- Decoded images are kept in a cache shared by the book's threads,
  bounded in bytes (64 MB), least recently used out first. Drawing
  scales from the decoded image to the drawn size with a filter that
  doesn't alias when shrinking (a map at 8:1 must stay legible): the
  `fast_image_resize` Catmull-Rom path `engine-comic` uses, the result
  kept per image and drawn size.
- Limits as `engine-comic`'s: 32 M pixels per image, an entry read no
  further than 256 MB; the header read capped (8 MB). These guard the
  render thread from a hostile archive.
- The image code `engine-comic` already has (header read, bounded
  decode, resize) is shared, not copied: moved into a small crate both
  use (`noido-image`), with the comic engine's behaviour unchanged.
- The 8 MB layout limit still counts the spine's text, not images.

## Tests

Tests use generated fixtures only (`fixtures/make_fixtures.py`): small
PNG/JPEG images written into the EPUBs.

- `book.epub`'s cover is drawn as its image: page 1 has the image's
  colours, no title text.
- A figure: a lone `<img>` paragraph gives one glyph whose rectangle is
  the image's laid-out size, centred on the measure; the page's pixels
  show the image there.
- A figure too wide is shrunk to the measure; one too tall to the text
  block; a small one is not enlarged; `width`/`height` attributes win.
- A figure that doesn't fit what's left of a page starts the next; a
  heading before it goes with it.
- An inline image is no taller than the line's ascent and sits in the
  line with text either side.
- A missing or corrupt image keeps the stand-in text; a decode failure
  at draw time draws the frame.
- An image in the spine (not first) is a figure part.
- An svg-wrapped `<image>` is drawn as an image.
- Locations don't move: a part's character count and every id's place
  are the same as under E1 for a fixture with figures; a highlight from
  the text before a figure to the text after it resolves; search finds
  alt text, copy gives it. A part holding only an image has a
  `page_start`.
- A large image inside a paragraph splits it: text before, figure, text
  after, the text's locations unchanged. Two side-by-side images stack.
- A heading holding only an image is a figure that starts a page (level
  1) and is where its TOC entry goes.
- Caching: drawing the same page twice decodes each image once (a
  test-only counter, not timing).
- `engine-comic`'s tests pass unchanged after the move.

By hand: the six test books, covers on page 1, title art, the family
trees and maps zoomed in, the bullet images.
