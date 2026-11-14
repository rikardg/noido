# Scale to fit

Date: 2026-11-14 · Status: draft for review

## Intent and why

Some documents have pages of different sizes. Fit-width and fit-page
size the zoom against the widest spread, so one oversized page (a
foldout, a plate, a page scanned at another size) makes every ordinary
page come out small, or, at a fixed zoom, sends the reader into panning
across the large page instead of paging on.

Scale to fit is a per-document toggle that shrinks every page larger
than the document's typical page down to that size. The PDF file is not
changed; like rotation, it is a page transform of the reader's.

## Goals

- An on/off entry, "Scale to fit", in the global menu, with a check
  while it is on. It sits with the layout toggles (Continuous, Dual
  page, …).
- While it is on, the document has a *reference box*: the median page
  width and, taken on its own, the median page height. Sizes are in PDF
  points, as the pages show before scaling: the PDF's own `/Rotate` and
  noido's rotation applied. On an even page count each median is the
  larger of the two middle values, so the box is made of sizes that
  occur in the document and a document of two shapes in equal numbers
  is left alone.
- Each page is scaled by `min(1, box_w / w, box_h / h)`: pages only
  shrink, keep their aspect ratio, and none is wider or taller than the
  box. Pages already within the box are unchanged.
- A scaled page is, for the rest of noido, a page of its new size: the
  layout, both fit modes, spreads (each page scaled on its own before
  pairing), thumbnails, pin cards, the pagelet, selection, highlights,
  search hits, links and link targets, and the outline's targets.
- Turning it on or off never moves the view to another place: the view
  keeps its anchor and the zoom mode in force is applied again, as when
  a page is rotated.
- Rotating a page while it is on recomputes the box and the scales,
  since the page's shape counts toward the medians.
- The toggle is saved per document and restored when it is opened
  again. It is off for a document opened for the first time.

Expected, and accepted: a landscape page in an otherwise portrait
document shrinks until its width fits the portrait width, so it comes
out small.

## Non-goals

- Enlarging pages smaller than the box.
- A global default, a keyboard shortcut, or choosing the reference size
  by hand.
- Scaling single pages or ranges.

## Anti-goals

- A scaled page whose text layer is out of step with its image (a
  selection, highlight, search hit or link area drawn in the wrong
  place).
- Highlights or pins lost or moved by the toggle.
- A toggle that makes the view jump to another page.

## Design

Scaling is applied where rotation is: in the `Transformed` wrapper in
`noido-render`, so everything above it sees ordinary pages of the
scaled size, as the rotation design planned for scaling. A uniform
scale is simpler than a turn: `page_size` multiplies by the page's
factor; `render_tile` asks the engine for the tile at `scale × factor`;
`render_page` is unchanged (it is given pixels); glyph quads and
origins, link areas and destinations multiply by the factor. Rotation
is applied first, then the scale.

The factors are derived, not stored. `PageTransform` gains a `scale`
(1 by default, part of `is_identity`), set for every page whenever the
toggle, or a rotation while it is on, changes; the box is computed in
`noido-core` from the rotated, unscaled sizes (a pure function, tested
there). What is saved is the toggle, a `scale_to_fit` flag kept with the
document's page transforms (`PageTransforms`, which the frontend
already owns and shares with the threads) and written as a top-level
`scale_to_fit = true` in the document's state file; a file without it
loads as off. It is not part of `ViewState`, so it never becomes a
default for new documents. The per-page table in the state file stays
rotation-only.

A change of the toggle is handled as a rotation change is, but for
every page whose factor changed: the layout gets the new sizes, the
view holds its anchor and refits, `DocPoint`s held for those pages (the
anchor, history entries, the pagelet's page, search hits) are mapped
through the change of factor, and the threads drop their tiles,
thumbnails, text and links for those pages.

## Testing

TDD, at the lowest layer that shows it:

- `noido-core`: the reference box (odd and even counts, the
  two-shapes-in-equal-numbers case, one oversized page, rotated pages
  counting by their turned shape) and the per-page factors (only
  shrinking, aspect kept, the landscape page in a portrait document).
- `noido-render`: the wrapper's size, glyphs, links and destinations
  for a scaled page, and for a page both rotated and scaled.
- `noido-state`: the toggle round-trips; an old file loads as off.
- Harness: with a fixture of mixed page sizes, the menu entry toggles
  the check; fit-width after turning it on gives the ordinary pages the
  window's width; the view stays on its page across the toggle; a
  highlight on an oversized page draws on the same words before and
  after.

A fixture with one oversized page is added to `make_fixtures.py` if
none of the existing ones has mixed sizes.

## Builder's calls

- The menu label and the entry's exact place in the layout section.
- Whether a factor of exactly 1 is stored or left out.
