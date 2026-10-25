# Reload on change, one window per file — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** noido follows a PDF that is rewritten on disk (keeping page and spot), and a second launch on an open file brings the running window forward instead of opening a copy.

**Architecture:** `noido-state` gets a `Slot` (a document's key plus the window file its entry names), so a running app can re-key its state after a reload without moving eframe's window file. The frontend gets a `watch` module (directory watcher, debounce, opens the new version off the UI thread) and an `instance` module (a local socket per canonical path; a second launch hands over and exits). `NoidoApp` swaps in a reloaded document and reacts to activation.

**Tech Stack:** Rust 2024, eframe/egui 0.37, `notify` 8, `interprocess` 2, blake3, toml.

**Spec:** `docs/superpowers/specs/2026-10-25-reload-single-instance-design.md`

## Global Constraints

- `unsafe_code = "forbid"` stays; no unsafe in noido's crates.
- No state file version bump: files without `window` keep loading.
- Reload keeps: zoom mode and scale, continuous, dual, centre (page + spot, clamped by `View::restore`), sidebar, search text.
- "Same file" = same canonical path. The instance check happens before the file is read.
- Wayland: activation is `Minimized(false)` + `Focus` + `RequestUserAttention(Informational)`; nothing more this round.
- No reload notices; a failed reload is silent and keeps the old version.
- Never send synthetic input to the desktop; interactions are tested in the headless harness or by the user.

## Review Focus

1. **A partial file that still opens** (hayro repairs a truncated xref): noido may show a short document for a moment, but must end on the final version, since the final write fires another event. Test: an event after a completed read arms the debounce again (`debounce_rearms_after_fire`, Task 2).
2. **Renders from the old document after a reload** must never be shown as the new one's: the old renderer is dropped with its channel, and images past the new page count go. Test: `reload_drops_images_past_new_count` (Task 3).
3. **Modes survive a reload:** paged + dual + custom zoom + open sidebar all stay. Test: `reload_keeps_modes_and_sidebar` (Task 3).
4. **A stale socket file after a crash** must not stop noido from opening, nor make a later launch hang. Tests: `stale_socket_is_taken_over`, `silent_listener_times_out` (Task 4).
5. **The file disappears and stays gone** (moved away, `latexmk -C`): no reload, no crash, the watcher keeps working when it reappears. Test: `missing_file_is_not_a_reload` (Task 2) and the real-directory test recreating the file (Task 2).

---

### Task 1: Store slots: the entry names its window file; re-key

**Files:**
- Modify: `crates/noido-state/src/lib.rs`
- Modify: `crates/noido-state/tests/store.rs`
- Modify: `crates/frontends/gui-egui/src/main.rs`, `crates/frontends/gui-egui/src/app.rs` (API change only)
- Modify: `docs/backlog.md` (drop the "`main` asks for the window file before `load`" minor)

**Interfaces:**
- Produces:
  - `pub struct Slot` (`Clone, Debug`): a `DocKey` and the window file's hex name. `Slot::key(&self) -> &DocKey`.
  - `Store::load(&self, key: DocKey) -> (Slot, Option<DocState>)` — by hash, else by path (re-filing the **state file only**); the slot's window name comes from the entry's `window`, else the entry's own hex (old entries), else the key's hex (new document).
  - `Store::window_file(&self, slot: &Slot) -> PathBuf` — the named file; when missing, a copy of the newest window file, as today.
  - `Store::save(&self, slot: &Slot, state: &DocState) -> io::Result<()>` — writes `window = <slot's name>`.
  - `Store::rekey(&self, slot: &mut Slot, key: DocKey)` — renames the state file from the old hex to the new (best effort; nothing to rename is fine) and sets the slot's key; the window name is unchanged.
  - `prune` removes the window file each pruned entry names (falling back to its own hex for old entries).
  - `Entry` gains `#[serde(default)] window: Option<String>`.

- [ ] **Step 1: Update the existing tests to the new API and add the new ones** in `tests/store.rs`:
  - existing tests use `let (slot, _) = store.load(key)` and `store.save(&slot, ..)`; `path_fallback_refiles_both_files` becomes `path_fallback_refiles_state_and_keeps_window`: after the edit, `docs/` holds `<edited>.toml` and `<before>.window.ron`, and `window_file(&slot)` reads `"window"`.
  - `entry_without_window_uses_its_own_hash`: write a v1 entry by hand without `window` under hex H; `window_file(&slot)` ends in `H.window.ron`.
  - `rekey_moves_state_keeps_window`: save under key A, write the window file, `rekey(&mut slot, B)`; `load(B)` gives the state, `load(A)` misses, `window_file` still names A's window file, which reads `"window"`.
  - `save_after_rekey_records_window`: after rekey and save, a fresh `load(B)` gives a slot whose `window_file` is A's.
  - `rekey_without_state_file_is_fine`: rekey a slot never saved; then save; `load(B)` finds it.
  - `prune_removes_named_window_file`: an entry under B naming window A; pruning it removes `A.window.ron`.
  - `new_document_window_copies_newest` (the old `window_file_copies_newest`, through `load`).

- [ ] **Step 2: Run, expect compile failure** — `cargo test -p noido-state` → errors for `Slot`, `rekey`, the new `load` signature.

- [ ] **Step 3: Implement** the interfaces above in `lib.rs`. Update the module doc: the entry names its window file.

- [ ] **Step 4: Run** `cargo test -p noido-state` → all pass.

- [ ] **Step 5: Adapt the frontend.** `main`: `let (slot, restored) = store.load(key)` **before** `window_file(&slot)`; `prune` before `load`. `NoidoApp::with_persistence(store, slot, restored)`; `Persistence` holds a `Slot`. Harness tests: `test_key()` callers go through `Store::load` or a `test_slot()` helper built by `Store::at(dir).load(test_key()).0`. Drop the backlog minor.

- [ ] **Step 6: Verify** `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` → pass, clean.

- [ ] **Step 7: Commit** `jj commit -m "State entries name their window file; re-key a slot"`.

---

### Task 2: Watching the file

**Files:**
- Create: `crates/frontends/gui-egui/src/watch.rs`
- Modify: `Cargo.toml` (workspace dep `notify = "8"`), `crates/frontends/gui-egui/Cargo.toml`, `main.rs` (`mod watch;`)

**Interfaces:**
- Consumes: `DocKey::new`, `load::load_document`, `Renderer::start`, `Thumbnailer::start`.
- Produces:
  - `pub struct Reload { pub renderer: Renderer, pub thumbnailer: Thumbnailer, pub info: DocumentInfo, pub key: DocKey }`
  - `pub struct Debounce` with `new(quiet: Duration)`, `event(&mut self, now: Instant)`, `wait(&self, now: Instant) -> Option<Duration>` (`None`: nothing pending; else time left, zero when due), `fire(&mut self, now: Instant) -> bool` (true once when due; clears).
  - `fn open_new(engine: &Arc<dyn PdfEngine>, path: &Path, current: &DocKey, waker: &Waker) -> Option<Reload>` — reads, skips an unchanged hash, missing/empty/unopenable files; `Waker = Arc<dyn Fn() + Send + Sync>`.
  - `pub struct Watcher` with `start(path: &Path, current: DocKey, engine: Arc<dyn PdfEngine>, waker: Waker) -> Result<Watcher, String>` and `try_recv(&self) -> Option<Reload>`. Watches the canonical path's directory non-recursively, keeps events whose path has the file's name, debounces with `QUIET = 300 ms` on its own thread, sends each `Reload` and wakes the UI. Dropping it stops watching.

- [ ] **Step 1: Tests (in `watch.rs`)**:
  - `events_within_quiet_period_fire_once`: events at 0, 100, 250 ms; `fire` false at 500, true at 551 ms (300 after the last), false after.
  - `wait_reports_time_left`: event at 0 → `wait(100ms) == Some(200ms)`; nothing pending → `None`.
  - `debounce_rearms_after_fire`: fire, then an event → fires again after the quiet period.
  - `unchanged_hash_is_not_a_reload`: `open_new` with the same bytes as `current` → `None`.
  - `missing_file_is_not_a_reload`: nonexistent path → `None`.
  - `unopenable_file_is_not_a_reload`: engine whose `open` errors → `None`.
  - `changed_file_reloads`: new bytes with a blank engine → `Some`, key differs from `current`, `info.page_sizes` from the engine.
  - `watcher_follows_rename_over_and_recreate` (real temp dir, 5 s deadline polling `try_recv`): write a.pdf, start; write `a.tmp` and rename over → one `Reload`; delete a.pdf, wait 500 ms (no reload), write it again → a `Reload`.
  The test engine is a small one in `watch.rs` tests reporting `bytes.len()` pages (so a change is visible), erroring on empty input.

- [ ] **Step 2: Run** `cargo test -p gui-egui watch` → fails to compile.

- [ ] **Step 3: Implement** `watch.rs`; the thread loop is `recv_timeout(debounce.wait(now).unwrap_or(long))`, feeding events into the debounce and calling `open_new` when it fires; the `current` key advances on each successful reload.

- [ ] **Step 4: Run** `cargo test -p gui-egui watch` → pass; whole workspace + clippy clean.

- [ ] **Step 5: Commit** `jj commit -m "Watch the document's file and open new versions in the background"`.

---

### Task 3: Reloading in the app

**Files:**
- Modify: `crates/frontends/gui-egui/src/app.rs`, `main.rs`

**Interfaces:**
- Consumes: `watch::{Reload, Watcher}`, `Store::rekey`.
- Produces: `NoidoApp::reload(&mut self, reload: Reload)`; `NoidoApp::with_watcher(self, Watcher) -> Self`; each frame takes the newest pending `Reload` (`while let` → keep the last) before input handling.

`reload`: take `self.view.state()`; `View::new(info.page_sizes)` with the current viewport size set, then `restore`; new `RenderTracker`; replace renderer and thumbnailer (thumbnail `sent = None`); keep `textures` and thumbnail textures for pages below the new count, drop the rest; `focus = None`; persistence `rekey` to `reload.key`. The old images keep showing in their new page rects until new renders arrive.

- [ ] **Step 1: Harness tests** (a `BlankEngineN(pages, width, height)` test engine; `Harness::reload(engine)` builds a `Reload` with `Renderer::start`/`Thumbnailer::start`):
  - `reload_keeps_page_and_spot`: go to page 6 via PageDown presses, note `view.state().centre`; reload with 10 pages → same centre.
  - `reload_clamps_to_last_page`: at page 8 of 10, reload with 4 pages → centre page 3.
  - `reload_keeps_modes_and_sidebar`: dual + paged (D, C) + zoom in (Mod+=) + F9; reload → `view.state()` modes and scale equal, `sidebar.open`.
  - `reload_keeps_old_images_until_new_ones_arrive`: with the gated engine, reload; the next frame paints an image (not a white rect) in the centre page's rect (`painted` helper).
  - `reload_drops_images_past_new_count`: reload 10 → 4 pages; no texture key ≥ 4.
  - `reload_saves_under_new_key`: with persistence in a temp dir, reload with key B, wait past `SAVE_DELAY`; `store.load(B)` has the state, the slot's window name unchanged.

- [ ] **Step 2: Run** → compile failure, then the new tests fail for the expected reasons.

- [ ] **Step 3: Implement** `reload`, `with_watcher`, the per-frame intake. `main`: start the `Watcher` after the renderer (`HayroEngine`, the key, a waker); a failure to start prints `noido: cannot watch <path>: <e>` and continues without.

- [ ] **Step 4: Verify** `cargo test --workspace`, clippy clean.

- [ ] **Step 5: Commit** `jj commit -m "Reload the document when its file changes, keeping the place"`.

---

### Task 4: One window per file

**Files:**
- Create: `crates/frontends/gui-egui/src/instance.rs`
- Modify: `Cargo.toml` (workspace dep `interprocess = "2"`), `crates/frontends/gui-egui/Cargo.toml`, `main.rs`, `app.rs`

**Interfaces:**
- Produces:
  - `pub enum Claim { Running, First(Listener), Unavailable }` — `Unavailable` when no socket can be made (noido then opens normally).
  - `pub fn claim(path: &Path) -> Claim` — canonicalises (`path` as given if that fails), names the socket after a blake3 hash of the canonical path (first 32 hex chars): `$XDG_RUNTIME_DIR/noido/<hash>.sock` on Linux (temp dir when unset), `std::env::temp_dir()/noido-<hash>.sock` on macOS, a named pipe `noido-<user>-<hash>` on Windows. `claim_at(socket: Name, timeout: Duration) -> Claim` is the testable core; `TIMEOUT = 1 s`.
  - Protocol: client sends `activate\n`, server answers `ok\n`. Connect refused / socket file without listener → remove it and listen. Connected but no `ok` within the timeout → `Unavailable`: noido opens its own window, as the spec says, but can't take the name from a live process, so it doesn't listen.
  - `Listener::serve(self, waker: Waker) -> Activation` — a thread answering each connection; on `activate` sets the flag and wakes.
  - `pub struct Activation` (`Clone`): `take(&self) -> bool`; `Activation::default()` for tests.
  - `NoidoApp::with_activation(self, Activation) -> Self`; each frame, if `take()`: send `Minimized(false)`, `Focus`, `RequestUserAttention(UserAttentionType::Informational)`.

- [ ] **Step 1: Tests** (`instance.rs`, sockets in a temp dir):
  - `second_claim_hands_over`: claim A → `First`, serve; claim A again → `Running`; the activation flag is set.
  - `different_path_is_first`: claims for A and B both `First`.
  - `stale_socket_is_taken_over`: create a plain file at the socket path (or bind and drop without cleanup) → `First`.
  - `silent_listener_times_out`: a raw listener that accepts and never answers; claim with 200 ms timeout → `Unavailable`, within 1 s.
  - App harness: `activation_raises_window`: `with_activation(a)`, set it, one frame → commands contain `Focus` and `RequestUserAttention(..)`, and the flag is cleared.

- [ ] **Step 2: Run** → fails to compile.

- [ ] **Step 3: Implement** `instance.rs` and the app hook. `main`: `claim` first, before `load_document`; `Running` → exit success silently; `First(listener)` → `listener.serve(waker)` once the context exists; `Unavailable` → continue without.

- [ ] **Step 4: Verify** `cargo test --workspace`, clippy clean, `cargo build --release`.

- [ ] **Step 5: Commit** `jj commit -m "A second launch on an open file brings its window forward"`.

---

### Final review

Fresh reviewer on opus over the whole range, with this plan's Review Focus. Then hand checks for the user: `latexmk -pvc` (or repeated `pdflatex`) with noido open, a failing build, a second `noido file.pdf` on Wayland (attention notice) and with `env -u WAYLAND_DISPLAY` (X11 raise).
