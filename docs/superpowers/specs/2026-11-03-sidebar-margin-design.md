# Sidebar margin: sharp thumbnails just out of view are rendered ahead

Date: 2026-11-03. Agreed with the user.

## Intent and why

The sidebar renders sharp thumbnails only for the pages it shows
(`Sidebar::request`). On PageUp or PageDown past the visible thumbnails,
the sidebar jumps to show the new current page, whose thumbnail nothing
had asked for: it shows low-res every time, in single and dual mode. The
sidebar thumbnails spec left a margin out "until scrolling shows it's
needed"; it now does.

## What changes

1. **A margin of two thumbnails each side.** The sidebar's renderer is
   asked for the visible pages and the two pages before and after them,
   clamped to the document. Two covers the next page in single mode and
   the next spread in dual paged mode. Where one PageDown moves further
   (continuous mode with several pages on screen) the thumbnail it lands
   on is still low-res for a moment.
2. **Order.** Visible pages first, then the margin, nearest first; at
   equal distance the page after before the page before.
3. **Kept.** Renders of margin pages are taken in and kept like visible
   ones (`receive` no longer drops them); the tracker, the renderer's keep
   set and the sharp images all follow the visible range widened by the
   margin. Pages past the margin are dropped as before.
4. **Drawing is unchanged:** only visible thumbnails are drawn; a margin
   page that comes into view is drawn sharp at once, with no fade (its
   render was there before it was shown).

## Cost

A sharp thumbnail is about 1.2–1.4 MB at a typical sidebar width on a 2×
screen, up to about 4.3 MB at the widest (480 pt). Four more: about
5–6 MB usually, at most about 17 MB. Accepted.

## Tests

Harness, sidebar open with sharp thumbnails: the thumbnails just above
and below the visible ones are requested; after PageDown and after PageUp
that make the sidebar follow, the newly current page's thumbnail is drawn
sharp on the first frame it shows, in single and in dual paged mode.
The render order, clamped at the document's ends. A page
scrolled beyond the margin is dropped.
