# Links

Date: 2026-10-26 · Status: draft for review

## Intent

PDFs link within themselves (tables of contents, lists of figures, footnote
markers, "see Table 3", citations) and to the web. noido draws them but does
nothing with them. This change makes them work: the pointer shows where a
link is, a click follows it, and after an internal jump a brief flash shows
where the target is on the page.

Both cases matter: papers, where links are small and inline and the reader
jumps and comes back, and books, where table-of-contents entries take the
reader somewhere to read on. Following a link is the same in both; they
differ in which follow-up matters most (history and previews for papers,
the outline sidebar for books), and those come later.

A network analysis paper in testfiles is the real-world
example: 85 links on 17 pages, made by hyperref. Internal links are GoTo
actions to named destinations (`chapter*.5`, `Hfootnote.8`, `table.0.3`),
and every destination is `[page /XYZ left top null]`. Web links are URI
actions, and a URL wrapped over two lines is two link annotations with the
same URI (16th page, printed page 14).

## Goals

- Internal links and web links both work.
- Over a link: the pointing hand (`CursorIcon::PointingHand`) and an outline
  around the link's area. A web link's address shows as a tooltip.
- A click on a link follows it. A drag that starts on a link still pans.
- Internal link, continuous mode: the target is centred in the window.
  Paged mode: the target's spread is shown, as turning to that page would.
- After an internal jump, the line of text at the target flashes, or a
  horizontal bar at the target's height where there's no text there.
- Web links open in the default browser at once.

## Non-goals (to the backlog)

- **History** (back and forward after a jump). Deferred on purpose: it is
  more complex than it looks and gets its own design.
- **A preview of the target** on hover. Unclear what it should be yet.
- **Highlighting all links** on a page, with a key or a button. To think
  about.
- **The outline** (bookmarks) in the sidebar. Same destinations; later.
- Other actions: links to another file (GoToR), Launch, JavaScript, named
  actions (NextPage …). They show no hand and do nothing.
- Drawing link borders as the spec says (`/Border`, defaulting to a 1 pt
  box, `/BS`, `/C`, `/H` for the click look, `/AP` appearances). hayro
  doesn't draw annotations, and hyperref's default red boxes are something
  readers often turn off anyway.
- Changing the zoom to the one a destination names.

## Anti-goals

- Hovering or clicking that waits on rendering, a search or a thumbnail
  pass. Links must feel instant on a 600-page book mid-search.
- A click that fires at the end of a pan.
- Opening anything other than a web or mail address from a PDF (`file:`,
  `javascript:`, …).

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| 1. A links thread of its own | Never waits behind rendering or a search; link code in one place, testable on its own. One more parsed copy (1–3 MB, bytes shared); measured with 20 pages' links and flashes: +2.7 MB (network paper), +4.2 MB (tracemonkey), +5.0 MB (Vaesen) | **Chosen** |
| 2. Links come with each rendered page; the flash line from the search thread | No new thread; but one feature spread over two threads, the flash waits behind a running search, and every render pays for link parsing | Rejected |
| 3. Everything from the search thread | During the first text pass or a search on a big book, links on the visible page show up late; hovering feels broken | Rejected |

## What this builds

### 1. Links from the engine (`noido-engine`, `engine-hayro`)

A new `Document` method, `links(page)`, returns the page's links, defaulting
to none (like `text`). A link has:

- **Areas:** one or more rectangles in page points, the same space as
  glyphs and `page_size` (origin top-left, y down, rotation applied). From
  `/QuadPoints` when present (one area per quad, made upright), else from
  `/Rect`. Areas are clipped to the page; a link with no area left is
  dropped.
- **Target**, one of:
  - **Internal:** a destination page and an optional point on it, x and y
    each optional (in page points, same space). Resolved fully by the
    engine, so the rest of noido never sees PDF names or object
    references.
  - **Web:** the URI string, as given.

Reading them, in engine-hayro (hayro has no annotation API; this works on
the page's raw dictionary and the xref):

- `/Annots` entries with `/Subtype /Link`. Order kept; where links overlap,
  the last one is on top (PDF painting order).
- Target from `/Dest`, or from `/A` with `/S /GoTo` (its `/D`) or `/S /URI`
  (its `/URI`). Anything else: not a link, skipped.
- A destination is an explicit array or a name (string or PDF name). Names
  are looked up in the catalog's `/Names /Dests` name tree (with `/Kids`
  and `/Limits`), then the older catalog `/Dests` dictionary. The value may
  be the array itself or a dictionary with `/D`.
- The array's first element is a page reference (or, in broken files, a
  page index). Its kind gives the point: `/XYZ left top` → (left, top);
  `/FitH top`, `/FitBH top` → (none, top); `/FitV left`, `/FitBV left` →
  (left, none); `/FitR l b r t` → (l, t); `/Fit`, `/FitB` → none. `null`
  stays none. The point is transformed with the page's transform, like
  glyphs, so rotated and cropped pages land right.
