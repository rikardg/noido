# Backlog

Known issues and deferred work, roughly in priority order. Remove an entry
when it is fixed.

## Known issues

- **Render-thread panic leaves the window blank.** If the engine panics on a
  page, the render thread dies silently and every page stays a white
  placeholder. Wrap `render_tile` in `catch_unwind` and report it as
  `EngineError::Render`. (`crates/noido-render/src/lib.rs`, `run`)
- **Zero-page PDF opens a blank window.** hayro also reports 0 pages for some
  malformed page trees. Treat it as an error ("no pages") before the window
  opens. (`crates/frontends/gui-egui/src/main.rs`)
- **Failed page looks like a loading page.** Both draw as a white rectangle.
  Draw failed pages differently, e.g. grey fill or an outline with a message.
  (`crates/frontends/gui-egui/src/app.rs`)
- **Raw error text for unreadable files.** A non-PDF shows
  `not a readable PDF: Invalid` (Debug output of hayro's `LoadPdfError`). Map
  errors to readable messages, with a distinct one for encrypted PDFs.
  (`crates/engine-hayro/src/lib.rs`)
- **Render size cap is hardcoded.** `MAX_RENDER_SIDE` (8192) matches
  egui-wgpu's texture limit today; use
  `min(MAX_RENDER_SIDE, ctx.input(|i| i.max_texture_side))` so another backend
  cannot exceed its limit. (`crates/frontends/gui-egui/src/app.rs`)
- **Downscaled capped pages alias.** Very tall pages rendered at the 8192 cap
  and shown smaller use linear filtering without mipmaps. Rare; revisit with
  tiled rendering.


- **Viewport size lags the window by one frame.** On resize the content is
  laid out for the old width for one frame (brief horizontal scrollbar or
  off-centre pages). Take the size from `ui.available_rect_before_wrap()`
  before showing the scroll area. (`crates/frontends/gui-egui/src/app.rs`)
- **Minimizing can lose the position.** Some platforms report a 0×0 viewport
  while minimized; `View::set_viewport_size(0, 0)` refits to 0.1 and anchors
  at (0, 0). Ignore non-positive sizes after the first real one.
  (`crates/noido-core/src/view.rs`)
- **Alt+W / Alt+F trigger the plain-key bindings.** `KeyPress` only carries
  `command`; carry `alt` (and later `shift`) or skip presses with Alt.
- **Kinetic scroll velocity survives a zoom.** A coasting touch drag keeps
  going in the new content coordinates after a zoom. Low impact.

- **More egui-level tests for view modes.** The headless harness doesn't yet
  cover PageUp flipping into a page of a different height, or a mode toggle in
  fit-page mode. Needs a `BlankDocument` with configurable page sizes (use a
  static `Mutex`, not `thread_local`: the render thread opens the document).
- **Prefetched pages in paged fit-page mode render twice** when the next
  spread fits at a different scale: they are rendered at the current spread's
  scale, then again after the flip. Wasted work only.
- **Shift+C / Shift+D also toggle view modes** (only `command` is checked;
  same root as the Alt+W/F item above).

## View state storage (review minors, 2026-10-25)

- A failed or interrupted state write leaves a `<hash>.tmp-<pid>` file in
  `docs/` that prune never removes; no fsync before the rename.
- Window files without a state file (noido killed within 2 s of opening a new
  document) are never pruned.
- `main` asks for the window file before `load`, relying on the re-filing
  rename overwriting the copied window file; untested in that order. Calling
  `load` first would be clearer.
- Windows puts state in `%LOCALAPPDATA%\noido\data`, not
  `%LOCALAPPDATA%\noido` as the spec says; macOS folder is `noido`, not a
  bundle id.
- Two instances on one document can both write its window file (eframe's
  write isn't atomic); a broken file falls back to the default window.
- Path-fallback ties within the same second are picked arbitrarily.

## Thumbnails (review minors, 2026-10-25)

- `thumbnail_order` with budget 0 still returns the focus page (no caller
  passes 0).
- Thumbnails cost about as much CPU as full renders (hayro parses and decodes
  images at full cost regardless of output size) and compete with them right
  after a jump; on few-core machines "not delaying full renders" is best
  effort. Thread priority is a non-goal for now.
- `jump_shows_thumbnail_before_full_render` unblocks the gated render thread
  only by dropping the gate at the end; worth a comment.

## Deferred features

- **Automated interactive testing** of the egui frontend: drive key presses,
  wheel/pinch zoom, scrolling and resizing headlessly and check the result
  (scroll offset, zoom, rendered output). Candidate: `egui_kittest`, which runs
  the app without a desktop session. A minimal headless harness already
  exists (`Harness` in `crates/frontends/gui-egui/src/app.rs` tests: real app,
  fake engine, key events and window sizes via `RawInput`); extend it or move
  to `egui_kittest`. Do not fake input on the real desktop —
  on GNOME that triggers a remote-desktop permission prompt.

- Vim key layer (zathura-style), with key sequences and counts.
- hayro render cache reused across renders (needs a self-referential holder
  for `Pdf` + `RenderCache`).
- Tiled rendering, LRU eviction, prefetch priorities.
- Memory budget for full renders in megabytes instead of a spread count
  (`PREFETCH_BUDGET` 10, `KEEP` 8 today): a page is ~6 MB at 900 pt fit
  width but ~27 MB maximised and ~48 MB at 400 %, so 17 kept spreads can
  reach hundreds of MB. Evict farthest first past a limit.
- Softer-looking thumbnails: at 120 px they look blocky/blurry when
  stretched; 240 px was tried and judged not worth it. Ideas: a smoother
  upscale (e.g. bicubic or a blur pass, in a shader or on the CPU before
  upload), mipmapped sampling, or a fade from thumbnail to full render.
- Several full-render threads, each with its own `Document`, sharing the
  priority queue, so one slow page (150–900 ms for art-heavy books) doesn't
  hold up the next. Only if renders still feel slow after the priority fix.
- **Title bar on macOS, checked on a Mac:** the traffic-light inset (76
  points), centring the traffic lights in our taller bar (an empty
  `NSToolbar`, via `objc2`), translucency (`window-vibrancy`), and reading
  the system's double-click preference (zoom or minimise). The code builds
  for macOS but nobody has seen it run.
- **Our window frame on Windows, checked on Windows:** snapping, resize
  edges, the window buttons, Alt+F4.
- **GNOME's button layout:** GNOME can put the window buttons on the left
  (`org.gnome.desktop.wm.preferences button-layout`); we always put them on
  the right.
- **Notes in a right sidebar.** The left sidebar holds search results,
  thumbnails and the outline; the right one holds the reader's notes:
  bookmarks, freeform notes, lasso captures (a region of the page) and text
  selections. Later: combine these, e.g. a selection or lasso capture with a
  note attached, or a bookmark with a comment.
- **ActualText in hayro.** hayro reads `/Span <</ActualText …>>` but doesn't
  pass it to the device, so books typeset with ligature fonts lose letters in
  extracted text (Vaesen: ~3.5 % of words; no other test file uses it). See
  `docs/investigations/2026-10-25-text-extraction.md`. Upstream issue or PR,
  or wait for hayro; the text API should let a glyph run carry replacement
  text so this slots in later.

## Far future

- **Our own OCR** for scanned PDFs without a usable text layer, so search,
  selection and notes work on them too. Example: `vehicles-…-annas-archive.pdf`
  in testfiles is 600 dpi page scans; only the cover has an (invisible, poor)
  OCR layer. Options: run an OCR engine (e.g. Tesseract) in the background and
  keep the result in a sidecar, or suggest `ocrmypdf` to the user.
