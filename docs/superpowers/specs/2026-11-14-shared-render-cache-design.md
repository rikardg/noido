# One hayro render cache per document

Date: 2026-11-14 · Status: small change, agreed in conversation · Issue #67

## Intent

`engine-hayro` builds a new `RenderCache` for every render, so fonts,
glyph outlines, colour spaces and (since hayro 0.9) rasterised glyphs are
only reused within one render of one page. Share one cache across the
renders of a document. It is also the first step towards tiled rendering,
where several renders of the same page follow each other.

## Measured (on `../testfiles/`, a Mac, release build)

- 40 pages in a row, one cache against one per render: a text-heavy book
  44–49% faster, art books 4–18%, scans 0%.
- Rendering one page again with a warm cache: 0–19% faster.
- Memory: about 0.1–0.4 MB per page. One art page through ten zoom steps
  of ×1.25 with one cache grew it to 24 MB, since glyphs are rasterised
  per size.
- At one zoom, 200 pages in a row: an art-heavy book's cache grew by
  84 MB (about 0.42 MB per page, without levelling off); text books
  levelled off at about 7 MB. Hence the limit of 64 renders, which bounds
  it to roughly 25 MB.
- Pages whose cost is a large image (scans, and figures embedded as big
  raster images) gain nothing: hayro 0.9 doesn't cache decoded images.

## Approaches

1. **Own the `Pdf` and the cache together** with `self_cell` (the cache
   borrows from the `Pdf`). Chosen: a small change inside `engine-hayro`.
2. A cache keyed so that it doesn't borrow: needs changes in hayro.
3. One cache per batch of renders, passed in by the render thread: leaks
   hayro's lifetimes into the engine trait.

## Behaviour

- `HayroDocument` owns the `Pdf` and one `RenderCache`; every render
  (`render_tile`, `render_page`) uses it.
- hayro offers no size limit and no way to empty the cache, so it is
  replaced in two cases: when a render's scale differs from the scale the
  cache was started at by more than 1% on either axis, and after it has
  served 64 renders. The first keeps it to one zoom level's worth of
  glyphs; the second bounds the growth at one zoom (see Measured). Page-to-page
  reuse stays: one cold render in 64. The 1% margin keeps pages of slightly
  different sizes at the same zoom from resetting it (`render_page`
  derives each axis's scale from rounded pixel sizes).
- The cache is taken out of its slot for the duration of a render and put
  back only when the render ends normally. A render that panics (the
  render thread catches it) drops the cache, whose mutexes the panic may
  have poisoned, and the next render starts a fresh one.
- Each render and thumbnail thread opens its own document, so the
  thumbnails' scale never resets the main view's cache.

## Tests

- A render with the shared cache is pixel-identical to one with a fresh
  cache: the same page twice, another page, and after a scale change.
- 64 renders at one scale keep one cache; the 65th starts a second.
- After a render that panics, the next render at the same scale starts a
  new cache and succeeds.
- A test-only count of caches started: renders of several pages at one
  scale (and within 1%) keep one cache; a zoom change starts a new one.

## Not included

Decoded-image caching, vello_cpu threads, tiles.
