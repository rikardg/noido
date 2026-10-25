# Reload on change, one window per file

Date: 2026-10-25 · Status: draft

## Intent

noido is used as the preview next to a LaTeX build: the PDF is rewritten
on every compile, and the open window should follow without losing the
reader's place. And opening a file that is already open should bring its
window forward, not open a second copy that fights the first over the
saved state. Both rest on how a document is identified (content hash,
path as fallback), so they are designed together.

## Behaviour (agreed)

- **Reload when the file changes on disk**, automatically.
- **Keep the same page number and the same spot on it** after a reload,
  clamped when pages were lost.
- **A file that is already open isn't opened again:** the running window
  is brought forward instead.
- **"Same file" means the same path** (canonical). A copy of the same PDF
  at another path opens its own window.
- **On Wayland, a real raise is deferred.** The running window asks for
  attention instead (on GNOME a "noido is ready" notice); proper
  activation goes to the backlog (see below).
- **No reload notices this round.** "Reloaded" and "Reload failed" notices
  in a warning colour need a place to show status, which noido doesn't
  have yet; backlog.

## Proposals (my calls, agreed in conversation)

- **Waiting for the writer:** LaTeX tools delete, truncate or half-write
  the file before the final write. noido waits until the file has been
  quiet for a short time (about 300 ms) before reading it, and keeps the
  old version on screen until the new one opens without errors. A file
  that is missing or doesn't open is ignored until the next change; the
  old version stays.
- **Unchanged content is not a reload:** if the new bytes hash the same
  (a `touch`, a rebuild with identical output), nothing happens.
- **No blank flash:** the old page images stay on screen, in the new
  layout, until the new renders and thumbnails replace them. Images of
  pages that no longer exist are dropped.
- **What a reload keeps:** zoom mode and scale, continuous and dual page,
  the centre point (page + spot, clamped by the existing
  `View::restore`), the sidebar, the search text.
- **Watching:** the file's directory is watched, not the file, since many
  tools replace the file by renaming a new one over it.

## Identity and stored state

The content hash still names what is remembered about a document; it
gains three adjustments.

1. **Re-key on reload.** After a reload the open document has a new hash.
   The running app moves its state file to the new hash (as `load`
   already does for a file edited between sessions) and saves under the
   new key from then on.
2. **The state file names its window file.** eframe receives the window
   file's path once, at launch, and writes to it on exit; it can't follow
   a hash change. The state entry therefore records the window file's
   name (a new optional field, `window`). A reload re-files the state
   entry only; the window file keeps its name. Entries without the field
   (written before this) use their own hash, as today: no version bump.
   `prune` deletes the window file an entry names. `main` reads the state
   before asking for the window file (resolving the backlog note about
   that order).
3. **"Is it open?" asks by path, before reading the file.** It doesn't
   need the hash, so a second launch hands over at once, even for a large
   file.

## Already open: handing over

- Each running noido listens on a **local socket named after its file's
  canonical path** (a hash of the path, so the name is short and safe):
  under `$XDG_RUNTIME_DIR/noido/` on Linux, the temp directory on macOS,
  a named pipe on Windows. My call: the `interprocess` crate, which covers
  all three with one API.
- A new launch first canonicalises the path and tries to connect. If a
  running noido answers, the new launch sends "activate", waits for the
  reply, and exits successfully. Otherwise it opens the document as
  today and starts listening.
- **A stale socket** (left by a crash: connecting fails) is removed and
  taken over. A running noido that doesn't answer within about a second
  is treated as gone: the new launch opens its own window.
- **The running noido, on "activate":** un-minimises and asks for focus
  (`ViewportCommand::Focus`; works on X11 and Windows, presumably macOS)
  and asks for attention (`RequestUserAttention`), which on Wayland is
  what makes the window noticeable.
- The protocol is a line of text, so a later round can add the launch's
  activation token or a page to jump to.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| Socket per open file, path-named; a launch hands over and exits | One process per window as today; simple, no shared server; each window is independent | **Chosen** |
