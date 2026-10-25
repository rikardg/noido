# Drag to pan

Date: 2026-10-25 · Status: agreed in conversation (small change)

## Intent

Dragging with the left mouse button moves the document in x and y.

## Decisions

- Left-button drag pans for now. Selection tools (text, region, highlight)
  come later and will need a tool mode that turns panning off.
- **Approach A (chosen):** enable egui's built-in drag-to-scroll for the mouse
  (`ScrollSource { drag: DragScroll::Always, .. }`), keeping egui's momentum
  and edge behaviour. Core picks up the new offset through the existing
  per-frame sync (`View::scrolled_to`).
  Rejected: panning in core (`View::pan`) — would rebuild momentum that egui
  already provides; revisit if tool modes need it.

## Testing

Headless app test: zoom in so the content is wider than the window, drag
up-left with simulated pointer events, and check the view's offset moved
down and right.
