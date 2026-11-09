# Pins in the sidebar

Date: 2026-11-09 · Status: draft for review

## Intent

The Pages tab is the whole document at a glance, and the Outline tab is the
document's own structure. Neither remembers the pages *the reader* cares
about: the figure they keep checking, the appendix, the two pages they are
working between. A pin is that shortlist, kept per document and shown in a
tab of its own.

A pinned page is also something the reader wants to *look at* without
leaving what they are reading. Single-clicking a pin opens the page in a
small floating panel — the page itself, whole and fitted, resizable, with
its links live. The panel is the point: you can check a footnote or a
figure on a pinned page and not lose your place in the main view, and
following a link inside it moves the document underneath.

## Goals

- A pin control in the upper-right corner of every Pages thumbnail.
  Clicking it pins or unpins that page, and does not navigate.
- A `Pins` tab beside `Pages`, in the strip at the left of `Outline`
  (`Pages · Pins · Outline · Search`). It shows the pinned pages, and only
  them, as thumbnails in page order: the page image, its number, and the
  current page marked.
- Pins are the reader's, kept per document, saved with the rest of the
  document's view state and restored with it.
- Single-clicking a pin opens the preview on that page, once egui's
  double-click window has passed with no second click. The main view does
  not move; the current page, zoom and scroll stay as they were.
- Double-clicking a pin navigates the main view to that page, as clicking a
  thumbnail in Pages does, and closes the preview. The first click of the
  double click does not open the preview.
- The preview is a floating panel over the pages: the whole page fitted
  inside it, resizable, movable, with a close `×` in its upper-right
  corner. The pointer leaving the panel fades it to half opacity; entering
  it makes it opaque again.
- Links on the previewed page are live: an internal link moves the main
  view and flashes the target (the preview stays on its page); a web or
  mail link opens the browser, as elsewhere.
- One preview at a time. Clicking another pin swaps the page, keeping the
  panel's place and size.
- Unpinning the page the preview shows closes the preview.

## Non-goals

- Pins at a place (a scroll position, a selection) rather than a page.
- Pinning or unpinning from the Pins tab; pins are managed from the Pages
  tab only. (A pin icon there, or a context menu, can come later.)
- Reordering pins by hand; they are in page order.
- Keys or menu items for pinning, for opening the preview, or for
  switching to the Pins tab.
- Highlights, search marks and text selection on the previewed page, and
  the page's text for them; and the document's highlights on the Pins
  tab's thumbnails.
- The glow on the Pins tab when a pin changes; deferred by the reader.
- A zoom inside the preview: the whole page is shown, and resizing the
  panel is how you make it bigger.
- Saving the preview's place or size between sessions, or reopening the
  preview on start.
- Closing the preview with Esc. Esc has other jobs (search, the select
  tool); if it wants one, it is a later change.
- More than one preview, or a preview as an OS window of its own.
- Pins shared between documents, or written into the PDF.

## Anti-goals

- A pin click that also navigates, or a thumbnail click that both previews
  and moves the main view.
- A preview that stalls the window, delays the pages or opens a second
  parsed copy of the document.
- Pins that point past the end of a document that got shorter: after a
  reload to fewer pages, no pin, and no preview, may refer to a page that
  is gone.
- A preview the reader cannot close, or that covers the whole window.
- A tab strip whose labels collide, or are cut mid-glyph, at the
  narrowest sidebar.

## Constraints in today's code

- `SidebarTab` (`noido-core/src/sidebar.rs`) has three variants; the strip
  in `sidebar.rs` lays the labels out in equal shares of the sidebar width,
  and `SIDEBAR_MIN_WIDTH` is 160. Four labels (`Pages`, `Pins`, `Outline`,
  `Search`) do not fit four ways at 160: the longest is about 40 points.
  The minimum stays 160; the labels truncate when they do not fit.
- `DocState` (`noido-state/src/lib.rs`) is `view`, `sidebar`, `history`,
  `highlights`, written as one TOML `Entry` with `#[serde(default)]` on
  everything added after a release. `sidebar_tab` is a string; unknown
  values mean thumbnails. `Results` is never saved (`last` is).