- A link whose destination can't be resolved is dropped.
- Malformed input never panics; whatever can't be read is skipped.

### 2. The line at a target (`noido-text`)

`PageText` gets a method that, given a point (x and y optional), returns the
bounds of the line to flash: of the lines whose top is at or below y minus
a small tolerance (a few points; hyperref puts the anchor a little above the
text) and, when x is given, that reach past x, the one whose top is nearest
y, by position, not content-stream order (my call: other producers draw
headers, footers and floats out of order).
No y: the page's first line. None found: nothing. The line's bounds are the
union of its glyphs' bounds.

### 3. The links thread (`noido-render`)

`Linker`, like `Thumbnailer` and `TextSearcher`: started with the engine,
the bytes and a waker; opens its own document in the background. Two
requests, answered in order:

- **Links of a page.** Answer: the page and its links.
- **The flash of a target** (page, optional point). Answer: the target and
  the rectangle to flash, from the page's text (§2), or `None` when there's
  no line there. Extracts that one page's text and doesn't keep it.

The GUI asks for a page's links when the page becomes visible and keeps
them while the page is in the keep window, as it does textures. A failed
page answers with no links. The thread is restarted on reload like the
others (`watch::Reload` gets a linker).

### 4. Jumping (`noido-core`)

`View::go_to_target(page, point)`:

- Continuous mode: scroll so the point is at the centre of the window, as
  far as scrolling allows. No y: the page's top at the top of the window,
  as `go_to_page` does. No x: horizontal position unchanged.
- Paged mode: show the page's spread, as `go_to_page` does. If the point
  isn't visible after that (zoomed in), bring it into view as `reveal`
  does.
- Zoom never changes.

### 5. The GUI (`gui-egui`)

- **Hover:** the topmost link under the pointer on a visible page shows the
  pointing hand and an outline around each of its areas (all areas of that
  link, so both halves of a wrapped URL light up). The hand takes precedence
  over the open hand but not over a resize edge or the sidebar's handle,
  as the Grab cursor already doesn't. Web links show their address as a
  tooltip.
- **Click:** a press and release on the same link, counted as a click by
  egui (no drag), follows it. A press that turns into a drag pans and
  follows nothing; the closed hand shows while dragging, as now.
- **Internal link:** `go_to_target`, then ask the linker for the flash.
  The target page becomes the current page (`chosen`), as with a sidebar
  thumbnail click, and the search is told the reader moved
  (`search_state.reader_moved()`), so the sidebar highlights the target
  page and prefetch focuses on it even where the centre of the window is on
  another page (clamped at the end, the other half of a dual spread). My
  call.
- **Web link:** opened through egui's `open_url` (eframe hands it to the
  system browser), only for `http`, `https` and `mailto` (case-insensitive).
  Others do nothing.
- **Flash:** an outline around the rectangle in the accent colour, fading
  out over about a second. No line found: a horizontal bar across the page's
  text width at the target's y (the page width minus margins if there's no
  text to measure; builder's call). A new jump replaces a running flash. In
  paged mode the flash shows the same way.
- Links arriving for a page under a still pointer take effect on the next
  frame; no hover shows until they're known.

## Testing

TDD throughout.

- **Fixture:** `fixtures/links.pdf` from `make_fixtures.py`: a page with
  text and links of each kind (explicit `/Dest` array, GoTo to a named
  destination in a `/Names` tree with `/Kids`, a name in the old `/Dests`
  dictionary, URI, a link with `/QuadPoints`, an unresolvable name, a
  GoToR), each destination kind, a `null` in an `/XYZ`, and a target on a
  rotated page.
- **engine-hayro:** areas and targets of every link in the fixture, in
  page points; the rotated page's target point; skipped and dropped links.
  An ignored test against the network analysis paper when testfiles are
  present (counts per page, one known target).
- **noido-text:** the line at a point: below the anchor, with x, without y,
  none found.
- **noido-core:** `go_to_target` in continuous and paged mode, near the
  ends of the document (clamped), without x, without y, zoomed in.
- **Harness** (fake engine with links): hover over a link → PointingHand
  and the outline drawn; hover over the page elsewhere → Grab; a click
  jumps and centres the target; a drag starting on a link pans and doesn't
  jump; a web link click sets `open_url`, a `javascript:` one doesn't; the
  flash is drawn after the jump and gone after its time.

## Left to the builder

The names of types and methods; the outline's and flash's exact look
(width, corner rounding, fade curve); the tolerance above a line; how links
are stored in the GUI; how the harness fakes the linker.
