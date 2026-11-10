# Pins: the arrow opens the pagelet, the card goes there

Date: 2026-11-10 · Status: draft for review

A revamp of the pins and pagelet feature
([2026-11-09-pins-design.md](2026-11-09-pins-design.md)). It changes how
the pagelet is opened and where the pin sits; the pagelet itself, its
persistence, fading, resizing and links stay as they were.

## Intent

The preview used to open from a press on a pin card, which meant a pin
card did not go anywhere on a single click and a double click was needed
to navigate. The card and the preview are two different intentions, and
they deserve two different targets: the card is "take me there", and a
small arrow on the card is "show me it here".

With the arrow on every card, both tabs open the preview the same way,
and the Pins tab is no longer the only door.

## Goals

- Clicking a card in either tab navigates the main view to that page, as
  a Pages thumbnail always did. A card in Pins navigates too.
- An arrow on the thumbnail's right edge opens or closes the pagelet on
  that page, on both tabs. It does not move the document: the current
  page, zoom and scroll stay as they were.
- The arrow is drawn only while the pointer is over that thumbnail. The
  thumbnail's full right strip, one arrow icon wide plus a little
  padding, is its hit area.
- The arrow closes the pagelet when it already shows that page, and
  opens it on that page otherwise — at the pagelet's remembered place and
  size, as any other open.
- The pin chip moves from the thumbnail's upper-right corner to the
  centre of its top edge, on both tabs. Its press still pins or unpins
  and neither navigates nor opens the pagelet. Unpinning the page the
  pagelet shows still closes it.
- The arrow's chip carries a subtle translucent fill, so the arrow reads
  over a white page and a dark scan. The exact alpha is tuned by hand.
- The double-click machinery goes: `pin_clicked`, the pending press and
  its `max_double_click_delay` wait, and the `double_clicked` test of a
  card. A double click is simply two navigations.

## Non-goals

- An arrow on Outline rows, Search results, or the pagelet itself.
- A keyboard key or menu item for the arrow; pointer only, as before.
- Tooltips, an open/close indicator beyond the arrow's ink, or a separate
  close control on the card.
- A second pagelet, or the pagelet opening without a page to show.

## Anti-goals

- A card click that does not navigate, or an arrow that moves the
  document.
- An arrow whose hit area is not where it is drawn, or one that shows on
  a card the pointer is not over.
- A pin chip that overlaps the arrow strip or the card's clickable area
  enough to steal a press from either.
- Keeping the double-click wait alive after a click navigates: it would
  make every card press feel late for an outcome nothing uses.

## Approaches considered

**The strip as its own click widget** (picked): a `ui.interact` over the
right strip, drawn over the card like the pin chip, registered after the
card's own click widget. A click there sets `pagelet: Some(page)` and
clears the card's `clicked`. It is the pattern `toggle_pin` already
proves — overlapping widgets, later one wins — and the cursor and the
suppression are egui's job.

**Splitting the card's click by x** (dropped): one response for the whole
card, then a position test against the strip's rect to decide
navigate-or-preview, and a hand-set cursor. Less machinery, but the hit
test, the hover visibility and the cursor would each repeat the strip's
geometry, and the pin's press would still need its own widget anyway.

**A button in the card's layout** (dropped): `egui::Button` brings its
own frame, padding and focus handling, and the card is painted, not laid
out. The arrow is a painted shape (`paint_chevron` is the precedent),
so a button would be fought more than used.

## What this builds

**The card.** A click anywhere on the card except the pin and the strip
navigates, through the existing `clicked` path (`leave_here`,
`go_to_page`, `choose_page`, `reader_moved`). Both tabs emit it; the Pins
tab stops mapping its card click to `pin_clicked`.

**The arrow.** A chevron pointing right, centred in the strip,
vertically centred on the page image, over a square chip the strip's
width, not the full-height strip — the strip only takes the presses. `ARROW_SIZE` is the icon (near the
pin's 20 points); the strip is that wide plus `ARROW_PADDING` per side,
inset from the page's right edge, and the page image's full height. It is
painted only when `rect_contains_pointer` holds for the thumbnail, over a
`panel_fill` chip at about 0.85 (as the pin's), with weak text ink; when
the pagelet shows that page the ink is the selection colour, so the arrow
reads as "open here". The strip sets `PointingHand` while hovered. A
press on it does not navigate.

**The pin.** `pin_rect` centres horizontally on the thumbnail, one
`PIN_INSET` below its top.

**The app.** `SidebarView`'s output gains `pagelet: Option<usize>`. The
app toggles: if `pin_preview.page == Some(page)` it closes, else it opens
with the same `open_pagelet` (saved place, clamped to the window). The
pending press (`pending_pin`), its repaint scheduling, and the
double-click branches are deleted.

**State.** Unchanged: the pagelet's place, size and page are still saved
in `[pagelet]`. A pagelet closed by the arrow keeps its place for the
next open. An open pagelet reopens on its page whether or not that page
is still pinned: the arrow opens any page, so the pin no longer decides
what may come back.

## Tests

- A Pages card click navigates; a Pins card click navigates the same way.
- The arrow on a Pages card opens the pagelet on that page without
  changing the current page or the view's scroll.
- The arrow on a Pins card opens it the same way.
- A second press on the same page's arrow closes it; the place survives
  for a reopen.
- A press on the strip navigates nowhere.
- The arrow is drawn only while the pointer is over the thumbnail, and
  its ink is the selection colour while the pagelet is on that page.
- The pin's rect is centred on the thumbnail's top edge.
- A pin press still pins and opens nothing; unpinning the pagelet's page
  still closes it.

## Left to the builder

- The arrow's exact size, padding, chip alpha and glyph weight; the
  chevron's proportions against the pin's.
- Whether the strip's hover should tint the chip more than the idle
  arrow (the pin's hover does brighten its chip).
- The tests that read a pin click as opening the pagelet move to the
  arrow; the double-click tests go.
