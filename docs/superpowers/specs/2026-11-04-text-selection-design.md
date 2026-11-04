# Text selection and copy, with a select tool

Date: 2026-11-04. Agreed with the user. Step 1 of the annotations project
(`2026-11-04-annotations-design.md`).

## Intent and why

There is no text selection today: a left drag on the pages pans them
(`DragScroll::Always`). Highlights and notes (steps 2–4) start from a
selection, and copying text is useful on its own. This step adds
selecting text with the mouse, a small bar by the selection, and copying.

## Goals

- **Shift+drag selects**, starting anywhere. With the **select tool** on,
  a plain drag selects and Shift+drag pans, keeping the selection: Shift
  always swaps the plain drag for the other one.
- **The select tool**: shortcut `S` (free; matches the single-letter
  toggles C, D, O, G), an icon button in the title bar next to the sidebar
  button, lit while on, and a check item "Select text" in the menu. Not
  saved; starts off.
- **A selection drag never pans.** Dragging to or past the window's edge
  scrolls toward it, faster the further out the pointer is.
- **What gets selected**: from the first character the drag reaches to
  the one under the pointer, in reading order, across pages and both
  pages of a spread. Starting in a page's margin anchors at the nearest
  line's start or end, at once. Starting in the gap between pages or on a
  page without text selects nothing until the drag reaches text. A page
  without any text (a scan without OCR) is crossed and adds nothing; no
  message. OCR layers are text like any other.
- **Clicks**: double-click selects a word, triple-click a line, only
  where a plain drag would select (tool on) or with Shift held; with the
  tool off and no Shift, clicks select nothing (the user's call).
  Shift+click with a selection extends it to the click, also on a link
  (Shift means selecting). With the tool on, a plain click on a link still
  follows it; a drag starting on one selects.
- **Handles**: a small knob at each end of the selection, as in mobile
  UIs. Dragging one moves that end (with edge scrolling). A drag starting
  on a handle always adjusts the selection, tool on or off; it never pans.
- **The bar**: on release, a small bar appears just above where the
  selection ends, below it if there's no room. It holds **Copy** in this
  step; step 3 adds the highlight colours. It moves with the page while
  scrolling, and hides while a handle is dragged.
- **Copy**: Mod+C copies at once (no need for the bar). A hyphen at a
  page's end is treated as at a line end. Lines are joined
  with spaces; a line-end hyphen inside a word is removed and the word
  rejoined, by the same rule search uses (including OCR's `¬` and the
  soft hyphen); a paragraph break, where the gap to the next line shows
  one, becomes a newline. To be refined later.
- **Clearing**: a click (not a drag) elsewhere on the pages, Esc, a new
  selection, and reloading or switching the document clear it. Panning
  and zooming don't. Esc with the pages using the keyboard clears the
  selection; today Esc there does nothing. While settings are open or the
  search field has focus, Esc does what it does today there (closing
  settings; clearing the field, then closing a sidebar the search opened)
  and leaves the selection alone. Esc doesn't turn the tool off; `S` does.
- **Pointer**: an I-beam over text where a plain drag would select (tool
  on, or Shift held); the hand where it pans, as today; the pointing hand
  over a link as today.
- **Look**: the selected lines filled with the theme's selection colour
  (`Palette::selection`), translucent, over the page image.
- Not saved, not part of history.

## Non-goals

Highlights, regions, notes (steps 2–4). Select-all (ambiguous: the page or
the document). Keyboard selection (Shift+arrows). Selecting by rectangle
(columns). Rich copy (HTML, Markdown).

## Approaches

Everywhere: the selection logic is pure, in `noido-core`; the frontend
does pointer, edge scrolling, handles, bar and drawing. What differs is
where the UI gets page text, which only the text thread has today.

- **A. The text thread shares its text (chosen).** The thread keeps
  extracting and caching; a page's text becomes `Arc<PageText>` and is
  sent to the UI for the pages near the view, as rendered tiles are.
  Hit-testing is synchronous on the UI thread: the selection follows the
  pointer exactly and harness tests stay simple. A page whose text hasn't
  arrived can't be selected for a frame or two. Steps 2 and 3 need the
  same text on the UI side.
- **B. Ask the text thread on each pointer move.** One owner of the text,
  but each move is a round trip: a frame of lag, line rectangles sent
  back, harness tests waiting on the thread.
- **C. Extract on the UI thread when needed.** Simplest, but heavy pages
  (dense tables, big OCR layers) take tens of ms and would stutter the
  drag; and a second copy of the text.

## Design

### Text for the UI (`noido-render`, frontend)

- The text thread gets a command asking for a page's text. It answers
  with the page's `Arc<PageText>` (extracting it if needed) ahead of any
  search step or background extraction. The search keeps working as now;
  `PageText` is shared, not copied. (Its lazily built search forms are a
  `OnceLock`, fine to share.)
- The frontend keeps the pages' text the way `links.rs` keeps links: asked
  for once per page when it shows (the visible spreads, as for links), and
  for pages a selection drag reaches; kept for the document, forgotten on
  reload. This starts the text thread for every document, not only once
  the search field is used; it extracts only the pages asked for until a
  search asks for all.

### Positions and the selection (`noido-core`, new `selection.rs`)

- A **position** is a caret in document order: a page and a character
  index between characters (`0..=char_count`). Document order is page
  order, then the page's character order (content-stream order, as
  `PageText` gives it).
