# Text selection: fixes from the first check by hand

Two changes to step 1 of annotations
(`2026-11-04-text-selection-design.md`), agreed after the user tried it.

## Below the last line

Dragging straight down from the top of a paragraph and past its last
line left part or all of that line unselected:

- Starting top left, the end landed on the last line under the pointer,
  partway along it.
- Starting top right, the pointer was nearer the long line above than
  the end of a short last line, so the end went to the line above.

What it does instead, the way Preview and browsers do it:

- The line under a point is found by height first. Lines are grouped
  into columns, first by where they run across the page, then by height
  within the column.
- Below a column's last line, the caret goes to that line's end, at any
  x. Above a column's first line, it goes to that line's start.
- Between two lines of a column, the nearest line by height is used, and
  x places the caret along it as before.

Approaches considered:

- Weighting horizontal distance less in the nearest-line search. It's
  fragile: it depends on line height against how ragged the last line
  is.
- Columns by horizontal overlap. This one is picked: consecutive lines
  (in text order) whose spans overlap form a column.

Builder's call: how columns are told apart. A page's lines, in text
order, start a new column where a line doesn't overlap the one before
it horizontally. A point picks the column holding the line nearest to
it. A column's outline won't do: a full-width title joins the left column
and its outline then covers the right one too.

Text drawn wholly outside the page is left out of the page's text, for
selection, search and copy alike. On the user's page a printer's slug
above the top edge joined the paragraph's column, so a point above the
first line counted as between two lines and stayed placed by x. It is
never shown, so it isn't part of the page.

## Edge scrolling, the same in both directions

Full speed was reached 100 pt past the viewport's edge. In a maximised
window there's little room below the window, so scrolling down could
never get as fast as scrolling up.

What it does instead (option B):

- The speed rises over a band 60 pt inside the edge and reaches the base
  speed, 800 pt/s, at the edge. Past the edge it stays at the base speed,
  so screen room doesn't matter.
- Holding the pointer in the band doubles the speed every second, up to
  6400 pt/s. Leaving the band starts the time again.

Approaches considered:

- A, all inside the window with no speed-up: the same in both
  directions, but long jumps are slow.
- B, A plus a speed-up while held: picked by the user.

Builder's call: the values above (60 pt, 800 pt/s, doubling each
second, capped at 6400 pt/s) are a first guess, to be tuned by hand.
