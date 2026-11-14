# Page rotation

Date: 2026-11-14 · Status: draft for review

## Intent

Some documents mix portrait and landscape pages. One landscape page is
enough to spoil fit-to-width for the whole document: the zoom fits the
widest spread, so every portrait page comes out small. Other documents
simply have pages that are sideways and need turning to be read.

The reader turns a single page in 90° steps, from a right-click menu on
the page or its thumbnail. The PDF file is never changed: the rotation is
the reader's, saved in the document's state next to the highlights and
pins.

Rotation is the first *page transform*. Scaling and cropping may follow,
and a page could carry several at once. How several combine needs more
thought and is not designed here; this design only has to leave room for
it.

## Goals

- A right-click menu on a page in the main view, on a thumbnail in the
  Pages tab, and on a card in the Pins tab. It has four entries, `0°`,
  `90°`, `180°`, `270°`, with a check by the page's current rotation.
- The entries are absolute, clockwise from the page as it shows without
  noido's rotation (that is, with the PDF's own `/Rotate` applied). `90°`
  twice is still `90°`; `180°` is its own entry. `0°` undoes a rotation.
- The page under the pointer is the one rotated; in a two-page spread,
  only that page turns.
- A rotated page is, for the rest of noido, a page of that shape: the
  layout, fit-to-width, fit-to-page, spreads, the thumbnail and its
  column, the pin card, the pagelet, selection, highlights, search hits,
  links and link targets, and the outline's targets all follow it.
- Rotating never moves the view to another place. The view keeps the
  anchor it has (the page and point it is on), whether the rotation came
  from the main view, a thumbnail or a pin; the turned page's new height
  only moves the pages after it. The zoom mode in force (fit-width,
  fit-page or a fixed zoom) is applied again to the new shapes. Rotating
  from the sidebar does not navigate to the page.
- Text that runs vertically on screen (a landscape page turned to
  portrait, or a page the PDF itself rotates) selects, highlights and
  shows search hits as well as horizontal text does.
- Rotations are saved per document, with the rest of its state, and
  restored when it is opened again.

## Non-goals

- Rotating several pages at once ("all landscape pages", a range, the
  whole document).
- Keyboard shortcuts or a menu-bar entry for rotation.
- Scaling and cropping, and how several transforms on one page combine.
- Undo for rotation. It is absolute and one click resets it, so it stays
  out of the highlights' undo history.
- Changing the PDF file.

## Anti-goals

- A rotated page whose text layer is out of step with its image: a
  selection, highlight, search hit or link area drawn in the wrong place,
  or a click landing on the wrong word or link.
- Highlights or pins lost or moved by a rotation.
- A rotation that makes the view jump to another page.

## Approaches

**A. Rotate where pages come into noido (chosen).** The engine already
applies the PDF's own rotation and hands out page sizes, renders, glyphs,
links and destinations in the page's displayed frame. noido's rotation is
applied at that same boundary, so everything above it (core layout, view,
selection, hits, links, the egui frontend) sees an ordinary page of the
new shape. The one exception is the text layer's picking and drawing,
which assume horizontal text (see "Text in any direction"). Cost: changing a rotation re-renders
the page and re-reads its text and links, a moment's wait like a zoom
step. Scaling and cropping later belong in the same place.

**B. Rotate in the window.** The engine stays as it is, and the frontend
turns the rendered images as it draws them. Turning is instant. But every
mapping between screen and page (selection, highlight and hit drawing,
link hit-testing, the pagelet, thumbnails) has to apply the transform: a
dozen places, each a chance of the anti-goal above, and cropping later
would add clipping to all of them. Rejected for that.

Writing a rotated copy of the PDF was ruled out: the file stays as it is.

The user picked A.

## Design

### The transform at the engine boundary

A per-page transform record in `noido-core` (a `PageTransform`, with
`rotation: Rotation` as its only field for now, `Rotation` being one of
the four quarter turns), and a document-wide map from page index to
record (`PageTransforms`). Pages with no record, or a `0°` record, are as
the engine gives them.

The transform is applied by a wrapper in `noido-render` around the
engine's `Document`, so it is written once and holds for any engine:

