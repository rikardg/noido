# Keep the nearest history targets sharp

Date: 2026-10-31 · Status: small change, agreed in conversation

## Intent

Jumping in history shows the stretched thumbnail until the sharp render
arrives: the keep window is centred on where the reader landed and drops
the page they jumped from. Back and Forward should find their targets
already rendered at the page view's scale. This generalises the hit
prefetch: the places a jump would go to are kept sharp, for Back and
Forward as well as for search's next and previous hit.

## Approaches (agreed: 1)

1. **Keep the nearest `HISTORY_KEEP` place on each side, recomputed every
   frame.** The stacks change only on a jump, so recomputing from them is
   "refresh on each jump": targets leave the keep set when the stacks
   shift, and the cache rebalances without extra bookkeeping. Chosen.
2. Rolling replacement: advance to the next target and drop the previous
   one. Rejected: dropping the immediate Back/Forward target brings the
   glitch back.
3. Wait for the current page to render before requesting the extra
   targets. Rejected for now: the render order already serves the current
   page first, and the gate needs a `RenderTracker` readiness query and
   fill state for no gain yet. Revisit if the extra targets compete with
   the window.
4. "Jump when sharp": move the view only once the render arrives.
   Rejected earlier: it defeats the low-resolution rendering that keeps
   the view snappy.

## Behaviour

- The nearest `HISTORY_KEEP = 1` entry of each stack: the next Back and the
  next Forward target. After a Back, the Forward target is the page just
  left; after a link, thumbnail or search jump only the Back side exists.
- Each target is expanded to its whole spread in dual/paged mode, so a
  spread is never half rendered.
- Target pages are kept wherever they are, as the hit targets are: added
  to the render order and to the keep set when outside the window. Order:
  visible, hit targets, history targets, then the window's spreads. One
  place each side delays the window's neighbours by at most a couple of
  renders.
- Pages past the page count are dropped: a saved history entry from a
  longer version must not be requested. Targets already visible or in the
  hit set are not repeated.
- Nothing changes without history: opening and scrolling leave the stacks
  empty. Scrolling adds no entries, so targets stay kept while the reader
  reads on. The next jump shifts the stacks and evicts targets that are no
  longer nearest, as every page outside the keep set is evicted each frame.

## Code shape

- `app.rs`: a constant `HISTORY_KEEP` beside `PREFETCH_BUDGET` and `KEEP`;
  a block in `pages()` next to the hit-page collection that reads the
  stacks' last entries, expands to spreads, filters and appends to `ahead`.
  `ahead` already feeds both `order` and `extra`, so nothing else changes.
- No core changes; no `RenderTracker` query; no direction field in v1. If
  the wrong side wins the render thread, `go_back`/`go_forward` can record
  the last jump direction and order that side first later.
- `History` needs no API change: the nearest entries are the stacks' last.

## Testing

- Failing first, mirroring the hit tests. A 200-page document with a link
  on page 0 to page 150: after following it and reading on past the keep
  window, page 0's full render is still kept, and Back lands on it sharp.
- After a Back to page 0, page 150 (the Forward target) is kept, and
  Forward lands there sharp.
- Dual mode keeps the target's whole spread.
- With no jump, only the window is rendered (the existing test stays).
- A reload to fewer pages with a longer saved history leaves the view in
  place and does not panic. Layout clamping already drops the stale page's
  spread, so this guards the reload-and-history path end to end rather than
  the page-count guard itself. The guard's only extra effect — a stale
  entry's spread keeping an in-range neighbour in dual mode with
  odd-pages-left off — stays untested and defensive.
