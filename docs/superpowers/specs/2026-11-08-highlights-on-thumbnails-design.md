# Text highlights on the thumbnails

Date: 2026-11-08. Agreed with the user. A small change: the left sidebar's
page thumbnails show the document's text highlights.

## Intent and why

The left sidebar is a column of page thumbnails, and marks made on the
pages leave no trace on it. A reader who has highlighted their way through
a document sees a column that looks untouched, which feels incomplete.
This paints each text highlight onto the thumbnails of the pages it
covers, in the same colour and place as on the page, so the column shows
where the document is marked at a glance.

## Goals

- Every text highlight is drawn on every visible thumbnail whose page it
  covers, scaled from page coordinates into the thumbnail's rect, in the
  same colour as on the page (light and dark).
- A highlight that spans pages draws on each page it covers.
- Marks appear as each page's text layout arrives. Pages with highlights
  that show in the column get their text requested, once.
- A mark whose scaled rectangle would be thinner than one point is drawn
  one point thick, centred on where it belongs, so a one-word highlight
  still shows at thumbnail size.

## Non-goals

- Quoted text, excerpts, page numbers or any list of highlights in the
  column. The column stays thumbnails; a list is the right sidebar's job
  later (`2026-11-04-annotations-design.md`).
- Clicking a mark to jump to that highlight or select it. A thumbnail
  click still opens its page at the top, as today.
- Hovering or otherwise interacting with marks.
- Live resizing of a mark while a highlighted selection's handle is being
  dragged; the thumbnail catches up when the change is committed.
- Showing marks in the search-results view, where thumbnails aren't drawn.
- Notes, tags, orphans, or any change to the highlight model.
- Indexing highlights by page: a linear scan per visible page is fine for
  this slice.

## Anti-goals

- Marks that drift from their page position when the sidebar is resized
  or the window scaled: they are recomputed from page coordinates and the
  thumbnail's current width each frame.
- Thumbnail colours that differ from the page's for the same highlight.

## Approaches

1. **Paint the marks on the thumbnails (chosen).** The column keeps doing
   what it does — showing pages — and gains the markings. It answers
   "which pages are marked, and where" without a second mode or a second
   place for annotations.
2. **A list of highlight cards in the column**, like search results:
   legible quoted text, but a second mode in a thumbnail column, and it
   duplicates the right sidebar the annotations design reserves for the
   reader's notes.

The user picked 1.

## Design

### The sidebar's inputs (`gui-egui/src/sidebar.rs`)

`sidebar::Input` gains:

- `highlights: &'a [Highlight]`, the document's highlights in their
  stored order.
- `texts: &'a mut Texts`, the page text layouts. `SidebarView::show` and
  the helpers it calls take `&mut Input` so the sidebar can ask for text.
- `request_text: &'a dyn Fn(usize)`, how to ask the text thread for a
  page.

In `SidebarView::column`, for each page in the kept range that any
highlight covers, ask once for its text (`texts.want(page, |p|
request_text(p))`). Destructure `Input` where both are needed so the
mutable and shared borrows are taken from the struct, not through it.

### Drawing

After `paint_page` draws a thumbnail's image, and before its outline
stroke, loop the highlights:

- `highlight.on_page(page, text.char_count())` gives the character range,
  if any, as on the page.
- `text.rects(range)` gives page-point rectangles.
- Map each into the thumbnail with
  `rect.min + vec2(area.x, area.y) * scale`, where
  `scale = thumb.width / layout.page_size(page).width`, and size
  `vec2(area.width, area.height) * scale`. Use `rect_filled(.., 0.0,
  fill)` as on the page, with `select::highlight_fill(colour,
  ui.visuals().dark_mode)`.
- Grow a side under one point to one point, centred, before filling.

A pure helper in `sidebar.rs` does the scaling and the minimum size, so it
can be tested without a window:

```rust
fn mark_rect(thumb: Rect, page_width: f32, area: noido_text::Rect) -> Rect
```

The sidebar already computes `self.thumb_width`; the page's width comes
from `input.layout.page_size(page)`.

Pages whose text is not known yet draw no marks; the request makes them
appear a frame or two later, as the views do.

### Wiring (`gui-egui/src/app.rs`)

Where `sidebar::Input` is built, pass `self.highlights.items()`,
`&mut self.texts`, and the search state's text request. No new state and
no change to saving.

## Tests

- **Unit (`sidebar.rs`)**: `mark_rect` maps a page-point rectangle into a
  thumbnail at the right position and size; a rectangle under one point
  tall or wide grows to one point, centred; an ordinary rectangle is
  unchanged.
- **Harness (`app.rs`)**: with known page text and a highlight on a
  visible page, the frames paint `select::highlight_fill(colour, dark)`
  rectangles inside that page's thumbnail rect, at the scaled position.
  A highlight spanning two pages paints on both thumbnails. A highlight
  on a page whose text is unknown paints nothing until the text arrives.
  While search results show, no highlight rectangles are painted in the
  sidebar. Existing thumbnail and result tests are unchanged.
- **No state, core or storage changes**, so their tests are untouched.

## By hand

Open a marked document and scroll the thumbnails: marks on the right
pages, in the right colours, following the sidebar's width as it is
resized. Make a new highlight from a selection and see it appear in the
column. Search a word and check the column still shows result cards.

## Left to the builder

The exact minimum thickness (one point unless it looks wrong at a high
scale factor); whether a mark needs clipping against the thumbnail rect
in practice; whether the page-number pill needs anything to stay readable
over a mark under it.
