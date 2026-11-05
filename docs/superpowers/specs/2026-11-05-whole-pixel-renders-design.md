# Renders of whole pixels, without a light edge

In gapless mode a thin light line shows between pages: down the seam of
a spread, along the bottom of a row, and on the outer right edge of a
page against the background.

## Why

A page is rendered at `scale` into `ceil(width × scale)` by
`ceil(height × scale)` pixels. The last column and row are only partly
page; the rest of them is the white the engine clears to first. Dark
content at the page's edge comes out lighter there. The frontend crops
the texture to the page's exact fraction (`page_uv`), but linear
filtering still blends in the light column and row. With a gap between
pages the line is lost against the background; without one it shows.
The top and left edges start exactly on a pixel boundary, so they are
clean.

## What changes

- The engine gets `Document::render_page(page, width, height)`: the
  whole page, stretched to fill exactly `width` × `height` pixels, on
  white. hayro scales each axis on its own to do it. The default
  implementation calls `render_tile` at `width / page width`, so test
  engines need no change.
- The render and thumbnail threads ask for the page's size times the
  scale, rounded to the nearest pixel (at least 1), through
  `render_page`. The stretch this adds is at most half a pixel over the
  page, which can't be seen.
- The frontend draws the whole texture: `page_uv` and the page size
  passed to `PageImage::load` go.
- `render_tile` stays for tiles later.

## Tests

- hayro: a page filled black edge to edge, rendered at a scale where
  neither side is a whole number of pixels, is black in its last column
  and last row. Fails today through `render_tile` + `full_page`.
- hayro: the fixture's square lands in the same place at every scale,
  measured with each axis's own scale.
- render threads: a page's render and thumbnail come at the rounded
  size.
