# Render the neighbouring hits' pages ahead

Date: 2026-10-25 · Status: small change, agreed in conversation

## Intent

Stepping to a far hit (page 1 → page 200) shows the 120 px thumbnail until
the sharp render arrives, and the page left behind loses its render once it
is outside the keep window, so stepping back blurs too. While a search is
active, render ahead the pages that next and previous would go to.

## Approaches (agreed: 1)

1. **Render the next and previous hit's pages ahead.** Two extra renders, only
   while searching; covers stepping in either direction, not just back and
   forth. Chosen.
2. Keep the last few jumped-from pages sharp: general (jump history), but
   ~15 MB a page and a new far hit still blurs. Later, with Ctrl-o / Ctrl-i.
3. Faster first sharp image via tiles of the visible part. Later, with tiled
   rendering.

## Behaviour

- While there are hits, the pages of the hits that next and previous would
  go to (the same targets as the keys: from the current hit when the view is
  at it, else from the view) are rendered at the page view's scale, and kept,
  even outside the keep window. In dual mode, their whole spreads.
- Priority: after the visible pages, before the prefetch window (my call:
  stepping is the likely next move while searching).
- Nothing changes without a search, or when those pages are in the window
  already.

## Code shape

- `noido-core`: `Keep { window: Range<usize>, extra: Vec<usize> }` with
  `contains(page)` and `From<Range<usize>>`. `RenderTracker::forget_outside`
  and `Renderer::set_keep` take it (`impl Into<Keep>`), so existing range
  callers stay as they are.
- `app.rs`: the target choice of `step_hit` becomes `hit_target(forward)`;
  `pages()` adds the two targets' spreads to the keep set and the priority.

## Testing

- `Keep::contains` with window and extra; the tracker forgets outside both.
- Renderer: a page in `extra` outside the window is rendered and kept.
- Harness: after a search lands on page 0 with the next hit on page 150,
  page 150 is rendered without visiting it; stepping there and back keeps
  page 0's full render; without a search no page outside the window is
  requested.
