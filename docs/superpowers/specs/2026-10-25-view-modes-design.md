# Page view modes

Date: 2026-10-25 · Status: draft for review

## Intent

Let the reader choose how pages are arranged: one page or two side by side,
in a continuous strip or one page (spread) at a time. Built together because
zoom, fit modes, PageUp/PageDown and anchoring each need a rule per mode.

## Behaviour (agreed)

Two independent toggles, four combinations:

- **Continuous / paged** (`c` toggles). Continuous: all pages in one scrolling
  strip (today's behaviour). Paged: only the current page or spread is shown.
- **Single / dual** (`d` toggles). Dual shows spreads of two pages side by
  side: pages 1–2, 3–4, …

Paged mode navigation (single and dual alike; a spread behaves like a page):

- PageDown scrolls one screenful within the page. If less than a screenful
  is left it scrolls to the bottom edge. A press when already at the bottom
  shows the next page from its top.
- PageUp mirrors this: at the top, a press shows the previous page from its
  bottom.
- The wheel and drag scroll within the current page. Exception (amended
  after trying it): when the whole page (spread) fits the window vertically,
  vertical wheel input flips instead — one flip per mouse wheel step, one per
  125 points of trackpad scrolling; trackpad momentum, Mod (zoom) and sideways
  scrolling (horizontal movement or Shift) never flip. Drag never flips.
- When the page fits the window (e.g. after `f`), every PageDown is a flip.

Continuous mode keeps today's behaviour (PageUp/PageDown scroll a screenful).

## Proposals accepted in conversation (my calls)

- After a flip: same horizontal position and zoom; in fit modes the new
  spread is refitted (fit page can change the zoom if spreads differ in size).
- Fit width and fit page measure the spread in dual mode (both pages and the
  gap between them). In paged mode `f` therefore shows exactly one whole page
  or spread; in continuous mode it only sets the zoom.
- An odd last page in dual mode takes the left slot of its spread.
- Switching any mode keeps the page at the window centre: into paged mode, its
  page (spread) is shown from the top; into continuous mode, the centre point
  stays in place.
- Default on start: continuous, single (as today).
- Zoom limits, step and anchoring are unchanged.
- No "cover page alone" option yet (pages 1, 2–3, … for books); later.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. Layout of rows (one page or a spread each); continuous stacks all rows, paged lays out only the current row | One model for all four modes; scroll area and wheel stop at the page edge naturally; zoom, fit and anchoring carry over | **Chosen** |
| B. Always lay out the whole document; clamp scrolling to the current spread in paged mode | Scrollbar still shows the whole document; other pages must be hidden from drawing and rendering; special cases everywhere | Rejected |
| C. Separate continuous and paged layout types behind a trait | Separate code per mode, but duplicates spread building, centring, conversions; View must handle both | Rejected |

## Design

### Layout (`noido-core`)

- Pages are grouped into **spreads**: in single mode one page each, in dual
  mode pages `2i` and `2i+1`. `spread_of(page)` and `spread_pages(spread)`
  give the mapping.
- A **row** is one spread: its pages side by side with `PAGE_GAP` between
  them, vertically centred within the row's height (the taller page). The
  row is centred in the content width. An odd last page in dual mode sits in
  the left slot (the row is laid out as if a right page of the same width
  followed).
- `LayoutMode { dual: bool, paged: Option<usize> }`: `paged` is the spread
  shown in paged mode; `None` means continuous (all rows stacked with
  `PAGE_GAP` between rows and `MARGIN` around, as today).
- `arrange(scale, available_width)` lays out only the shown rows. Content
  width is `max(available_width, widest_shown_row * scale + gaps + 2*MARGIN)`.
- `page_rect(page)` is `None` for pages not shown. `visible_pages(top, bottom)`
  returns the range of shown pages in rows intersecting the span.
- `doc_point` finds the row at `y` (last row whose top ≤ y), then the page in
  that row at `x` (last page whose left ≤ x, else the first). Points outside a
  page stay relative to it, as today.
- Fit helpers take the rows into account: the widest row (sum of page widths;
  the gap is in UI points and comes off the available width) and the row's
  height.

### View (`noido-core`)

- New state: `dual: bool`, `continuous: bool`, `spread: usize` (current
  spread in paged mode).
- New actions `ToggleContinuous`, `ToggleDual`, bound to plain `c` and `d`.
- Toggle rules: find the page at the window centre; switch; in paged mode set
  `spread` to that page's spread and show it from the top (x kept, clamped);
  in continuous mode keep the centre point in place. Fit modes refit.
- PageDown/PageUp in paged mode: if the offset is within 0.5 of the bottom
  (top), flip to the next (previous) spread if there is one and show it at
  its top (bottom); otherwise scroll a screenful, clamped. Continuous mode
  unchanged.
- Fit page fits the spread at the window centre (continuous) or the current
  spread (paged). Fit width fits the widest row.

### Frontend

- Map egui `C` and `D` to the new keys.
- Pages not shown have no rect and are not drawn. Prefetch stays as now:
  pages around the visible range are rendered ahead, which in paged mode
  means the neighbouring pages are ready for a flip.

## Non-goals

Cover-page option, right-to-left spreads, page number indicator, go-to-page,
animated flips, remembering the mode between runs.

## Testing

Unit tests in `noido-core` for spreads, row layout and centring, the odd last
page, doc point lookup in a row, fit scales for spreads, toggles keeping the
centre page, paged PageDown/PageUp including partial fits and flips at both
ends of the document, wheel scrolling clamped to the page in paged mode, and
zero pages. Headless app tests for `c`, `d` and a paged flip.

## Left to the builder

Internal representation of rows; exact helper names; how `View` stores the
anchor during a toggle.
