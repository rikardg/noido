# Low-res thumbnails while pages render

Date: 2026-10-25 · Status: draft for review

## Intent

When the reader moves faster than full-resolution renders arrive, for example
holding PageDown, pages show as white. Show a small, blurry but real rendering
of the page instead, so the reader always sees the page layout.

## Behaviour (agreed)

- **Thumbnails are small real renders**, not thumbhashes, kept in memory only.
- **Rendered in the background**, without delaying startup or full-resolution
  renders.
- **A sliding window of the 100 pages nearest the current page**, rendered
  nearest first. At either end of the document the whole budget goes to the
  one side.
- **Biased towards the direction of travel:** two pages ahead for every page
  behind.
- **Eviction with slack:** a thumbnail is dropped only when its page is more
  than 100 pages from the current one. Moving back and forth doesn't
  re-render.
- **Re-targeting on a jump:** after a jump, for example via the scrollbar, the
  window follows the new position at once. Work queued for the old position is
  dropped.

## Proposals (my calls)

- **Current page:** the page at the window centre, or the current page in paged
  mode.
- **Direction:** the direction of the last change of current page; downwards at
  start. It is not reset while standing still, since readers mostly continue
  the way they were going.
- **Bias order:** moving down gives +1, +2, −1, +3, +4, −2, … (the current page
  first). The window holds about 67 ahead and 33 behind, and when one side
  runs out at a document edge the rest goes to the other side.
- **Thumbnail size:** 120 px wide in physical pixels (constant
  `THUMBNAIL_WIDTH`); height follows the page. That is about 75 KB per page,
  at most 201 kept, around 15 MB.
- **Drawing priority per page:**
  1. the full-resolution image, possibly at an older scale (stretched, as
     today)
  2. otherwise the thumbnail, stretched to the page
  3. otherwise white
- **Failed thumbnails** stay white and are not retried until evicted. Telling
  a failed page apart from a loading one remains a backlog item.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. Second thread with its own `Document`, opened from the shared bytes; the thread orders work from the latest focus | Truly parallel with full renders; startup not delayed (opens in the background); one more parsed copy of the document | **Chosen** |
| B. Same render thread; thumbnails as low-priority jobs | No second parse; but a long full-resolution render still blocks thumbnails, which is exactly when they're needed | Rejected |
| C. Render all pages up front | Simplest; costs CPU and memory on large documents and is slow to follow a jump | Rejected (discussed) |

## Design

### `noido-core`: the window (pure)

- `Direction { Forward, Backward }`.
- `thumbnail_order(page_count, focus, direction, budget) -> Vec<usize>`: the
  `budget` pages nearest `focus` in the biased order above, focus first.
  Clamped at the document edges, with the leftover budget given to the other
  side.
- `thumbnail_keep(page_count, focus, budget) -> Range<usize>`:
  `focus − budget ..= focus + budget`, clamped.
- `THUMBNAIL_BUDGET = 100`.

### `noido-render`: `Thumbnailer` (new)

- `Thumbnailer::start(engine, bytes, page_sizes, waker) -> Thumbnailer`: spawns
  `noido-thumbnails` and returns at once. The thread opens its own `Document`
  from the same `PdfBytes` (`Arc`, no copy). If opening fails it stops
  quietly.
- `set_focus(page, direction)` sends the latest focus. `try_recv() ->
  Option<Thumbnail { page, result }>`.
- **Thread loop:**
  - Block until a focus arrives.
  - Compute the order, then render pages in that order that haven't been
    delivered.
  - Before each page, drain the channel to the newest focus. On a change,
    forget delivered pages outside the new `thumbnail_keep` and recompute the
    order.
  - When the order is done, block again.
- **Scale:** `THUMBNAIL_WIDTH / page_width`, a scale that doesn't depend on the
  display. A thumbnail is replaced by the full render once that arrives.

### Frontend (egui)

- Start the `Thumbnailer` next to the `Renderer` in `main` and pass it to
  `NoidoApp`.
- **Each frame:**
  - Work out the current page.
  - If it changed, update the direction and call `set_focus`.
  - Receive thumbnails into `thumbnails: HashMap<usize, TextureHandle>`.
  - Drop entries outside `thumbnail_keep`.
- **Drawing:** use the priority above.

## Non-goals

- Saving thumbnails to disk.
- A thumbnail sidebar.
- Lowering the thread's OS priority.
- Showing failed pages differently.
- Thumbnails for zoom (full renders at an old scale already cover zoom).

## Testing

- **`noido-core` unit tests for the order and window:**
  - nearest-first with the 2:1 bias in both directions
  - edges: page 0 and the last page put the whole budget on one side
  - a document smaller than the budget
  - zero pages
  - the keep range clamped at the document edges
- **`Thumbnailer` tests with a fake engine:**
  - renders in order from the focus
  - a new focus re-targets mid-way (gated engine)
  - delivered pages aren't rendered again
  - pages evicted and later back in range are rendered again
  - an engine that fails to open stops the thread without a panic
- **A headless app test:** after a jump far down, the visible page gets a
  thumbnail before its full render. A gated full-render engine makes this
  deterministic.
- **Checked by hand:** holding PageDown through `tracemonkey.pdf` and a large
  PDF.

## Left to the builder

- Exact names.
- How the thread drains the focus channel.
- Whether the frontend tracks thumbnail state or only the texture map.