- **Hit-testing** a point on a page with text: the line whose rectangle is
  nearest the point (distance 0 inside it), then on that line the caret
  before or after the glyph under or nearest the point by its horizontal
  middle; left of the line is its start, right of it its end. A point on a
  page without text is the position at that page's start. A press on a
  page whose text hasn't arrived keeps its point in page coordinates and
  is hit-tested when the text comes (a frame or two), so the anchor lands
  where the press was, not at the page's top. A point between pages or outside them maps to the
  boundary between the pages around it, in layout order.
- A **selection** is an anchor and a focus position; the selected
  characters are those between them in document order, whichever comes
  first. Starting off text needs nothing special: the anchor is a
  boundary position, and the selection is empty until text lies between
  anchor and focus.
- **Word and line**: the word at a point (letters, digits and joiners, or
  else the single character), and the line at a point, for double- and
  triple-click.
- **What's drawn**, per page: one rectangle per line, the union of the
  selected characters' glyph bounds on it, its height the line's. Handle
  positions: the caret at each end.
- **Copied text**: the selected text with the joining rules above. The
  joining itself lives in `noido-text` beside search's line-end hyphen
  rule, so both use one rule; it takes the selected pieces of each page in
  order and joins across pages as across lines.
- **Actions**: `ToggleSelectTool` (`S`), `Copy` (Mod+C), and a check for
  the menu item. `Copy` must not act while a text field or other egui
  widget is using the keyboard: the search field's own copy has to keep
  working. egui-winit turns Mod+C into `egui::Event::Copy` and sends no
  key event for it, so the window acts on `Event::Copy` when no widget
  has keyboard focus; the keymap's binding is for the shortcut label and
  the menu, and harness tests send `Event::Copy`. With nothing selected,
  Mod+C does nothing.

### The window (`gui-egui`)

- **Which drag**: decided when the button goes down, from the tool, Shift,
  and whether the press is on a handle; changing Shift mid-drag changes
  nothing. A selecting press turns the scroll area's drag scrolling off
  (`DragScroll::Never`) for that drag. A click on a link with Shift or
  the tool still follows the link (clicks and drags are already told apart
  for links).
- **Edge scrolling** while a selection or handle drag is under way: within
  a small margin of the viewport's edge, or past it, the pages scroll
  toward it at a speed growing with the distance, capped; frames are asked
  for while it goes on. In paged (non-continuous) mode it scrolls within
  the spread when zoomed in; it doesn't turn to the next spread.
- **Handles and the bar** are drawn on the pages' layer, positioned in
  page coordinates, so they move with the pages.
- **Selection colour**: `Palette::selection` at reduced opacity, drawn
  above the page image and below the link flash and search highlights'
  current hit, so those stay visible.

## Tests

- **Core**: hit-testing inside a glyph's left and right halves, left and
  right of a line, above and below the text, between two lines, in a
  two-column page (the nearer column wins); on a page without text;
  between pages; order across pages and spreads; empty selection when
  anchor and focus have no text between; rectangles per line for a
  selection spanning lines and pages; word and line at a point; handle
  positions; Shift+click extending from either end.
- **Text**: copied text joins lines with spaces, removes line-end hyphens
  inside words (and `¬`, soft hyphens), keeps real hyphens at line ends
  where search does, puts a newline at a paragraph gap, joins pages.
- **Render**: a page's text is answered on request, also while a search
  runs, and is the same text search uses.
- **Harness**: Shift+drag selects and doesn't pan; a plain drag pans and
  selects nothing; with the tool on, a plain drag selects and Shift+drag
  pans and keeps the selection; `S`, the menu item and the title bar
  button turn the tool on and off and show its state; Mod+C copies the
  selection (`copied`); Mod+C in the search field doesn't copy the
  selection; the bar appears on release and its Copy copies; Esc clears
  the selection, and Esc in the search field leaves it; a click on the
  pages clears it; a handle drag moves that end, also with the tool off;
  dragging past the bottom edge scrolls and extends the selection;
  double- and triple-click select a word and a line with the tool on or
  Shift held, and nothing with the tool off and no Shift; the I-beam shows
  with Shift held over text; a selection drag starting on a link selects
  and doesn't follow it.

## By hand

On a paper and an RPG rulebook: Shift+drag across a paragraph, across a
page break, across both pages of a spread; drag past the window's bottom
edge and watch it scroll; adjust both ends with the handles; Copy from the
bar and with Mod+C and paste into an editor: hyphenated words rejoined,
paragraphs on their own lines. Press `S` and drag without Shift; Shift+drag
pans without losing the selection. On a scan with OCR, select and copy;
on a scan without OCR, drag across it: nothing selected, nothing else
happens. In the search field, Mod+C copies the field's text.

## Left to the builder

The handle and bar sizes and look; the edge margin and scroll speeds; the
paragraph-gap threshold; the selection's opacity; the title bar icon's
drawing; the menu group the check item goes in.