- The sidebar's `SidebarView` has a `Renderer` of its own (a second parsed
  copy), a `RenderTracker`, and `sharp: HashMap<usize, PageImage>`, keyed
  by page. It asks for the visible thumbnails plus a margin, and keeps
  them in a contiguous `Range`.
- The main renderer keeps `Keep { window, extra }` pages; `extra` already
  holds the spreads of Back/Forward targets and of neighbouring search
  hits, so a page off screen can be kept sharp. There is one `textures`
  texture per page and one requested scale per page in `tracker`, chosen
  from the view's `display_scale`.
- `Links::want(page)` asks the links thread for a page's link areas;
  `Links::at(page, x, y)` hits them in page points; `follow_target` moves
  the view, marks the page chosen, records history and flashes the target,
  or opens a web link.
- egui's fonts carry no icon glyphs: icons in noido are painted shapes
  (`paint_chevron`). The harness can click, hover, drag, double-click, and
  inspect the shapes a frame drew.

## Approaches considered

Three questions had more than one real answer.

| Question | The alternatives | Chosen, and why |
| --- | --- | --- |
| What kind of window is the preview? | **A separate OS window** (a viewport): could sit on another monitor. Pointer-leave opacity is not available for a native window on Linux/Wayland or macOS without platform work, and it would be a second window through the shell, menu and instance code. | **A floating panel inside the document window.** Opacity, the `×` and live links all stay in one coordinate space, and the panel is drawn over the pages with egui. |
| How does the page sit in the panel? | **Fit the width and scroll the height**: readable at once, but hides the part of the page you pinned it to look at, and the wheel inside the panel fights the document's. **Fit the whole page**: a big thumbnail with live links until the panel is dragged larger. | **The whole page, fitted, no scrolling.** The thumbnail already answers "which page", so the panel's job is the page at a useful size; it is resizable precisely so it can be made readable. |
| Where do the preview's pixels come from? | **A renderer of its own**: perfect isolation; but a sixth parsed copy of the document (3–5 MB for a big book) and either an open at document-open or a stall on the first click. **The sidebar renderer, extended**: no new parse, but the preview's policy in a renderer that exists for thumbnails, with the same one-image-per-page coupling. | **The main renderer's `extra` keep.** The previewed page is kept like a Back target, at the panel's scale; where it is also visible in the view, the larger of the two scales is asked, so one texture serves both. No new thread, no new parse. |

The Pick-another-page and fade behaviours have one sensible shape each and
are settled in the design below.

## What this builds

### 1. Pins as core state (`noido-core`)

A new `pins.rs`:

```rust
pub struct Pins(Vec<usize>);
```

Pages in ascending order, no duplicates; `toggle(page) -> bool` (true when
it is now pinned), `contains`, `pages() -> &[usize]`, `remove`,
`retain_below(page_count)`, `Default` empty. Unit tests for order, the
toggle's answer, and pruning.

`SidebarTab` gains `Pins`. The strip puts it second; `choose` and
`search_cleared` treat it like any tab that is not `Results`, so it is the
tab a search returns to and the one saved. `SIDEBAR_MIN_WIDTH` stays 160:
the strip draws each label truncated to its tab when it does not fit
(egui's `truncate_at_width`, as an outline row's title does), so four
labels share 160 without colliding. A test at the minimum asserts that
every drawn label fits inside its tab. Making the narrowest sidebar
comfortable is left for later, as the reader asked.

### 2. Storage (`noido-state`)

`DocState` gains `pins: Pins`. The `Entry` gains

```toml
pins = [2, 7, 61]            # sorted, unique; absent when empty
```

`#[serde(default, skip_serializing_if = "Vec::is_empty")]`. Loading sorts
and drops duplicates; the state is read before the document is parsed, so
it has no page count and leaves the range alone. The app prunes past-the-
end pages with `Pins::retain_below` when it restores the state, where the
page sizes are known (`with_persistence`), so a pin made in a long version
and opened in a short one lands on no page. `sidebar_tab` saves `"pins"`
and reads it back; every other unknown value, including the old
`"bookmarks"`, still means thumbnails. Old files load with no pins.

### 3. The pin control on Pages thumbnails (`egui`)

