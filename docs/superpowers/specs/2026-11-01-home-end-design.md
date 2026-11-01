# Home and End go to the first and last page

Date: 2026-11-01 · Status: small change, proposed

## Intent

Home and End do nothing today. Make Home show the first page and End the
last, without changing the zoom or the page arrangement.

## Approaches (my call: 1)

1. **New actions in the keymap, carried out by `View`.** `Action::FirstPage`
   and `Action::LastPage`, bound to plain Home/End; `View::apply` uses the
   existing `go_to_page`; the GUI records the place for Back before the jump.
   Testable in `noido-core` and in the harness. Chosen.
2. Translate Home/End straight to `View::go_to_page` in `app.rs`, skipping
   the keymap. Fewer lines, but the keymap stops being the one list of keys,
   there is no action for a label or a future menu, and core has no test.

## Behaviour

- **Home** goes to page 0, **End** to the last page.
- Continuous mode: the target page's top goes to the top of the viewport, as
  far as scrolling allows. Home lands at the start of the document. End lands
  on the last page from its top; only when the page is too short for its top
  to reach the viewport top does the scroll stop at the document's end
  instead. Paged mode: the spread containing that page is shown from the top.
- Zoom and arrangement (continuous, dual, odd pages left) stay as they are,
  as for links and thumbnails.
- **They add a history entry:** the place left is remembered, so Back
  (Alt+←) returns there. A press that moves nothing, such as Home at the
  first page, adds none.
- Like other page moves, the jump clears a page chosen in the sidebar; the
  page at the centre is current again.
- Plain Home/End only. While typing in the search field they keep moving the
  caret (start/end of the text) and do not jump; the existing
  `acts_while_editing` rule gives this without new work.
- Not in the menu, like Page Up/Down.

## Code shape

- `noido-core/keymap.rs`: `Key::Home`, `Key::End`, labels "Home" and "End";
  `Action::FirstPage`, `Action::LastPage`, and the two bindings.
- `noido-core/view.rs`: `apply` carries the two actions out with
  `go_to_page(0)` and `go_to_page(page_count - 1)`.
- `gui-egui/app.rs`: `core_key` maps egui's Home and End; `apply` records the
  place for Back when the jump moves the view.
- No changes to state files, settings or menus.

## Testing

- Keymap: Home → `FirstPage`, End → `LastPage`; no binding while editing;
  labels "Home" and "End".
- View: continuous Home lands at the document top, End at its end; paged Home
  shows the first spread and End the last, both from the top; zero pages safe.
- Harness: Home and End move the view; Back returns to where they were
  pressed.

## Non-goals

- Ctrl+Home / Ctrl+End, Shift+Home / Shift+End.
- The media-key named actions in the backlog (`FirstPage`, `LastPage`).
- The screen-edge flash minor for moves other than Page Up/Down (backlog,
  2026-10-26): Home/End leave a fading edge alone, as links do.
