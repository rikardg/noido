# Page gaps grow and shrink with the pages

Date: 2026-11-20. Agreed with the user. A small change: the gap between
pages follows the zoom instead of standing still, so the pages keep their
share of the space between them.

## Intent and why

The gap between rows in continuous mode, and the gutter between the two
pages of a spread, was `PAGE_GAP` = 12 UI points at every zoom. The view
opens at fit width, so a Letter page is usually shown larger than 100%: the
gap was 12 px against an 860 px page in a 900-wide window, and 12 px against
a 2448 px page in a wide one. Zoomed out it was a fifth of a 61 px page. The
gap is the desk between sheets, so it should keep its size against the
page.

`PAGE_GAP` becomes a gap in document points instead. The layout gives it
`scale` UI points per point, so at 100% it is 8 px — two thirds of today's
12, the size the user picked by eye at fit width — and it grows and shrinks
with the pages from there.

## Goals

- At 100% zoom the gap is 8 px, two thirds of what it was; at 50% it is 4,
  at 200% it is 16, at 400% (zoom's ceiling) it is 32. The gap is then
  always 1.31% of a Letter page's width.
- Both gaps follow: the gutter between the two pages of a spread in dual
  mode, and the gap between rows in continuous mode.
- Fit width, fit page, holding the zoom across a window resize, the zoom
  steps and fit window to the page all re-arrange as they do now; only the
  size of the gap changes.
- Fit width and fit page solve for the gap rather than taking a fixed 12
  points off the room, so a fitted spread still fills the room exactly.
- Gapless mode is unchanged: no gap in either direction.

## Non-goals

- The `MARGIN` around the column of pages stays 12 UI points. It is the
  desk around the pages inside the window, not document geometry, and it is
  zero in fullscreen.
- A numeric gap setting, or a per-document one.
- A gap that is a share of the page rather than a number of document
  points: see the approaches below.

## Approaches

- **The gap in document points (chosen).** `PAGE_GAP` times the scale, with
  the fit solving the scale in closed form. One constant, the same look at
  every zoom, and no reference page to choose.
- **A share of the page** (2% of its width or height). Makes the gap look
  the same in documents of different point sizes — a 365-point book page
  gets 2.2% of its width against Letter's 1.31%, and a 1500-point
  comic page 0.5% — but it needs a reference side, which is ambiguous for a
  gutter between pages of different widths and for a row whose height is
  unrelated to its width, and it changes the look at 100% for every page
  that is not Letter.
- **A share, clamped** to a floor and ceiling in UI points. The clamps
  reintroduce the zoom dependence the change is meant to remove, exactly
  where the gap is largest or smallest on screen.

The chosen rule is proportional within a document, not across documents:
that is what the user asked for, and `G` (gapless) covers comics, where a
number of document points is smallest against the page.

This supersedes the older specs' description of the gap as a number of UI
points taken off the room a fit has (`2026-10-25-view-modes-design.md`,
one page for a spread).

## Behaviour

- Layout: the gutter is `PAGE_GAP * scale` in dual mode and 0 in single
  mode, which has no pages side by side; the gap between rows is
  `PAGE_GAP * scale`. Content size, centring and hit testing follow from the
  page rectangles, so they need no change.
- Fit width in dual mode: `scale = (viewport width - 2 * MARGIN) /
  (widest spread + PAGE_GAP)`, which fills the room exactly. The fitted
  scale grows by about 1.3% where the gap used to take a fixed 12 points
  (0.4902 to 0.4968 for two Letter pages in a 636-wide viewport).
- Fit page: the smaller of that width solve and the height fit, as now; the
  height has no gap, because a paged spread stacks nothing.
- Fit window to page and fit window to width ask the spread's size at the
  scale it is held at, so the gutter they report is the one the view has.
- Zooming into a point that lies in a gap no longer drifts: a gap is 8
  document points at every zoom, so the point maps affinely and comes back
  to where it was. The existing test `anchor_in_gap_is_stable` tightens from
  a whole gap of tolerance to a hundredth of a point, and a new one does the
  same in a dual spread's gutter.

## Code shape

- `noido-core/layout.rs`: `PAGE_GAP`'s doc says document points at 100%;
  a private `gap()` gives `PAGE_GAP * scale`, 0 when gapless; `spread_gap`
  is `gap()` in dual mode and 0 otherwise; `row_gap` is `gap()`; a public
  `gap_points(spread)` gives the document-point constant the fit's maths
  needs (0 in single mode, 0 when gapless).
- `noido-core/view.rs`: `fit_scale_in` takes no gap off the room; the width
  ratio divides by `length + gap_points` and the height ratio by `length`.
- Nothing else: no state, settings, keymap, menu or state-file change.

## Testing

- Layout: a dual spread at 2× has a 16-point gutter, page 2 at
  `MARGIN + 612 * 2 + 2 * PAGE_GAP`; two rows at 0.5× are 4 points apart;
  gapless stays touching; `gap_points` is 0 in single mode.
- View: fit width in dual mode fills the room exactly (content width is the
  viewport's, the scale is `(w - 2 * MARGIN) / (spread + PAGE_GAP)`); fit
  page where the height binds is unchanged, and where the width binds it
  fills the room too; the spread's viewport at a held 2× carries a 16-point
  gutter; `anchor_in_gap_is_stable` and `anchor_in_a_gutter_is_stable` come
  back within a hundredth of a point.
- Harness: the gap drawn between two rows is `PAGE_GAP` times the view's
  scale, and keeps its share of the page across a zoom step that is checked
  to have happened.
