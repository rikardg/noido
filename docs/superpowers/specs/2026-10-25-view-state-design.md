# Remember view state per document

Date: 2026-10-25 · Status: draft for review

## Intent

Reopening a PDF puts the reader back where they left it: same place, same
zoom, same view mode. This is disposable per-machine state, kept apart from
notes, which are the user's data and will live next to the PDF.

## Behaviour (agreed)

- Stored per document: position, zoom mode and scale, continuous/paged,
  single/dual, and the window: size, position (its screen), maximized and
  fullscreen.
- A document opened for the first time gets the window of the most recently
  saved document, so noido opens where it was last.
- On Wayland the compositor places windows: size, maximized and fullscreen
  are restored, position and screen are not (accepted; no XWayland fallback).
  X11, Windows and macOS restore all of it, moved back onto a connected screen
  if its screen is gone.
- Stored in the platform's place for application state:
  - Linux: `$XDG_STATE_HOME/noido` (default `~/.local/state/noido`).
  - macOS: `~/Library/Application Support/<bundle id>` (not `Caches`, which
    the system may purge).
  - Windows: `%LOCALAPPDATA%\noido` (local, not roaming).
- A document is recognised by a hash of its content first, then by its path.
  A moved file and a file edited in place both find their state.
- Saved on close, and a couple of seconds after the reader stops moving, so a
  crash loses only seconds.
- No command-line options in this step (`--page N` and the like come later).

## Proposals (my calls)

- **Position** is the document point at the window centre, the same anchor as
  zoom and resize. It works when the window size differs between runs.
- **Restoring zoom:** in fit width and fit page the mode is restored and the
  page is refitted to the new window; in custom zoom the scale is restored.
- **Bad or stale state is ignored, never fatal.** That covers an unreadable
  file, an unknown format, or a page index past the end (the document
  changed). Values are clamped: page to the page count, scale to the zoom
  limits. If the state directory can't be written, noido prints one warning
  to stderr and carries on.
- **Two small files per document**: `docs/<hash>.toml` (view state) and
  `docs/<hash>.window.ron` (window, written by eframe; see below). Pruning,
  re-filing and copying treat them as a pair.
- **View state file** (`docs/<hash>.toml`). It is written to a
  temporary file and renamed into place. Two noido windows never write the
  same shared file; for the same document the last write wins.
- **Pruning:** at startup, beyond 1000 entries the least recently written
  ones are deleted.
- **Path fallback:** when no entry matches the hash, scan the entries for the
  canonical path and take the newest. The state found that way is saved under
  the new hash and the old entry is removed. Scanning 1000 tiny files at
  startup is cheap.
- **Hash:** BLAKE3 over the whole file, which is already in memory. It takes a
  few milliseconds even for large PDFs and is done once at startup.

## Approaches considered (where the code lives)

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. Snapshot/restore in `noido-core` (no IO); storage in a new `noido-state` crate; frontend wires them | Core stays pure and testable; a later TUI reuses both; one more crate | **Chosen** |
| B. Everything in the egui frontend, or eframe's built-in persistence | Least code; eframe's persistence is one blob per app, not per document, and keyed for egui | Rejected |
| C. Storage inside `noido-core` | Brings filesystem, serde and hashing into the pure logic crate | Rejected |

## Design

### `noido-core`: snapshot and restore

- `ViewState { zoom: ZoomMode, scale: f32, continuous: bool, dual: bool,
  centre: DocPoint }`: plain data, `Clone + PartialEq`.
- `View::state() -> ViewState`: the current snapshot. The centre is the
  document point at the viewport centre.
- `View::restore(state)` may be called before the first viewport. It sets
  the modes and zoom right away and holds the centre as pending. On the first
  non-empty `set_viewport_size`, instead of starting at the top, the view
  fits (in fit modes) and places the pending point at the window centre. In
  paged mode the spread is the one containing the saved page. Out-of-range
  values are clamped as above. With zero pages, restore does nothing.

### `noido-state`: storage (new crate)

- `Store::open_default() -> Option<Store>` uses `directories::ProjectDirs`:
  `state_dir()` on Linux, `data_local_dir()` elsewhere.
  `Store::at(dir)` is used in tests.
- `DocKey { hash: [u8; 32], path: PathBuf }` (canonical path), built with
  `DocKey::new(bytes, path)`.
- `Store::load(&DocKey) -> Option<ViewState>`: by hash, else by path
  (re-filed as above).
- `Store::save(&DocKey, &ViewState) -> io::Result<()>`: atomic write.
- `Store::prune(keep: usize)`.
- File format: TOML with `version = 1`, `path`, `saved` (Unix time), and the
  `ViewState` fields. A version other than 1 means the entry is ignored.

- `Store::window_file(&DocKey) -> PathBuf`: the document's window file. If it
  doesn't exist yet, the newest `*.window.ron` in `docs/` is copied to it
  first (best effort).

### Window: eframe persistence

eframe's `persistence` feature already saves and restores window size,
position, maximized and fullscreen, with clamping onto connected monitors and
mixed scale factors handled. `NativeOptions::persistence_path` points it at
the document's window file and `persist_window` stays on. Because noido passes
its own `egui::Context`, eframe does not load egui memory from the file;
`App::persist_egui_memory` returns false so it doesn't write it either. The
900×1100 default applies only when there is no window file.

### Frontend (egui)

- In `main`: build the `DocKey`, open the store, load the state, and pass
  `Option<(Store, DocKey)>` plus the loaded state to `NoidoApp`.
- `NoidoApp::new` calls `view.restore(state)` when a state was loaded.
- Each frame: compare `view.state()` with the last saved snapshot. If it
  differs and nothing has changed for 2 seconds, save (the store write is
  small and synchronous). Also save in eframe's `on_exit`.

## Non-goals

- Command-line options.
- A recent-files list.
- Syncing state between machines.
- Notes and annotations.

## Testing

- `noido-core` unit tests:
  - round trip `state()` → `restore()` in each mode combination
  - restore before the first viewport lands at the saved centre
  - fit modes refit to a different window size
  - page past the end is clamped
  - zero pages
- `noido-state` tests in a temporary directory:
  - save then load
  - path fallback re-files under the new hash
  - unknown version and garbage files are ignored
  - pruning keeps the newest
  - atomic write leaves no temporary file behind
- Window restore is eframe's code and is checked by hand (open, move, resize,
  close, reopen), on Wayland for size and maximized only.
- A headless app test checks that a restored state shows the saved page on
  the first frame, and that a change is saved after the idle delay. Time is
  injected, not slept.

## Left to the builder

- The exact TOML field names.
- The `ProjectDirs` qualifier and organisation strings.
- How the idle timer is injected for tests.