Each Pages thumbnail gets a small pin in its upper-right corner, inside the
page image: a painted pin (head and needle), over a rounded chip filled
with the panel colour at most of its opacity so the pin reads on a white
page, a dark scan or a photo. It is drawn always, in the weak text colour
when the page is unpinned and the accent (link/selection) colour when it is
pinned; hovering it brightens the chip and gives the pointing hand.

Its hit area is about 22 points square, inset from the page's corner. It is
registered after the thumbnail's own response, as the outline's chevron is,
and a press on it neither navigates nor scrolls: the thumbnail's click is
ignored for that press. The sidebar reports `toggle_pin: Option<usize>` to
the app, which applies it to `Pins` and saves as usual.

### 4. The Pins tab (`egui`)

The same card as a Pages thumbnail — page image, number row, current-page
highlight — but built from a column of the pinned pages only, in page
order, and without the document's highlights (that waits). A filtered
column (list index to page)
keeps the existing layout maths; the tab has its own scroll id and scroll
position, and when the tab is shown it reveals the current page if that
page is pinned.

Rendering works as the Pages tab's does, on the sidebar's own renderer,
except that the kept pages are sparse. The Pages tab's keep is kept while
Pins shows — the existing contract that sharp thumbnails survive a trip to
another tab (`sharp_thumbnails_survive_other_tabs`) holds here too — and
the visible pinned pages, plus a margin of neighbouring pins, join it as
`Keep::extra`. `SidebarView.kept` becomes a `Keep` (a window and extras)
rather than a range, and the Pins tab's request unions the Pages window
with the pinned extras. Low-res thumbnails stand in where they exist.

An empty document for pins (the common case at first) shows a muted note:
`No pins yet. Pin pages from the Pages tab.`

### 5. The preview panel (`egui`, with the fit maths in `noido-core`)

**State.** `NoidoApp` gains:

```rust
struct PinPreview {
    page: usize,
    /// The panel's rect in screen points.
    rect: egui::Rect,
    /// False until the reader moves or resizes it: it opens centred.
    placed: bool,
    /// How opaque it is drawn now, 0.5 to 1.0.
    opacity: f32,
}
```

`Option<PinPreview>` is `None` while closed, and `NoidoApp` also holds the
last rect and its `placed` flag, so closing keeps them: within a session,
reopening opens where it last was (nothing is saved to disk). The app is
the source of truth for the rect, not egui's window memory: each frame it
hands the window `current_pos(rect.min)` and reads the resulting rect back.
Opening a pin that is not the shown page swaps `page` and keeps `rect`,
clamped into the window; the first open of a session computes it (page
aspect, centred in the page view). Unpinning the page the preview shows
closes the preview: the panel is the pin's page, so it goes with the pin.

**Fit.** A pure helper in `noido-core/src/preview.rs` fits a page's size
into a rect with the page centred and its aspect kept, and maps a screen
point back to page points for link hits; unit tests cover the letterboxing
and the mapping's round trip.

**The panel.** An `egui::Window` with no title bar, resizable, movable by
dragging anywhere on it, drawn after the page view so it is above the
pages. Its content is the page fitted inside a small padding, the paper
tint, the sharp render from `textures`, with the thumbnail as stand-in
while the sharp one arrives, fading in as on the pages. The panel has a
border and a shadow from the theme so it reads as floating. First open
sizes it to the page's aspect at about 60% of the page view's height,
capped by the view's width, centred in the page view and kept inside the
window; after the reader has placed it, the size and place stand.

**Close.** A `×` chip in the panel's upper-right, over the page's corner,
with its own hit area that closes the preview. It is registered after the
window's own content, as the pin is after the thumbnail.

**Fade.** While the pointer is inside the panel's rect it draws at full
opacity; when it leaves, the panel eases to 0.5 (and back in on entry),
repainting while it does. Both the page tint's alpha and the chrome's
colours carry the opacity. Under reduced motion the two values are swapped
at once.

**Links.** While the preview is open, the app asks
`Links::want(preview.page)`; when the page's links arrive it hit-tests the
pointer's page point with `Links::at`, shows the pointing hand over a link,
and on click follows it with the existing `follow_target`. An internal
target moves the main view, marks the page chosen, records history and
flashes there, and the preview stays on its own page; a web link opens the
browser. Following a link inside the preview must not re-enter the main
view's own link handling.

