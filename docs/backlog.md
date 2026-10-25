# Backlog

Known issues and deferred work, roughly in priority order. Remove an entry
when it is fixed.

## Next up

- **Open from Nautilus (file manager integration).** Decided: app ID
  `noido`. Steps:
  1. A `noido.desktop` in `~/.local/share/applications/` (kept in the
     repo, with an install step): `Exec=/home/<user>/.cargo/bin/noido %f`
     (`~/.cargo/bin/noido` links to `target/release`),
     `MimeType=application/pdf;`, `Type=Application`, `Terminal=false`,
     `Categories=Office;Viewer;`. Then `update-desktop-database`, and
     `xdg-mime default noido.desktop application/pdf` to make it the
     default. Several selected PDFs open one window each; reopening a file
     brings its window forward (already works).
  2. Set the window's app ID to `noido` (`ViewportBuilder::with_app_id`
     in `main.rs`), so GNOME ties the window to the `.desktop` file;
     without it the dock shows a generic, unnamed icon.
  3. An icon (a stock document icon until noido has its own).
  4. Try by hand whether a window opened from Nautilus gets focus on
     GNOME Wayland: Nautilus passes `XDG_ACTIVATION_TOKEN`, but eframe
     doesn't hand it to the first window (winit has
     `read_token_from_env` / `with_activation_token`). The same token is
     what "Proper window activation on Wayland" below needs.

## Known issues

- **Try the LaTeX loop by hand (user).** Run `latexmk -pvc` (or repeated
  `pdflatex`) with noido open: it should follow each compile, keep the
  place, and survive a failing build.

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

## Title bar and sidebar (review minors, 2026-10-25)

- On the first frame with a restored sidebar wider than the window allows,
  the search field is sized from the unclamped width (clamp at restore, or
  draw the sidebar before the bar).
- A NaN sidebar width in a hand-edited state file is never saved over
  (NaN ≠ NaN, so the debounce restarts every frame) and wakes noido every
  2 s; sanitise on load.
- The comment on calling `frame::resize_edges` first is misleading: order
  doesn't keep presses from widgets underneath; the page view's 8 pt margin
  does.
- A double-click in the top 5 points both starts resizes and maximises.
- After a mouse click an icon button keeps focus, so Space or Enter presses
  it again.

## Reload and one window per file (review minors, 2026-10-25)

- A second launch while the first is still starting (reading the file,
  loading state, parsing) waits 1 s, gets no answer and opens a second
  window: the first only serves after `Renderer::start`. Serve right after
  `claim`.
- Two launches at the same moment can both become first: the second's
  `try_overwrite` unlinks the first's live socket. Retry `connect` once on
  `AddrInUse` before overwriting, or lock around the claim.
- A write between reading the file and `Watcher::start` is missed until
  the next change. Arm the debounce once at start (`open_new` skips an
  unchanged hash anyway).
- Watcher overflow or rescan events carry no paths, so they're dropped;
  treat `event.need_rescan()` as a change.
- Without `XDG_RUNTIME_DIR` the socket is `/tmp/noido-<hash>.sock`, which
  another local user can predict and squat (every launch on that file then
  exits silently). Use a per-user 0700 directory and check its owner.
- `prune` trusts the window name in a state file: a hand-edited
  `window = "../../x"` makes it delete `docs/../../x.window.ron`. Accept only
  64 hex characters, as `files_with` does.
- The socket file stays after a normal quit (the serving thread owns the
  listener); harmless, the next launch replaces it.
- Tests: no reload to fewer pages in paged or dual mode comparing the
  centre; the activation path isn't checked while minimised on Windows or
  macOS (eframe may not run `ui()` then).

## Sidebar thumbnails (review minors, 2026-10-25)

- A fling of the page view keeps gliding after a thumbnail click (egui keeps
  the scroll velocity when the offset is set), and the gliding then clears
  the clicked page. Reset the page view's scroll velocity after
  `go_to_page`. Keys during a fling already behave this way.
- A vertical window resize, or a reload to fewer pages, can re-clamp the
  sidebar's offset and count as browsing, moving the low-res window. Count
  a height change as a resize too.
- After a reload a kept sharp thumbnail is stretched to a page whose aspect
  changed until its new render arrives, and for good if the new sidebar
  renderer fails to open. Drop images whose page size changed.
- `reload_swaps_sidebar_renderer` is weak: only pages 0–2 are visible, so
  "sharp pages below 5" holds anyway, and it doesn't check the place is
  kept. Scroll the sidebar to pages 6–9 first.
- The wheel over the title bar no longer flips pages in paged mode (only
  over the page view does); fine unless someone misses it.

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
- **Reload notices and a status area.** Show "Reloaded" briefly after a
  reload, and "Reload failed — showing the previous version" in a warning
  colour when the new file doesn't open. noido has no status bar yet;
  decide where status like this lives.
- **Proper window activation on Wayland.** winit's focus call does
  nothing on Wayland, and it can't activate an existing window with a
  token from another process. A second launch should pass its
  `XDG_ACTIVATION_TOKEN` to the running noido, which activates with
  `xdg_activation_v1` itself (bound on winit's connection via the raw
  window handles). Only helps when the launcher supplies a token (file
  managers and the desktop do; terminals and editors often don't).
  Today the running window only asks for attention.
- **macOS open-file events.** On macOS files reach a running app as
  "open document" events, not new launches; handle them, and dropping a
  file on the app icon.
- **Bringing the window forward on Windows, checked on Windows:** the
  foreground rules may need `AllowSetForegroundWindow` from the second
  launch. The handover itself no longer relies on pipe timeouts (unsupported
  there) but has never run on Windows.
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
