# Screen edge flash

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

When a page is taller than the window, PageUp/PageDown jumps 90% of a
screen and it is hard to find where to go on reading. A brief line marks
where the previous view's edge now is: its bottom edge after PageDown, its
top edge after PageUp. A try-out; it may be tuned or dropped.

## Approaches (agreed: 1)

1. **`View` records the old edge.** When PageUp/PageDown scrolls (in
   continuous mode, or within a spread in paged mode), `View` keeps the old
   edge as a `DocPoint`; the frontend takes it after the action and flashes
   it. `View` already decides scroll versus flip, and is unit-tested.
   Chosen.
2. The frontend compares offsets before and after: no core change, but it
   must work out after the fact whether a flip happened; harder to test.

## Behaviour

- PageDown/PageUp that scrolls: a line at the old bottom/top edge.
- A flip to another spread in paged mode, or a key press that moves
  nothing (at the document's ends): no line.
- The line runs across the page(s) of the row it falls on, at the old
  edge's height. In a gap between rows (continuous mode) it is drawn in the
  gap, across the pages of the row above (my call: still visible there).
- Looks like the link-target flash: hyperlink colour, its stroke width,
  fading out over the same 1 s (`FLASH_SECONDS`). A new edge replaces one
  showing. Kept as a page point, so a zoom during the fade keeps it on the
  same text.

## Code shape

- `noido-core/view.rs`: a `screen_edge: Option<DocPoint>` set by the
  screen-scroll actions when the offset moved; `take_screen_edge()`.
- `gui-egui/app.rs`: after `view.apply`, take the edge and start its flash;
  draw it beside the link flash, one segment per page of the row.
