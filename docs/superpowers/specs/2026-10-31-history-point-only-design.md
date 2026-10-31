# History entries hold only the place

Date: 2026-10-31 · Status: draft for review

Supersedes the "an entry is a `ViewState`" rule in
[2026-10-27-history-design.md](2026-10-27-history-design.md).

## Why

The history design stored a whole `ViewState` per entry, so Back and Forward
restored zoom, scale, continuous, dual and odd-left as well as the place. In
use that reads wrong: change the zoom or the page arrangement at one place,
follow a link, and Back quietly reverts the change. Zoom and arrangement are
the document's view settings (per document, 2026-10-26 settings), not
properties of a place. They should stay where the reader put them; Back and
Forward should move the place only.

## Behaviour

- **A history entry is a `DocPoint`** (`page`, `x`, `y`) — the same anchor
  used by zoom, resize and the saved view. Nothing else.
- Jumps still add an entry (link, thumbnail, first search hit); scrolling,
  paging, zooming and arrangement changes still add none.
- **Back and Forward restore only the place**, keeping the document's
  current zoom and arrangement. In paged mode the shown spread follows the
  target page.
- A stored point is honoured the way the document's own view is: page
  clamped to the page count, x and y clamped to the page, non-finite values
  read as 0 — one shared helper, so the two restore paths can't drift.
  *(Decision: keep the clamp. A damaged `y` must not move the view to a page
  other than the one the entry names; the visible cost for a genuine point
  sitting in a page gap is at most about a page gap.)*
- The cap (100) and saving between runs are unchanged.
- **Old state files still load.** Entries written with the extra view fields
  deserialize as points, their unknown fields ignored; those fields are
  dropped, so the first Back after upgrading uses the current view. No
  `VERSION` bump.

## Non-goals

- Zoom and arrangement becoming application-global. They stay per document.
- The document's own saved view: still a full `ViewState`, unchanged.
- A visible history list or menu of entries.

## Approaches

- **A (chosen): point-only `History`.** `History` holds `Vec<DocPoint>`.
  `View` gains `place()` (the centre computation already in `state()`) and
  `restore_place()`, and `restore` becomes "set the view, then place the
  point". `noido-state` writes `page`/`x`/`y` per entry. The rules stay
  testable in core.
- B: keep `Vec<ViewState>` and ignore the extra fields when restoring.
  Rejected: entries would still carry dead fields, keeping the misleading
  shape in the file format for no gain.

## Left to the builder

- The exact `View` API, and whether `restore` delegates to `restore_place`.
- The TOML name for the point (a `Point` beside the existing `Place`).
- Whether `state()`'s centre computation moves into `place()`.

## Testing

- Core `History`: over `DocPoint` — entry on leave, back/forward round trip,
  forward dropped on a new jump, the cap, empty back/forward.
- Core `View`: `restore_place` keeps the current zoom and modes, switches the
  shown spread in paged mode, clamps a stale page and out-of-page x/y, and
  reads non-finite values.
- State: point history round-trips; an old entry with the view fields loads
  as its point; a file without history still loads.
- Harness: changing zoom or page mode, then Back, keeps the change; each
  jump kind still adds one entry.
