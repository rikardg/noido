# Highlights on the page

Date: 2026-11-07. Agreed with the user. Step 2 of the annotations project
(`2026-11-04-annotations-design.md`), rescoped as a vertical slice covering
text highlights end-to-end.

## Intent and why

Step 1 (`2026-11-04-text-selection-design.md`) added text selection,
handles, the selection bar, and copying. This step lets the reader turn a
selection into a coloured highlight on the page, click an existing
highlight to adjust its ends with the same handles, change its colour,
copy its text, or delete it, undo and redo changes, and keep highlights
in the document's state file across launches.

## Goals

- **Five colours**: yellow, green, blue, pink, and purple.
- **Making a highlight**: when text is selected, the selection bar shows
  `Copy` followed by the five colour swatches. Clicking a swatch creates a
  highlight over the selected range and selects it, so the bar stays open
  on it (now with its delete button) for a recolour or a delete.
- **Across pages**: a highlight is a pair of carets `(start, end)` in
  document order (`start < end`) and a colour, so a selection crossing a
  page break becomes one highlight spanning those pages.
- **Overlapping highlights**: highlights stay independent (never merged or
  split when they overlap or touch), so a highlighted word inside a
  highlighted paragraph remains its own item.
- **Drawing**: each highlight is drawn on the pages it covers using
  `PageText::rects` for its character range on each page, filled with its
  translucent colour above the page image and below the active text
  selection, search hits, and link outlines/flashes.
- **Selecting an existing highlight**: a plain click (not a drag) on a
  highlight selects it, whether the select tool is on or off:
  - When a link and a highlight share the spot, a plain click follows the
    link as today; `Shift`+click (with nothing selected) selects the
    highlight instead of following the link.
  - When several highlights cover the click point, the shortest one (by
    page span, then character span, tie-broken by most recently added) is
    selected so a highlighted phrase inside a highlighted paragraph is
    easy to pick.
  - Double- and triple-click with the select tool on or `Shift` held still
    select a word or line as a fresh text selection, even inside an
    existing highlight.
- **Adjusting a selected highlight**:
  - A selected highlight shows the same two handles (stem and knob at
    start and end) as a text selection, without drawing the blue selection
    fill over the highlight.
  - Dragging either handle (with edge scrolling) or `Shift`+clicking moves
    that end across the text and updates the highlight on the page live.
    Releasing a handle drag (or `Shift`+clicking) commits the new
    `(start, end)` as one undo step. If a handle drag collapses the range
    so no characters remain between the ends, the highlight keeps its
    range from before the drag.
  - The bar at the selected highlight's end shows `Copy`, the five colour
    swatches (with the highlight's current colour marked), and a delete
    button. It hangs from the point the highlight was clicked at when a
    click selects it, and from the focus caret's line otherwise.
  - Clicking a colour swatch sets the highlight's colour (recording an
    undo step if the colour changed) and keeps it selected, so the bar
    stays open.
  - Clicking the delete button, or pressing `Backspace` or `Delete` while
    no widget has keyboard focus, deletes the selected highlight (one undo
    step) and clears the selection.
  - `Mod+C` copies the selected highlight's text, just as for a fresh text
    selection.
  - `Esc`, a click on the page outside any highlight, starting a new
    selection drag, or reloading/switching the document deselects the
    highlight. Panning and zooming keep it selected.
- **Undo and redo**:
  - `Undo` (`Mod+Z`) and `Redo` (`Mod+Shift+Z`, plus `Ctrl+Y` on Linux and
    Windows) undo and redo adding, recolouring, resizing, and deleting
    highlights, up to `UNDO_LIMIT` (100) steps.
  - `Undo` and `Redo` also appear in the application menu.
  - Neither `Undo`/`Redo` nor `Backspace`/`Delete` acts while a text field
    or other widget has keyboard focus, so the search field keeps its own
    editing keys.
  - Undoing or redoing clears any active selection.
  - The undo and redo stacks live in memory for the open session.
- **Storage**: highlights are saved in `noido-state`'s per-document
  `<hash>.toml` as part of `DocState`.

## Non-goals (saved for later steps)

- Keyboard shortcuts for creating highlights.
- Notes, tags, regions, and point notes.
- The right sidebar.
- Quote- and fuzzy-based re-anchoring when a PDF is edited in place.

## Anti-goals

- Highlights shifting with zoom or layout: all positions are page and
  character indices (`Caret`), converted to page-point rectangles through
  `PageText::rects`.
- Surprising splits when highlighting a word inside an already-highlighted
  paragraph.

## Approaches