| One noido process owning all windows (eframe multi-viewport); later launches forward to it | Natural for macOS's open-file events; a large rework of the frontend, and one crash takes all documents down | Rejected for now |
| Look up "open" by content hash | Needs the whole file read and hashed first; and the hash of an open document changes on every reload | Rejected |
| Name window files by path hash | Window size would follow the path, not the document; mixes two keys | Rejected; the state entry names its window file instead |

## Design

### `noido-state`

- `Entry` gains `#[serde(default)] window: Option<String>` (hex of the
  window file); `None` reads as the entry's own hash.
- `Store::load(&DocKey) -> Option<DocState>` keeps its re-filing for a file
  edited between sessions, but re-files only the state file; the
  window name moves with the entry.
- `Store::window_file(&DocKey) -> PathBuf` takes the name from the entry
  when there is one; otherwise as today (a copy of the newest window
  file, under the document's hash), and the next save records it.
- `Store::rekey(&mut self or &self, old: &DocKey, new: &DocKey)` (exact
  signature left to the plan): moves the entry to the new hash, keeping
  its `window`.
- `prune` removes the window file each removed entry names.
- `DocKey` exposes the canonical path for the instance lookup (or the
  lookup canonicalises itself; plan's call).

### Frontend (`gui-egui`)

- **`instance` module:** `claim(path) -> Claim`, either `Running`
  (handed over; exit) or `First(Listener)`. The listener runs on a thread,
  and on "activate" sets a flag the app reads and wakes the UI.
- **`watch` module:** watches the file's directory (my call: the `notify`
  crate), filters events to the file's name, and debounces. After a quiet
  period it reads the file, skips it when the hash is unchanged, and
  opens it (`Renderer::start` and `Thumbnailer::start` with the new
  bytes, off the UI thread). A successful open is handed to the app with
  the new `DocKey`; a failure is dropped silently (backlog: notices).
  The debounce is a small state machine with an injected clock, tested
  without a file system.
- **`NoidoApp::reload(renderer, thumbnailer, info, key)`:** captures the
  current `ViewState`, builds a `View` for the new page sizes, restores
  the state, resets the render tracker, keeps the old images as
  placeholders (dropping pages past the new count), and re-keys
  persistence.
- **Activation:** when the listener's flag is set, the app sends
  `Minimized(false)`, `Focus` and `RequestUserAttention(Informational)`.

## Non-goals

- Reload notices and a status area (backlog).
- Proper Wayland activation with the launcher's token (backlog).
- macOS open-file events and dropping files on the app (backlog).
- Jumping to a page or position from the command line (SyncTeX and the
  like); the protocol leaves room.
- Opening several files in one launch.

## Testing

- **`noido-state`:** an entry without `window` uses its own hash; `rekey`
  moves the state and keeps the window name; `window_file` after a rekey
  returns the original window file; `prune` deletes the named window
  file; loading by path after an edit between sessions keeps the window
  name.
- **Debounce:** events closer than the quiet period collapse into one
  read; a read that fails waits for the next event; an unchanged hash
  doesn't reload.
- **Instance:** in a temp directory, a second claim for the same path gets
  `Running` and the first sees "activate"; a different path gets `First`;
  a stale socket file is taken over; an unresponsive listener times out
  into `First`.
- **Headless app:** reload with a document of fewer pages clamps to its
  last page; reload with the same page count keeps page and spot; old
  images stay until new ones arrive; persistence saves under the new
  key after a reload.
- **By hand:** `latexmk -pvc` (or repeated `pdflatex`) with noido open:
  follows each compile, keeps the place, survives a failing build. A
  second `noido file.pdf` brings the first forward (X11) or shows the
  attention notice (Wayland).

## Left to the builder

- The exact quiet period, socket file naming, and timeout values.
- Whether `watch` and `instance` later move to a frontend-neutral crate.
