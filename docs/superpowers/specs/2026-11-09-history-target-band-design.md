# Keep a history target's whole band rendered

Date: 2026-11-09. Agreed with the user. A small change: the pages visible
beside the centre of a Back or Forward target stay cached, so returning to
a place is sharp in continuous mode.

## Intent and why

In continuous mode several pages can be on screen at once. A history entry
is the point at the viewport centre, and the keep set for the nearest Back
and Forward target covers only that point's page (or its spread). Jumping
away therefore evicts the other pages that were visible beside it. Going
Back or Forward then re-renders them over their thumbnails, and each
re-render is also a full-page texture upload — the stall in issue #43 on
macOS. Keeping the whole band the reader saw at each target makes a return
sharp at once.

## Goals

- The pages a restored history target would show are kept: the band around
  its centre in continuous mode, its spread in paged mode.
- Keep only: those pages are retained if already rendered, never requested
  ahead. A jump asks for the target's own spread, as today, so the place
  being jumped to is not delayed by rendering a band that is not visible.
- No change to `History`, the persisted state format, or the page cache's
  existing windows.

## Non-goals

- The first visit to a far place: nothing there is cached yet.
- More than one step back or forward; `HISTORY_KEEP` stays 1.
- Scrolling back beyond the existing `KEEP` window.
- The neighbouring search hits' bands.
- A byte budget for the sharp cache (a separate question raised by the
  assessment; this change stays inside today's envelope).

## Design

`noido-core`: `View::band_around(&self, point: &DocPoint) -> Range<usize>`,
the pages a view restored to `point` would show:

- Clamp `point` with the same helper `restore_place` uses, so a point in a
  page gap lands where the restore lands.
- In a continuous layout, `content_point` gives the content `y` the restore
  centres; `visible_pages(y - height/2, y + height/2)` is then exactly the
  restored view's visible range, computed by the same function `pages()`
  uses.
- Where the point's page has no place (a paged layout showing another
  spread), the answer is the point's spread, which is all a restore to it
  shows.

`gui-egui`: in `pages()`, `extra` keeps being built from `ahead` as today
and is then extended with each nearest back and forward target's
`band_around`, skipping pages already in `window` or `extra` and skipping
targets past the page count. `ahead` — and so `order` — keeps only the
target's spread as today, so nothing new is rendered.

## Tests

- Core `View`: a band centred in a page matches the pages visible at the
  viewport placed there; at the top of the document it is clamped; a point
  in a page gap gives the band the clamped restore would show; in paged
  mode it is the target's spread.
- Harness: continuous mode zoomed out so three pages are on screen; jump
  far with a link; the pages of the band left behind are still in
  `textures`; Back lands there without re-rendering them.
- The existing history, window and paged-mode tests are unchanged.

## By hand

Open a long PDF, continuous, zoom out until three pages are visible, click
a far thumbnail or link, then Back: the pages either side of the centre
should show sharp, with no thumbnail flash.

## Left to the builder

The exact `View` method name; whether a point in a gap needs any slack
beyond the clamp; whether the band should also join the request order (no,
per the keep-only goal).