1. **One selection/handle controller for both fresh text and a selected
   highlight (chosen).**
   In `noido-core`, `Highlights` holds the document's highlights plus
   snapshot-based `undo` and `redo` stacks. In `gui-egui`, `select::State`
   tracks whether the current `Selection` is a fresh text selection or is
   editing a selected highlight. Both share caret/handle placement, handle
   dragging, edge scrolling, bar positioning, `Mod+C`, and `Esc`/click
   clearing.
2. **Separate highlight-editing state alongside `select::State`.**
   Clean separation on paper, but duplicates handle placement, hit-testing,
   edge scrolling during handle drags, pointer grab rules, and bar
   positioning.

## Design

### Model and undo/redo (`noido-core`, new `highlights.rs`)

- `HighlightColour`: `Yellow`, `Green`, `Blue`, `Pink`, `Purple`, with
  `HighlightColour::ALL: [HighlightColour; 5]`.
- `Highlight`: `start: Caret`, `end: Caret` (`start < end`), and
  `colour: HighlightColour`.
  - Helper `on_page(&self, page: usize, char_count: usize) -> Option<Range<usize>>`,
    sharing the same clamping logic as `Selection::on_page`.
- `Highlights`:
  - Holds `items: Vec<Highlight>`, `undo: Vec<Vec<Highlight>>`, and
    `redo: Vec<Vec<Highlight>>`, capped at `UNDO_LIMIT = 100`.
  - `from_items(items: Vec<Highlight>) -> Self` filters out any malformed
    entry with `start >= end` and starts with empty `undo`/`redo`.
  - Mutations (`add`, `set_colour`, `set_range`, `remove`) push the
    previous `items` snapshot onto `undo` and clear `redo` only when the
    mutation actually changes `items`. `add` appends and returns the new
    highlight's index.
  - `undo(&mut self) -> bool` and `redo(&mut self) -> bool` move the
    current `items` to the opposite stack and restore the popped snapshot.
  - `at_point(&self, p: DocPoint, text: &PageText) -> Option<usize>` finds
    the highlight covering `(p.x, p.y)` on `p.page` (tested against
    `text.rects(range)` for each highlight on that page). When several
    match, it picks the one with the smallest `(end.page - start.page,
    end.char.saturating_sub(start.char))`, breaking ties by highest index
    (most recently added).

### Keymap and menu (`noido-core`)

- `Key` gains `Z` and `Y`.
- `Action` gains `Undo` and `Redo`.
- `Keymap::native()` binds:
  - `command(Key::Z)` → `Action::Undo`
  - `shift(command(Key::Z))` → `Action::Redo`
  - `command(Key::Y)` → `Action::Redo` (second binding, so menus show
    `Ctrl+Shift+Z` / `⌘⇧Z`).
- `Keymap::action(press, editing)` does not return `Action::Undo` or
  `Action::Redo` while `editing` is `true`, leaving `Mod+Z`,
  `Mod+Shift+Z`, and `Mod+Y` to the focused text field.
- `menu()` gains an `[Undo, Redo]` section after `[Select text]`.

### Storage (`noido-state`)

- `DocState` gains `pub highlights: Vec<Highlight>`.
- `Entry` in `noido-state` serialises `highlights` as
  `#[serde(default, skip_serializing_if = "Vec::is_empty")] highlights: Vec<StoredHighlight>`,
  where `StoredHighlight` has `start_page: usize`, `start_char: usize`,
  `end_page: usize`, `end_char: usize`, and `colour: StoredColour`
  (`#[serde(rename_all = "kebab-case")]`).
- Files written before highlights existed deserialise with an empty list;
  entries with `start >= end` are dropped on load.

### Window (`gui-egui`)

- `NoidoApp` holds `highlights: Highlights`, restored in `with_persistence`
  from `DocState::highlights` and written out in `doc_state()`.
- `select::State` gains `editing: Option<usize>`, the index in
  `Highlights::items()` of the currently selected highlight (`None` when
  the selection is a fresh text selection or nothing is selected):
  - `clear(&mut self)` also resets `editing = None`.
  - When `editing` is `Some(idx)`:
    - While a handle drag is under way, the highlight at `idx` is drawn
      using the live `self.select.selection` range instead of its stored
      range (so the reader sees the highlight itself resize live), and the
      blue selection fill is not drawn.
    - When a handle drag releases, if the new `selection` is non-empty and
      has `carets() == Some((start, end))`, `highlights.set_range(idx,
      start, end)` commits it; otherwise `self.select.selection` is reset
      back to the highlight's stored `start..end`.
    - When `Shift`+click extends the selection, the new `carets()` are
      immediately committed with `highlights.set_range(idx, start, end)`.
  - On a plain click (not a drag, not double/triple-click, not `Shift`
    extending an existing selection) on a page:
    - If a link is under the pointer and neither `Shift` nor the select
      tool is active, the link is followed and any selection is cleared.
    - Otherwise, if `highlights.at_point(p, text)` finds a highlight `idx`,
      `editing` becomes `Some(idx)` and `selection` becomes its
      `start..end` (and `clicked_link` is not triggered).
    - Otherwise, `selection` and `editing` are cleared.
  - Starting a new `DragKind::Select` drag sets `editing = None`.
