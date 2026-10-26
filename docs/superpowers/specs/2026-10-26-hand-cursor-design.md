# Hand cursor over the pages

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

A left drag pans the document; the pointer should say so.

## Behaviour

- Over the page area (pages and the background around them): the open hand
  (`CursorIcon::Grab`).
- While a drag that started there is under way: the closed hand
  (`CursorIcon::Grabbing`).
- Elsewhere unchanged: title bar, sidebar, the settings page and menus show
  their own cursor; the window's resize edges keep their arrows.

## Code shape

- `gui-egui/app.rs`, after the page `ScrollArea`: if the pointer is over its
  inner rect on that layer (nothing above it) and no cursor was set this
  frame (resize edges), set Grab, or Grabbing while the primary button is
  down from a press inside it.

## Testing

Harness records the frame's cursor: hover the pages → Grab; mid-drag →
Grabbing; title bar → Default; settings page open → not a hand; a resize
edge → its resize cursor.