**Rendering.** The previewed page joins the main renderer's priority order
and its `extra` keep, so it is rendered and kept even far from the current
page. Its wanted scale is the panel's (page width in the panel, times the
pixels per point), capped by `MAX_RENDER_SIDE`; where the page is also
visible in the main view, the larger of the panel's scale and the view's
`display_scale` is requested, so one texture is sharp for both. Closing
the preview removes it from the keep, and the page's texture is dropped as
any other page outside the keep.

**The delay, and double click.** A click on a pin card does not open the
preview at once: the app records the page and the time, and opens the
preview only when egui's double-click window (`max_double_click_delay`,
0.3 s by default) has passed without a second click; it asks for a repaint
at that moment. A second click on the same card inside the window is
egui's `double_clicked`: it cancels the pending preview and navigates the
main view to the page — the same path a Pages thumbnail click takes, with
history and the current page — closing any preview. So the first click of
a double click never opens the panel. A click on another card replaces the
pending one, as the reader changed their mind; two different cards are not
a double click, as egui keeps the double click per widget. The cost is
that a single click shows the preview about a third of a second late,
which is the price of telling the two apart.

**Reload and shrinking.** A reload to a shorter version prunes pins to the
new page count; if the previewed page is gone, the preview closes.

## How it is split into commits

1. `core: pins, a list of pages the reader keeps` (with the fourth tab and
   truncated labels).
2. `state: save and restore pins, and the Pins tab`.
3. `egui: pin pages from their thumbnails`.
4. `egui: a Pins tab with the pinned pages`.
5. `egui: preview a pinned page in a floating panel` (fit maths first as a
   core commit if it is cleaner alone).

## Tests

Every behaviour is written as a test first, watched failing, then made to
pass. In `noido-core`: `Pins` order/toggle/prune, `SidebarTab::Pins`
choice and `last`, the preview fit and its inverse. In `noido-state`:
pins round-trip, empty pins absent from the file, duplicate pins dropped
on load, old files load with none, `"pins"` as a saved tab.

In the GUI harness (`app.rs`), at least:

- each Pages thumbnail draws a pin; clicking a pin pins its page and does
  not navigate or scroll; the same click again unpins; a pinned page's pin
  is drawn in the accent colour;
- the strip shows four labels, each drawn inside its tab at the narrowest
  sidebar; the ones that do not fit whole are truncated (a `…`), not cut
  mid-glyph;
- the Pins tab shows exactly the pinned pages, with their numbers, in page
  order, and the empty note when nothing is pinned; the Pages tab's sharp
  thumbnails survive a trip to Pins and back;
- a restored state with pins past the end of a shorter document keeps only
  the pins that exist;
- a single click on a pin card opens the preview on that page once the
  double-click window has passed, without moving the main view; the panel
  is not open in the frames before that; a second pin swaps the page and
  keeps the panel's rect; closing and reopening within the session opens
  in the same rect;
- a double click on a pin card navigates the main view and closes the
  preview, and the preview is never open, not even for a frame;
- the `×` closes the preview;
- the preview draws the page image fitted; the page is requested from the
  renderer at the panel's scale and kept while open, even when the main
  view is far away;
- with the pointer outside the panel, the page image is drawn tinted to
  half opacity, and full when the pointer is on it (asserted from the mesh
  the frame drew, not from the field);
- a link on the previewed page: hovering gives the pointing hand; clicking
  an internal link moves the main view and leaves the preview on its page;
  clicking a web link opens it;
- a reload to a shorter document drops out-of-range pins and closes a
  preview of a page that is gone;
- unpinning the page the preview shows closes the preview.

## What is left to the builder

- The exact pin glyph and chip, the `×` chip, and the fade's duration
  (kept short; the theme's reduced-motion setting is honoured).
- The exact default panel size (about 60% of the page view's height, capped
  by its width) and its padding.
- Whether the panel's shadow and border come from the theme's dialog or
  menu surface.
- Whether the preview's stand-in comes from the low-res thumbnailer, the
  sidebar's sharp thumbnail of that page, or blank paper.
- Whether the wheel over the preview should reach the document behind it;
  test by hand.
- How a too-long tab label is shortened (egui's `truncate_at_width` and
  its ellipsis), and whether the narrowest strip wants more later.