- **The bar** (`selection_bar` in `app.rs`):
  - Shown whenever `self.select.selection` has placed carets in the
    viewport and no drag or modal page is active.
  - Hangs from the click that selected the highlight (`bar_doc`, kept in
    document coordinates and projected each frame, so it follows the
    text when the view moves), and from the focus caret's line for a
    fresh selection.
  - Contains `Copy`, then five circular colour buttons (yellow, green,
    blue, pink, purple; when `editing == Some(idx)`, that highlight's
    colour button is drawn with a ring around it), and a delete (`×` or
    trash) button. The delete button is always drawn — disabled while
    the bar belongs to a plain text selection — so the bar keeps its
    width when a highlight is made or selected.
  - Each colour button names its colour in a tooltip above the bar.
  - Clicking a colour button:
    - When `editing == Some(idx)`: calls `highlights.set_colour(idx,
      colour)` and leaves the selection and `editing` as they are, so the
      bar stays open.
    - When `editing == None`: if the selection is non-empty with
      `carets() == Some((start, end))`, calls `highlights.add(Highlight {
      start, end, colour })` and sets `editing` to the new highlight's
      index, keeping the selection, so the bar stays open on it.
  - Clicking the delete button calls `highlights.remove(idx)` and clears
    the selection.
- **Keys**:
  - `Backspace` and `Delete` when `!editing && !self.select.widget_focused
    && !egui::Popup::is_any_open(ctx)` and `self.select.editing ==
    Some(idx)` call `highlights.remove(idx)` and clear the selection.
  - `Action::Undo` and `Action::Redo` call `highlights.undo()` /
    `highlights.redo()` and clear the selection.
  - `Esc` clears both `selection` and `editing`.

## Tests

- **Core (`highlights.rs`, `keymap.rs`, `menu.rs`)**:
  - Adding, recolouring, resizing, and removing highlights.
  - `on_page` for single-page and multi-page highlights.
  - `at_point` hit-testing on a line rectangle, missing outside it, and
    picking the shorter highlight when two overlap (and the newer one on
    a tie).
  - `undo` and `redo` across add, recolour, resize, and remove; no-op
    mutations (same colour, same range) do not push an undo step; new
    mutation after undo clears redo; `UNDO_LIMIT` cap.
  - `Mod+Z`, `Mod+Shift+Z`, and `Mod+Y` map to `Undo` and `Redo` when not
    editing, and return `None` when `editing` is `true`.
- **State (`noido-state`)**:
  - Round-trip of `DocState` with single-page and multi-page highlights of
    every colour.
  - Old state files without `highlights` load with `highlights: vec![]`.
- **Harness (`gui-egui`)**:
  - Selecting text and clicking a colour swatch in the bar creates a
    highlight and selects it, leaving the bar open.
  - Clicking an existing highlight selects it (handles and bar appear,
    the bar at the click) even with the select tool off; clicking
    outside clears it, and the bar follows the line when the view
    scrolls.
  - Dragging a selected highlight's handle resizes it, and `Mod+Z` /
    `Mod+Shift+Z` undoes and redoes the resize.
  - Clicking another colour swatch on a selected highlight changes its
    colour and keeps it selected; clicking `Copy` copies the text and
    keeps the bar.
  - Pressing `Backspace`/`Delete` or clicking the bar's delete button
    removes the selected highlight; `Mod+Z` brings it back.
  - `Mod+C` with a highlight selected copies its text.
  - Overlapping highlights: clicking the inner word selects the shorter
    highlight.
  - Highlights persist through `Store` when the app saves state.

## By hand

On a paper and an RPG rulebook: select a sentence and click a colour in
the bar; select a word inside that sentence and highlight it in another
colour; select across a page break and highlight it. Click each highlight
to see its handles and bar; drag a handle to grow or shrink it; change its
colour from the bar; copy its text with `Mod+C`; delete one with
`Backspace` and bring it back with `Mod+Z`, then `Mod+Shift+Z`. Close and
reopen the document and check the highlights are still there.

## Left to the builder

The exact RGBA values and opacity for the five highlight colours in light
and dark themes; the size and spacing of the colour swatches and delete
icon in the selection bar; the colour of the handles when a highlight is
selected.
