# PageUp / PageDown scrolling

Date: 2026-10-25 · Status: agreed in conversation (small change)

## Intent

First key bindings, and the seed of the keymap from the overall design:
PageDown / PageUp scroll the document by one screenful.

## Decisions

- **Approach A (chosen):** actions and keymap live in `noido-core` as data; the
  frontend translates toolkit keys to core keys, asks the keymap for an action,
  and applies the scroll offset core computes.
  Rejected: handling the keys directly in the egui frontend (quicker, but puts
  logic in the UI crate against the overall design, and would move later).
- **Behaviour (my defaults, accepted):**
  - One screenful = visible height minus a 10% overlap.
  - Clamped to the top and bottom of the document.
  - Key repeat scrolls repeatedly.
  - Keys are ignored while a text field has focus.
- Vim layer deferred (`docs/backlog.md`).

## Non-goals

Modifiers, key sequences, other actions, user overrides, menu.