- `page_size`: width and height swapped for 90° and 270°.
- `render_tile` / `render_page`: the wrapper maps the requested pixel
  rectangle back to the unrotated page, has the engine render that, and
  turns the pixels a quarter turn at a time. A quarter turn of pixels is
  exact and cheap, so the engine trait does not change. *(Builder's
  call, see below; the conversation spoke of folding the turn into
  hayro's own render transform, which also works but changes the trait.)*
- `text`: each glyph's quad (and anything else positional on `Glyph`)
  mapped through the rotation.
- `links`: link rectangles mapped; internal link destinations mapped too,
  since they may point at a rotated page.
- `named_destination`, `outline`: destinations mapped by the transform of
  the page they point at.

Mapping a point for a turn of `r` on a page of unrotated size `w × h`
(origin top-left, y down): 90° gives `(h − y, x)`, 180° gives
`(w − x, h − y)`, 270° gives `(y, w − x)`.

Each thread that opens its own copy of the document (render, thumbnails,
text, links, and the properties reader if it reads geometry) sees the same
transforms. When a page's rotation changes, each of them is told, and each
drops whatever it holds for that page: rendered tiles and thumbnails,
the page's extracted text, its links.

### Text in any direction (first step)

`noido-text` builds lines along each glyph's baseline, so character
indices, and with them highlights and selections (`Caret`), are the same
whatever the direction. But picking and drawing assume lines run left to
right: `caret_at` and `char_at` pick a glyph by x within a line,
`line_for` and `columns` order lines by y, `rects` takes a glyph's
x-extent with the line's full height, and `caret_rect` is a vertical bar.
On vertical text a click lands on the wrong character and a partial
highlight is drawn across the whole line. Pages with the PDF's own
`/Rotate 90` already have this problem.

Fixed first, on its own, before rotation: picking and rectangles follow
each line's writing direction. Along the line, the position along the
baseline picks the glyph; across it, lines are ordered by their offset
across the baseline; a run's rectangle spans its glyphs along the line
and the line's full extent across it; a caret is a bar across the line.
Upright and upside-down lines (0° and 180°) count, as do the two
vertical ones. This step fixes `/Rotate` pages too, and is tested there
(the `text.pdf` fixture's second page) before any of the rotation work.
How a direction is found and kept per line is the builder's call.

### Core

- `PageTransforms` is part of the document's state alongside `Pins` and
  `Highlights`.
- Setting a page's rotation updates the layout's size for that page and
  re-arranges, holding the view's current anchor (`rescale` /
  `pairing_anchor` in `view.rs` already anchor zoom changes) and
  re-applying the current zoom mode. It does not navigate.
- `DocPoint`s already held for the rotated page (history entries, the
  view's anchor, the pagelet's page) are in the old frame. Those that are
  points on that page are mapped through the change of rotation, so they
  keep pointing at the same spot on the content. *(Builder's call.)*
- Search hits on that page carry rectangles in the old frame; they are
  replaced once the page's text is re-read, or mapped through the change.
  Either way they are drawn in the right place. Selections and
  highlights are character positions (`Caret`) and need nothing.

### The menu

noido has no context menu yet; this is the first. In egui, a secondary
click on the page area, a Pages thumbnail or a Pins card opens a menu
(`Response::context_menu`) with the four entries, a check by the current
one. Right-click is unused today, so nothing conflicts. Choosing an
entry rotates that page; choosing the current one does nothing.

On the main view, the page is the one under the pointer when the menu
opened, not the current page.

### Saving

The state file gets a per-page table, keyed by page index the way pins
are, holding only pages with a transform other than identity, e.g.

```toml
[[pages]]
page = 3
rotation = 90
```

`rotation` is in degrees, one of 90, 180, 270; anything else is ignored
on load. Records past the document's last page (a file edited in place
that lost pages) are dropped on load. A file without the table, written
before this feature, loads as no transforms. Further transforms become
further fields of the same record.

## Testing

TDD throughout, at the lowest layer that shows the behaviour:

- `noido-text`: caret, word and rectangles on vertical and upside-down
  lines, from synthetic glyphs and from the `/Rotate` fixture page.

- `noido-core`: the point mapping for each turn (and round trips); the
  layout and fit modes with a rotated page (a landscape page turned to
  portrait no longer sets the fit-width zoom); the view holding its
  anchor when the page in view, a page before it, or a page after it is
  turned.
- `noido-render`: the wrapper's sizes, rendered pixels (a fixture with an
  asymmetric mark, checked in each orientation), glyph quads, link rects
  and destinations, against `fixtures/`. A fixture with a landscape page
  if none exists.
- `noido-state`: round trip of the table; unknown angles and out-of-range
  pages dropped; old files load.
- `gui-egui` harness: secondary click on a page, a thumbnail and a pin
  opens the menu; picking an entry rotates the right page (the one under
  the pointer in a spread); the check mark follows; text selection and a
  link click on a rotated page land where the image shows them.

## Left to the builder

- Destinations that give only one coordinate, and right-clicks in the
  gaps between pages (no menu, or the nearest page).
- How the transforms reach each render thread (a shared, versioned map,
  or a message to each) and how the frontend knows a page's new size.
- Whether pixel turning or a render transform in the engine is used; the
  trait stays as it is unless there is a reason to change it.
- The exact look of the menu, within egui's ordinary menus.
