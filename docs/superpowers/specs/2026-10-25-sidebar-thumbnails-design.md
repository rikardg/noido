# Page thumbnails in the sidebar

Date: 2026-10-25 · Status: draft for review

## Intent

The sidebar is empty today. Fill it with page thumbnails the way macOS
Preview does: one column, each thumbnail scaled to the sidebar's width, so
widening the sidebar makes the pages bigger rather than adding columns. The
reader sees where they are in the document and can jump by clicking.

Later the sidebar will also hold search results and the outline, with a way
to switch between them. That switch is not part of this work; until it
exists, the sidebar shows thumbnails.

## Behaviour (agreed)

- **One column of thumbnails**, each as wide as the sidebar allows; the height
  follows the page. Resizing the sidebar resizes the thumbnails.
- **Always one thumbnail per page**, never spreads, whatever the page view's
  dual mode.
- **A page number under each thumbnail.**
- **The current page is highlighted.**
- **Clicking a thumbnail goes to that page.**
- **The sidebar follows the reader:** as the current page changes, the sidebar
  scrolls to keep it in view, unless the reader is scrolling the sidebar.
- **Sharp images only where visible.** Thumbnails in the sidebar's visible
  area get renders at the sidebar's width, from a renderer of their own (a
  second `Renderer`: own thread, own parsed copy of the document). While those
  are on their way, for example when scrolling fast, the existing 120 px
  low-res thumbnails stand in. A margin of sharp renders beyond the visible
  area is not part of this work; add it only if scrolling shows it's needed.
- **The sidebar opens with thumbnails;** the placeholder text goes.
- **A clicked page stays the current page** until the reader moves the page
  view themselves: scrolling, zooming, a page key or a mode change. Resizing
  the window or the sidebar doesn't count. Otherwise clicking page 5 could highlight page 6:
  the page at the view's centre is a later one when pages are shorter than
  half the view, and the last pages can't scroll to the top. This current
  page drives the highlight, the sidebar's following and the thumbnailer's
  focus; the saved reading position stays the page view's centre.

Memory, measured on the test files: a parsed copy of a document is 1–3 MB
(the file's bytes are shared, not copied); rendering adds working memory on
the order of 10–25 MB per thread at its peak. A sharp thumbnail at 260 pt on a
2× screen is about 1.4 MB, so a visible column is about 6–8 MB. Accepted.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. A second `Renderer` for the sidebar | Reuses the queue, keep range and priorities; neither view waits for the other; one more parsed copy (1–3 MB) and one more thread competing for CPU | **Chosen** |
| B. Share the page view's `Renderer` | No extra thread; but the queue holds one request per page, so the two views would overwrite each other's requests without a tag, and sidebar renders wait behind the page view's prefetch | Rejected |
| C. Let the `Thumbnailer` also render sharp ones | One thread for all thumbnails; but sharp renders would hold up the low-res fill that fast scrolling relies on, and splitting priorities rebuilds what `Renderer` already does | Rejected |

## Proposals (my calls)

- **Size and spacing:** thumbnails are the sidebar's inner width minus a
  12 pt margin each side; 12 pt between a thumbnail's number and the next
  thumbnail. Pages of different sizes all get the same width. A thin outline
  or shadow around each page so white pages stand out from the sidebar.
- **Highlight:** the current page is the clicked page (above), else the page
  at the page view's centre (the same page the thumbnailer focuses on today).
  In dual mode both pages of the
  current spread are highlighted, since both are on screen. The highlight is
  a rounded rectangle in the selection colour behind the thumbnail, and the
  number drawn in that colour too, as in Preview.
- **Clicking** goes to the page without changing zoom or modes: in continuous
  mode the page's top lands at the top of the page view; in paged mode the
  spread holding the page is shown. A new `View` method does this; nothing in
  `View` jumps to a page today.
- **Following:** when the current page changes, or the sidebar opens, the
  sidebar scrolls the least distance that brings the current page fully into
  view (centred if it doesn't fit). It doesn't move otherwise, so a reader
  who scrolled the sidebar away keeps their place there until the current
  page next changes. A click in the sidebar changes the current page but
  doesn't scroll the sidebar, since the page is already in view.
- **The low-res window follows whichever moved last.** The thumbnailer keeps
  the 100 pages nearest its focus. Its focus is the current page, or the
  page at the middle of the sidebar after the reader scrolls the sidebar,
  until the current page next changes. Otherwise scrolling the sidebar more
  than 100 pages away would show white.
- **Sharp render scale:** the thumbnail's width in points times
  `pixels_per_point`, through `render_scale` with the same size cap as the
  page view. During a resize the existing sharp image is stretched until the
  new one arrives, as the page view does on zoom.
- **What is kept:** sharp thumbnails only for visible pages; a page scrolled
  out of view drops its sharp image (the low-res one stays, within its
  window). Closing the sidebar drops all sharp images and stops requests.
- **Drawing priority per thumbnail:** its sharp render, possibly at an older
  scale; else the 120 px low-res thumbnail; else white. A full render from
  the page view is not used: drawing it much smaller without mipmaps aliases.
- **Failed renders:** a page whose sharp render fails falls back to its
  low-res thumbnail; errors are reported as for the page view.
- **Startup:** the sidebar renderer starts with the others at launch, even
  with the sidebar closed; an idle one costs only its parsed copy. It starts
  without blocking, like the thumbnailer (the page sizes are known by then),
  so neither startup nor a reload waits for a second parse.
- **Reload:** a new version brings a new sidebar renderer along with the
  page view's renderer and thumbnailer; the sidebar keeps its images until
  new ones arrive, and drops those past the new page count, as the page view
  does.
- **Not saved:** the sidebar's scroll position isn't stored; on opening, the
  sidebar follows the current page.
- **Code shape:** the sidebar's state (its renderer, tracker, sharp
  textures, last followed page) lives in `sidebar.rs`, next to the drawing;
  `app.rs` passes in the current page and spread, the low-res textures and
  receives a clicked page back.

## Non-goals

- The switch between thumbnails, search results and the outline.
- Keyboard navigation inside the sidebar.
- Sharp renders outside the visible area (revisit if scrolling shows white
  or blur).
- Several columns, drag-to-reorder, or any editing of pages.
- Saving the sidebar's scroll position.

## Testing

Headless harness tests in `app.rs`, as for the page view, with the fake
engines:

- the sidebar shows one thumbnail per page, as wide as the sidebar minus its
  margins, also in dual mode;
- a click on a thumbnail goes to its page, keeping zoom and modes;
- after a click on a short page in continuous fit-page mode, and on the last
  page, the clicked page is highlighted; scrolling the page view afterwards
  returns the highlight to the centre page;
- the current page (both in dual mode) is highlighted;
- moving the current page out of the sidebar's view scrolls the sidebar;
  scrolling the sidebar by hand isn't undone until the current page changes;
- only visible pages are requested from the sidebar renderer, at the
  sidebar's width; widening the sidebar requests a larger scale;
- a page scrolled out of the sidebar drops its sharp image; closing the
  sidebar drops them all;
- with no sharp image the low-res thumbnail is drawn;
- scrolling the sidebar far away moves the thumbnailer's focus there;
- a reload swaps the sidebar renderer and keeps place.

The `View` method that goes to a page gets unit tests in `noido-core` for
continuous, paged and dual modes, and for the first and last page.

## Left to the builder

- Exact colours and outline style, within the look of the rest of the UI.
- How the sidebar's scroll area exposes its offset for following.
- Names of new types and methods.
