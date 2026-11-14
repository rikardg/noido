# Tiled rendering: what tiles, caches and threads cost

Date: 2026-11-14

Status: measured. The shared render cache (#67) is done; tiling waits for
image caching in hayro (hayro PR #1672). The plan is in issue #90.

## Question

Tiled rendering would lift the 400% zoom limit (`MAX_ZOOM`, set by
`MAX_RENDER_SIDE` = 8192 px) and bound memory at high zoom. Which shape
should it take, and what does a tile cost against a whole page? Would
threads or a shared interpretation make tiles cheap?

## Answer

- With hayro 0.9, a tile costs nearly as much as the whole page on any
  page whose cost is an image (scans, figures embedded as big raster
  images): hayro decodes and resamples each image on every render.
  Nine tiles cost 7–9× one full render. On text and vector pages a tile
  costs 25–90% of the page; nine cost 1.2–6×.
- With hayro PR #1672 (images cached in the `RenderCache`, as drawn) and
  one cache shared across renders (#67), nine tiles at one zoom cost
  about one full render (0.9–1.2×) on every kind of page. The first tile
  pays for the decode; the rest reuse it.
- Threads don't make tiles cheap. Tiles of one page on separate threads,
  each with its own document and cache, were slower than one full render
  on scans and text (0.3–0.9×), because every thread decodes the same
  images. Only vector-heavy art at high zoom gained (about 3× on 9
  threads).
- vello_cpu's own multithreading (one render, several rasteriser
  threads) gives 1.0× at fit width and 1.2–1.46× at scale 6–12 on vector
  pages; nothing on image pages.
- Interpreting a page once and rasterising tiles from a recording (option
  D below) isn't offered by hayro or vello_cpu 0.3 (vello_cpu's recording
  is private and in pixel space). It would need our own display list via
  a recording `Device`. Its main gain, decoding images once, is what PR
  #1672 gives.

## How it was measured

A throwaway benchmark, kept here as `2026-11-14-tiled-rendering/tilebench.rs`
(with its manifest as `Cargo.toml.txt`; it isn't part of the workspace).
It renders like `engine-hayro` does, with the cache, the tile grid and the
thread count as options. Modes: `page` (one page: full, tiles, threads),
`run` (a run of pages, fresh against shared cache), `zoom` (cache growth
through zoom steps), `scan` (find slow pages), `info` (content and
images on a page). Run on a Mac (12 cores), release build, on files in
`../testfiles/`. "Scale" is pixels per point: 2 is about fit width on a
2× display, 6 about 300%.

Pages used: a text-heavy book, an art-heavy RPG book (vector art), two
scanned books (JPEG and JPEG 2000), and the network analysis paper, whose
"vector" figures are one large Flate-compressed raster image per page
(2879×7000, 60 MB decoded on its slowest page, 33 ms just to inflate).

## Numbers

### hayro 0.9 (what noido uses)

One page, nine tiles (3×3) rendered one after another, against one full
render:

| Page | Scale 2 | Scale 6 | One tile, scale 6 |
|---|---|---|---|
| Text book | 4.3–6.5× | 3.4–4.5× | 58–67% of full |
| Art book (vector) | 2.0–2.4× | 1.2–1.5× | 26–30% |
| Scan (JPEG) | 8.6–9.0× | 7.7× | 82–86% |
| Scan (JPEG 2000) | 8.2–8.9× | 7.5–7.9× | 84–86% |
| Network paper p11 (raster figure) | 6.8–8.8× | 7.4–7.9× | 79–86% |

The network paper's page 11 takes 46 ms even rendered at 30×43 px: the
cost doesn't depend on zoom, it is the image.

Shared `RenderCache` (#67), 40–200 pages in a row at one zoom: text book
44–51% faster, art books 4–24%, scans 0%. Its memory grows about 0.4 MB
per page on the art book without levelling off (84 MB after 200 pages),
and levels off at about 7 MB on the text book; through ten zoom steps
on one art page it reached 24 MB. Hence #67's reset rules (a zoom change
beyond 1%, or 64 renders).

### hayro PR #1672 against its base commit

Measured on a quiet machine; the base is upstream `main` at the commit
the PR branches from (8558a8a). "Again" is rendering the same page a
second time with the cache shared, as a share of the first render.

| Page | Scale | Again, base | Again, PR | 3×3 tiles, base | 3×3 tiles, PR | Cache |
|---|---|---|---|---|---|---|
| Network p11 | 2 | 79% | 4% | 7.1× | 1.0× | 2.7 MB |
| Network p11 | 6 | 99% | 20% | 7.9× | 1.0× | 23 MB |
| Network p12 | 6 | 107% | 30% | 4.5× | 1.0× | 16 MB |
| Scan, JPEG 2000 | 6 | 98% | 22% | 7.7× | 1.0× | 41 MB |
| Scan, JPEG | 2 | 108% | 2% | 8.6× | 1.0× | 5.6 MB |
| Scan, JPEG | 6 | 100% | 16% | 7.7× | 0.9× | 25 MB |
| Art book | 6 | 98% | 83% | 1.2× | 1.0× | 7.4 MB |
| Text book | 6 | 86% | 36% | 3.4× | 1.0× | 13 MB |

Tiles on 9 threads, PR: 0.3–0.9× on all pages except the art book at
scale 6 (2.9×): each thread's own cache decodes the images again.

The PR caps its image cache at 256 MiB (least recently used out) and
doesn't cache a single image over 64 MiB. A scan page at scale 6 holds
about 25–41 MB.

## The options considered

- **A. Whole pages normally; above a pixel budget, a capped full-page
  backdrop plus tiles of the visible part**, keyed by (page, scale,
  column, row). No change at normal zoom; lifts the zoom limit; bounded
  memory. Two drawing paths, and blur at the edges while panning.
  Chosen, once images are cached in hayro.
- **B. Tiles always**, fixed size. One path, memory budgeted in MB,
  visible tiles first, smaller texture uploads (#43). Without image
  caching every tile pays a full decode, and seams between tiles need
  care (the light-edge problem of the whole-pixel spec). Possible later
  on top of A's tile cache, if A's two paths prove a burden.
- **C. Render only the visible part, as one image**, again when
  scrolling stops. Smallest change; no reuse between views. A fallback
  if image caching doesn't land.
- **D. Interpret once, rasterise tiles from a recording.** Not offered
  by hayro or vello_cpu; would need our own display list. Its main gain
  comes with PR #1672.
