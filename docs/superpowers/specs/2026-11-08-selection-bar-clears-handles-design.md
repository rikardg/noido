# The selection bar keeps off the handles

Date: 2026-11-08. Agreed with the user. A small change, following
`2026-11-08-selection-bar-at-release-design.md`: the bar no longer covers a
selection's handles or lands under the mouse, and a selection just made has
it outside the selection when it can.

## Intent and why

The bar hung above the mouse, and takes any press on it. A handle under it
looks unavailable and can't be grabbed. On a selection a few lines long this
is common, not only after overshooting into blank space. Trying it, the user
also came to prefer the bar outside a selection just made: it leaves the text
in view, and the mouse is at one of the ends anyway.

## Goals

**A selection just made** (a drag, a double or triple click, a Shift+click, a
handle let go) has the bar outside it, at the end the mouse let go of: below the
last knob, or above the first line. On one line it goes above, unless let go in
the blank past the line and its knob. If that doesn't fit in the view, it follows
the rule below. Which end is remembered, not read from where the mouse is: in
tight lines, or across columns, the pixels can't tell.

**A click on a highlight** keeps the bar by the mouse, as it was: the click can
be anywhere in a long highlight, with both ends far off. No distance is
measured to decide between these; which one it is, is what made the anchor.

**By the mouse** the bar goes above it, or below if there is no room above, and
stays there if it covers no knob, even in a view too short to hold it. If it
would cover a knob, the bar takes the first of these that fits in the view
vertically and covers no knob:

1. Outside the selection, at the end nearer the mouse.
2. Moved just far enough to clear the covered knob: up off the end handle, down
   off the start handle.
3. Outside the selection, at its other end, for a short selection near the edge
   of the view, where step 2 lands on the other handle.

If none fits, it stays by the mouse. If both knobs are covered, the "covered
handle" is the one nearer the mouse.

**The bar never lands under the mouse**, where a click would hit a swatch
instead of clearing the selection. Let go in blank space past the selection, it
goes past the mouse as well.

## Non-goals

- Moving the bar sideways: it stays centred on the mouse, or as near as egui's
  keeping it inside the window allows.
- Moving the bar below a clicked highlight when the click is within a few lines
  of its end: the same highlight would behave differently line to line.
- Keeping the bar off the selected text, or off a handle's stem: only the knob
  is pressed.
- Changing which handle a press takes, or giving handles priority over the bar.

## Design

A pure `select::bar_place` picks the position from the anchor, both ends' caret
lines, the view, the window, the bar's size, and the end to go outside, if any.
Coverage is worked out from the caret lines, which are known for a handle out
of view, not from the handles shown, and from where egui will really draw the
bar, pushed inside the window. The bar's size is the one measured when it was
last drawn, with an estimate before the first time. `State::bar_doc` remembers
which end the mouse let go of, or that a click on a highlight made it.

## Tests

Unit, on `bar_place`: a selection just made goes to the end let go of, in tight
lines and with the end above the start too; one line keeps it above; past a
mouse beyond the selection; no room outside keeps it by the mouse. By the mouse:
nothing covered leaves it alone; each handle covered; both covered, clearing the
one nearer the mouse; no room outside, so moved clear, each way; never moved
clear onto the mouse; a short selection at the view's bottom and top goes to the
other end; nothing fits; pushed in from either window edge; a knob centred out
of view; the end's own caret as the anchor.

In the headless harness: each way of making a selection, and each handle let go,
over two lines; a drag down when zoomed out; a click on a two-line highlight
stays by the mouse, and moves off a handle it would cover, even one scrolled out
of view; a handle let go where it changes nothing leaves the bar; a drag let go
in blank space below the text; the bar's measured size is the one it is placed
with.

## By hand

Drag up and down over a few lines, and double-click a word: the bar sits
outside the selection. Click a long highlight, from each line: the bar stays by
the click, off the handles. Overshoot below the text, then click where you let
go: the selection clears, no highlight. Repeat near the window's left and right
edges, and with a selection filling the view.
