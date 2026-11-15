# Page textures within a byte budget

Date: 2026-11-14 · Status: small change, to be agreed

## Intent

What is kept around the reader is counted in spreads (`KEEP = 8`,
`PREFETCH_BUDGET = 10`), but a spread's cost is `page size × display
scale² × 4` bytes, which depends on the page and on the window. So the
same `KEEP` holds 14–18 MB at a 900×1100 window and 41–51 MB a page at
1512×982 with a Retina backing store: Lazarus holds 458 MB of page
textures when it opens maximised and 763 MB after a read-through, for
106 pages of 477×733.5 pt.

Cap the kept textures by bytes instead: the reader keeps what fits, and
what doesn't fit is dropped farthest-first, to be rendered again when it
comes back.

## Measured

- Lazarus (106 pp, 80 ms a page): 458 MB of page renders when opened at
  a maximised window, 763 MB after reading through.
- building-git (737 pp, 2 ms a page, `pdl` text): 279 MB of page renders
  at a default window, for pages that cost 4 ms each to make again.
- Per page at the display scale: 14–18 MB at 900×1100 pt, 41–51 MB at
  1512×982 pt with `pixels_per_point` 2.
- An alternative — prefetching off-screen pages at half scale — was
  built and rejected; see the last section.

## Approach

- **A budget in bytes** for kept page renders: `PAGE_TEXTURE_BUDGET =
  256 MB`. It is a cap on what is *kept*, not on what is requested.
- **Kept by distance**: spreads around the reader, nearest first, taken
  until the budget is spent; the ones that don't fit are forgotten
  exactly as pages outside the keep window are forgotten today
  (`RenderTracker::forget_outside`, `textures.retain`). Because the
  window is already biased ahead of the reader (six ahead, three behind,
  flipped with the direction of travel), the budget spends itself on the
  spreads the reader is most likely to see next.
- **Never evicted**: the visible spread and the one beside it. They are
  the two the reader is looking at or about to; without that floor a
  budget smaller than one spread would blank the page on screen.
- **Prefetch follows the budget**: nothing is requested that would not be
  kept once it arrived, so a maximised window stops asking for ten
  spreads it cannot hold.
- **A cheap document keeps less.** A page that renders in under 10 ms
  (the median of the render thread's last twelve, a `Renderer` accessor)
  is better remade than kept: such a document keeps only the visible
  spread and the one beside it. building-git goes from 279 MB to the
  four pages on screen and beside them.
- Unchanged: the sidebar's sharp thumbnails keep their own fixed budget
  (`THUMBNAIL_BUDGET`), and hit and history pages keep the same
  treatment they have now.

## Tests

- A maximised harness window (1512×982 at `pixels_per_point` 2) with a
  budget that fits three spreads: exactly the three nearest are kept, the
  rest are dropped, and the visible spread and its neighbour survive a
  budget smaller than themselves.
- Dropped farthest-first: with a budget that fits `n` spreads, the kept
  set is the `n` nearest, and no farther page is among them.
- The default harness window (900×1000) keeps all of `KEEP` when the
  budget allows it, so nothing about today's behaviour changes where the
  pages are small.
- A page that renders in milliseconds (`RecordingEngine`) keeps only the
  visible and beside spreads; one that takes 15 ms a page
  (`SlowRecordingEngine`) keeps the full window. Both exist in the
  harness today.
- Nothing outside the budget is requested: the pages the recorder saw
  are a subset of the kept ones.

## What the soft-prefetch prototype showed

Built and dropped (`t3-prototype`): off-screen pages rendered at half
the display scale, sharpened when read. Two things were measured.

- A half-scale render stretched back to full is **3.3% more ink** than
  the full-scale one on vector text (0.16 px of shift, so alignment is
  not the cause; scans are unaffected). Sharpening therefore reads as a
  small contraction, which no fade hides — macOS Preview shows the same
  thing for the same reason.
- The stand-in saves memory only while it *rests*; once the full render
  follows, the window converges to full renders and the saving is gone
  (Lazarus 190 MB with a resting soft tier, 285 MB without). Those two
  are mutually exclusive, and the resting variant is the visible one.

What survives is the cost measurement: the render thread reports the
median of its last twelve page renders, which is what the cheap-document
rule above uses.
