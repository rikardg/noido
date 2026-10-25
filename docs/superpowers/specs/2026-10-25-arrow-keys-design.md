# Arrow keys scroll like the wheel

Date: 2026-10-25 · Status: small change, agreed in conversation

## Intent

The arrow keys do nothing today. Make them move the view the way the mouse
wheel does: a small scroll when there is room to scroll, a page (or spread)
flip when the page fits the window in paged mode.

## Approaches (my call: 1)

1. **New actions in the keymap, carried out by `View`.** `ScrollUp`,
   `ScrollDown`, `ScrollLeft`, `ScrollRight`, bound to the arrows; `View`
   flips when `wheel_flips()` holds, else scrolls one step. Testable in
   `noido-core` without egui. Chosen.
2. Feed the arrows into the app as synthetic wheel steps: reuses
   `wheel_flip`, but mixes keys into wheel state (trackpad accumulation)
   and keeps the logic in the frontend.

## Behaviour

- **Up / Down:** where the wheel flips (paged mode, spread fits the window
  vertically), flip to the previous / next spread. Elsewhere scroll 40 pt,
  egui's scroll per wheel line on native, clamped at the ends; like the
  wheel, a tall page in paged mode scrolls within itself and doesn't flip.
- **Left / Right** (my call): where the wheel flips, flip to the previous /
  next spread, as readers expect from left and right. Elsewhere scroll
  40 pt sideways when the page is wider than the window, else nothing.
- Key repeat repeats the step. No modifiers: Shift/Ctrl + arrow stay
  unbound.
- While typing in the search field the arrows move the cursor, as now.
- Counts as the reader moving (like Page Up/Down): the search's "next from
  where the reader is" follows.

## Code shape

- `noido-core/keymap.rs`: `Key::ArrowUp/Down/Left/Right`, labels "↑ ↓ ← →",
  the four actions and bindings; not acting while editing.
- `noido-core/view.rs`: `SCROLL_STEP = 40.0`; `apply` handles the four.
- `gui-egui/app.rs`: `core_key` maps egui's arrows.

## Testing

- Keymap: arrows map to the actions, not while editing.
- View: down/up scroll by the step and clamp; flip when the page fits;
  tall page in paged mode scrolls; left/right flip when fitting, scroll
  sideways when zoomed in, nothing when the page fits the width.
- Harness: an arrow press moves the view.
