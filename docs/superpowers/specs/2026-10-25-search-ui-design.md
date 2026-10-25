# Search

Date: 2026-10-25 · Status: draft for review

Part 2 of 2 of search. It builds on part 1,
`2026-10-25-text-layer-design.md`: the text thread (`TextSearcher`) that
extracts text and returns hits per page with highlight rectangles and a
context line, matching the way agreed there (case ignored, normalised,
accents ignored unless the query has some, across line breaks and
hyphenation, no whole-word option).

## Intent

Find words in the document: type in the title bar's search field, see every
match on the pages and a list of them in the sidebar, and step through them
from the keyboard. Searching should feel immediate and never move the page
under the reader unexpectedly.

## What exists

The title bar has a search field. Mod+F focuses it. Enter in it opens the
sidebar; Esc closes the sidebar again if Enter opened it; leaving the field
otherwise hands the sidebar back to the reader. Nothing is searched yet. The
sidebar shows page thumbnails.

## Behaviour (agreed)

- **Live search:** the search runs about 200 ms after the last keystroke.
  No minimum length for now; we'll see whether one is needed.
- **Extraction starts the first time the search field gets focus,** so
  opening a document stays as cheap as today.
- **The sidebar shows results while the search field has text,** and
  thumbnails otherwise. No separate switch until the sidebar's mode switcher
  (thumbnails, results, outline) is built. A search opens the sidebar if it
  is closed.
- **Results list:** one row per hit: the page number and a line of context
  with the match in bold. Clicking a row goes to that hit.
- **On the pages, every match is highlighted** for as long as the search
  lasts, also after the field loses focus.
- **The first hit:** when a search's first hit arrives, the page view goes
  there, once. After that it stays where it is while more results come in.
- **Results arrive page by page** while the text is extracted; a line like
  "Searching… page 212 of 600" shows until done. (Extraction measured
  0.15–0.35 s for 220–240 page books, so this is brief.)
- **Next and previous hit:**
  - Enter / Shift+Enter, while the search field has focus;
  - Mod+G / Mod+Shift+G and F3 / Shift+F3, everywhere.
- **"Next" starts from where the reader is.** If the reader has moved the
  page view since the last hit was shown (scrolling, a page key, zoom, a
  thumbnail click), next and previous go from the current view, not from the
  last hit. Example: on hit 3 on page 10, scroll to page 80, press Mod+G:
  it goes to the first hit at or after page 80's view, not to hit 4 on
  page 10.
- **Esc in the search field clears the search:** the text, the highlights
  and the results; the sidebar shows thumbnails again. A second Esc, with
  the field empty, does what Esc does today: closes the sidebar if search
  opened it, and leaves the field.

## Proposals (my calls)

- **The current hit** is highlighted more strongly than the others on the
  page, and its row in the results list is marked like the current page's
  thumbnail (the selection colour). The list scrolls to keep the current
  hit's row in view, the way the thumbnails follow the current page.
- **Counter:** "3 of 41" next to the search field (inside its right end, or
  just right of it), "No matches" when a finished search found none. While
  results still arrive the total may grow; it shows the hits found so far.
- **Wrapping:** next after the last hit goes to the first, and previous
  before the first goes to the last. Without a hit to go to, nothing
  happens.
- **Where "the reader is":** next goes to the first hit that starts at or
  below the top edge of the page view, previous to the last one that starts
  above the bottom edge. So with hits visible, next picks the first visible
  one and previous the last visible one. In paged mode the view's edges are
  those of the shown spread.
- **Going to a hit** keeps zoom and modes. If the hit's rectangles are
  already fully visible, the view doesn't move. Otherwise, in continuous
  mode it scrolls so the hit sits at the vertical centre of the view (and
  horizontally into view when zoomed in); in paged mode it shows the spread
  holding the hit, and scrolls within it when the page is larger than the
  view.
- **Searching from the reader:** each new query asks the text thread to
  start at the current page, so the first hit, the one jumped to, is the
  first one at or after the current page (wrapping to the start of the
  document if there is none after it).
- **The jump to the first hit is dropped if the reader moves the page view**
  (as defined for "next") before that hit arrives: a slow first hit, on a
  big OCR'd book say, mustn't yank the view away from where the reader went
  meanwhile.
- **The results list is in document order**, by page and position on the
  page, whatever order hits arrive in. Hits found later on earlier pages are
  inserted above, so while a search runs the current hit's number can change
  ("3 of 41" becomes "17 of 55"); that lasts well under a second on the
  books measured. The list draws only its visible rows, like the
  thumbnails, since a search can have thousands of hits. The current hit
  keeps its row in view as rows are inserted above it.
- **Next and previous during a search** use the hits known so far,
  wrapping among them; they don't wait for unsearched pages.
- **Cut-short searches:** when the text thread stops at its cap (5 000 hits,
  part 1), the counter shows "5000+" and next and previous wrap among the
  hits kept.
- **When a search opens the sidebar:** only when a search starts from an
  empty field (the first letter typed), and only if the sidebar is closed;
  then it counts as opened by search, for Esc. More letters, a reload's
  re-run, next and previous never reopen a sidebar the reader hid with F9.
- **Typing more letters** is a new query: it jumps once to its own first
  hit, which is usually the same place.
- **Mod+F with text in the field** focuses it and selects the text, so typing
  replaces it.
- **Highlight colours:** a translucent fill over each hit (no outline), from
  the theme's selection colour or a yellow, whichever reads better on white
  pages in both light and dark themes; the current hit more opaque.
- **The sidebar toggle (F9) while searching** hides and shows the sidebar as
  usual; the search and its highlights stay.
- **Reload:** the new document's text thread re-runs the current query;
  the page view doesn't jump, and the current hit becomes the first one at
  or after the current page.
- **Keys:** `KeyPress` gains a `shift` flag so Shift+F3 and Mod+Shift+G can
  be bound. This also fixes the backlog item where Shift+C / Shift+D toggle
  view modes; the item goes. New actions for next and previous hit.

## Non-goals

- Rendered previews of hits (backlog, ideas and thoughts).
- The sidebar's mode switcher.
- Search options (case, whole word, regular expressions).
- Highlights on the sidebar thumbnails.
- Search history, or remembering the query between runs.
- Text selection and copy.

## Testing

Headless harness tests in `app.rs` with a fake engine that has text:

- typing searches after the delay, not before; an empty field searches
  nothing;
- the first focus of the field starts extraction; opening the document
  doesn't;
- the sidebar opens and shows results while the field has text, thumbnails
  after it's cleared;
- the view goes to the first hit once; later results don't move it;
- Enter / Shift+Enter in the field, Mod+G / Mod+Shift+G and F3 / Shift+F3
  outside it, go to the next and previous hit, wrapping at the ends;
- after scrolling away, next goes to the first hit at or after the view;
- clicking a result goes to its hit;
- Esc clears the search; a second Esc closes a sidebar the search opened;
- highlights are drawn for every hit on visible pages, the current one
  differently;
- a reload re-runs the search without moving the view.

`View` gets a method that brings a rectangle on a page into view, with unit
tests in `noido-core` for continuous and paged modes, already-visible hits,
and hits larger than the view.

## Left to the builder

- Exact colours, the counter's placement and the results row layout,
  within the look of the sidebar thumbnails.
- Names of new types, actions and methods.
- How the results list shares the sidebar panel's code with the thumbnails.
