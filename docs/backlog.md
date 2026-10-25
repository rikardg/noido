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


## Deferred features

- **Automated interactive testing** of the egui frontend: drive key presses,
  wheel/pinch zoom, scrolling and resizing headlessly and check the result
  (scroll offset, zoom, rendered output). Candidate: `egui_kittest`, which runs
  the app without a desktop session. Do not fake input on the real desktop —
  on GNOME that triggers a remote-desktop permission prompt.

- Vim key layer (zathura-style), with key sequences and counts.
- hayro render cache reused across renders (needs a self-referential holder
  for `Pdf` + `RenderCache`).
- Tiled rendering, LRU eviction, prefetch priorities.
