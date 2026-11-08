# The selection bar comes up where the mouse was released

Date: 2026-11-08. Agreed with the user. A small change: the bar with Copy
and the highlight colours appears by the mouse, however the selection was
made.

## Intent and why

A fresh selection hangs the bar from its later end in the document. Select
upwards and the bar appears at the far end, away from the mouse, so the
reader has to travel the length of the selection to reach it. Dragging a
highlight's start handle does the same. A clicked highlight already brings
its bar up at the click; this makes that the rule everywhere.

## Goals

The bar hangs from the page point where the mouse was released, and follows
that point when the view moves:

- a drag that selects, in either direction;
- a double or triple click, and a Shift+click;
- the release of a handle, on a selection or on a highlight;
- a click on a highlight, as today.

Clicking a colour swatch to make a highlight leaves the bar where it is.

## Non-goals

- Lining the bar up with the text line, or keeping it over the text when
  the mouse overshoots into the margin: it stays by the mouse.
- Any change to the bar's contents, or to how it picks above or below.

## Design

`select::State::bar_doc`, the page point the bar hangs from, is set when a
drag ends and ignored while nothing is selected. `selection_bar` uses
`bar_at` whenever there is one, no longer only while editing.

- **Release outside the view.** Letting go past the pages' edge after an
  edge scroll would leave the point out of view, which hides the bar. The
  release point is clamped into the view before it is kept. The bar is
  centred on that point, so sideways it can still reach past the view, over
  the sidebar, as a bar on a caret could.
- **An end on a page out of view.** Such a release can leave the selection's
  end waiting for the text of a page that is not shown, with no bar until
  the reader scrolled there. A waiting end now asks for its page's text.
- **No anchor.** With no page point to use, the bar falls back to the
  selection's later end, as it does today.

## Tests

In the headless harness:

- An upward drag brings the bar up at the release, not the later end.
- Dragging a highlight's start handle brings it up at the release.
- A double click and a Shift+click bring it up at the click.
- A release past the window's bottom edge still shows the bar, inside the
  view, once the next page's text has come.
- Panning leaves the bar on its page point.
- Making a highlight from a swatch does not move the bar.

`bar_hangs_from_the_click_on_a_highlight` asserted the old behaviour for
Shift+click and drag, and changes with it.

## By hand

Select text upwards, downwards, and across lines; double- and triple-click
a word; drag each handle of a selection and of a highlight; drag past the
window's edge so it scrolls, then let go. Each time the bar should come up
by the mouse.
