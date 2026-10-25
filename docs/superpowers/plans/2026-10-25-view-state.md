# Remember View State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reopening a PDF restores position, zoom, view mode and window.

**Architecture:** `View` gains a `ViewState` snapshot/restore (pure). A new
`noido-state` crate stores it per document (BLAKE3 key, path fallback) as
TOML in the platform state directory and manages the per-document eframe
window file. The egui frontend wires both and turns on eframe persistence.

**Tech Stack:** as before, plus `blake3`, `serde`, `toml`, `directories`
(`tempfile` for tests), eframe feature `persistence`.

**Spec:** `docs/superpowers/specs/2026-10-25-view-state-design.md`

## Global Constraints

- `noido-core` stays free of egui, serde and IO.
- State problems never stop noido: load failures return `None`, save failures
  are reported once on stderr.
- fmt, clippy `-D warnings`, `cargo test --workspace` pass after every task.

## Review Focus

1. **Restore before the first viewport** in each mode combination: lands on
   the saved centre, not the top. → `restore_before_viewport_lands_on_centre`.
2. **Stale state**: page past the end, scale out of range, zero pages. →
   `restore_clamps_stale_state`, `restore_with_zero_pages_does_nothing`.
3. **Path fallback re-filing** moves both files and doesn't lose the window
   file. → `path_fallback_refiles_both_files`.
4. **Atomic write** leaves no temp file and never a half-written entry. →
   `save_leaves_no_temp_file`.
5. **Debounce**: not saved before 2 s idle; saved after; not resaved when
   unchanged. → `saves_after_idle_delay`.

---

### Task 1: ViewState snapshot and restore in View

**Files:** `crates/noido-core/src/view.rs`.

**Produces:**
- `#[derive(Clone, Copy, Debug, PartialEq)] pub struct ViewState { pub zoom: ZoomMode, pub scale: f32, pub continuous: bool, pub dual: bool, pub centre: DocPoint }`.
- `View::state(&self) -> Option<ViewState>`: `None` with zero pages. Centre is
  the doc point at the viewport centre; before the first viewport, the
  pending restore point if any, else page 0 at (0, 0).
- `View::restore(&mut self, state: &ViewState)`: clamps page to the page
  count and scale to the zoom limits; sets dual/paged (spread of the page),
  zoom mode, and in custom zoom the scale; then places the centre point at the
  viewport centre now if the viewport is non-empty, else on the first
  non-empty `set_viewport_size`.

- [ ] Failing tests (10 Letter pages, 636×800 unless noted):
  - `state_round_trips_in_every_mode`: for each continuous×dual and a custom
    zoom, scroll to y 3000, take `state()`, restore into a fresh view with the
    same viewport → same `state()` (centre within 0.5).
  - `restore_before_viewport_lands_on_centre`: restore into `View::new`, then
    `set_viewport_size(636, 800)` → doc point at the centre is the saved one
    (within 0.5), not the top.
  - `restore_fit_width_refits_new_window`: saved at 636 wide, restored at
    1236 wide → scale 2.0 (fit width), mode FitWidth.
  - `restore_clamps_stale_state`: page 99, scale 50 custom → page 9 shown,
    scale `MAX_ZOOM`.
  - `restore_paged_shows_saved_spread`: paged + dual, centre page 5 →
    `current_spread() == Some(2)`.
  - `restore_with_zero_pages_does_nothing`, and `state()` is `None`.
- [ ] Implement; `cargo test -p noido-core`. Commit "View state snapshot and restore".

### Task 2: noido-state crate

**Files:** new `crates/noido-state/{Cargo.toml,src/lib.rs,tests/store.rs}`;
workspace `Cargo.toml` members and deps.

**Produces:**
- `DocKey::new(bytes: &[u8], path: &Path) -> DocKey` (BLAKE3; path
  canonicalised, falling back to the given path).
- `Store::at(dir) -> Store`, `Store::open_default() -> Option<Store>`
  (`ProjectDirs::from("", "", "noido")`; `state_dir()` else
  `data_local_dir()`).
- `Store::load(&DocKey) -> Option<ViewState>`, `Store::save(&DocKey,
  &ViewState) -> io::Result<()>`, `Store::prune(keep: usize)`,
  `Store::window_file(&DocKey) -> PathBuf`.
- Files in `<dir>/docs/`: `<hex>.toml` (version 1, path, saved, state fields;
  zoom as `"fit-width" | "fit-page" | "custom"`), `<hex>.window.ron`.

- [ ] Failing tests (temp dir):
  - `save_then_load`; `missing_entry_loads_none`.
  - `path_fallback_refiles_both_files`: save under bytes A at path P with a
    window file; load with bytes B at P → state found; `<A>.*` gone, `<B>.*`
    exist.
  - `moved_file_found_by_hash`: save at P, load same bytes at Q → found.
  - `garbage_and_unknown_version_ignored`.
  - `save_leaves_no_temp_file`: only `<hex>.toml` in `docs/` after save.
  - `prune_keeps_newest`: 5 entries with distinct mtimes, `prune(2)` → the 2
    newest remain, with their window files; others' window files gone.
  - `window_file_copies_newest`: entries X (older) and Y (newer) with window
    files; `window_file(Z)` exists with Y's content; `window_file(Y)` is
    untouched.
- [ ] Implement; `cargo test -p noido-state`. Commit "noido-state: per-document view state storage".

### Task 3: Frontend wiring

**Files:** `crates/frontends/gui-egui/{Cargo.toml,src/main.rs,src/app.rs}`,
workspace eframe features.

- eframe `features = ["persistence"]` (plus the defaults).
- `main`: `DocKey::new`, `Store::open_default`, `prune(1000)`, `load`,
  `window_file` → `NativeOptions { persistence_path, persist_window: true }`.
- `NoidoApp::with_persistence(self, store, key, restored: Option<ViewState>)`
  calls `view.restore`.
- Each frame: if `view.state()` differs from the last saved one, save once it
  has been unchanged for `SAVE_DELAY = 2.0` s of `input.time`; request a
  repaint for when the delay ends. `on_exit` saves if unsaved.
  `persist_egui_memory` → false. A save error prints `noido: cannot save view
  state: <e>` once.

- [ ] Failing headless tests:
  - `restored_state_shows_saved_page`: restore centre page 6 → after the
    first frames the centre page is 6.
  - `saves_after_idle_delay`: PageDown; after 1 s nothing on disk; after 2.5 s
    the loaded state equals `view.state()`; delete the file, run 3 s more
    without input → not rewritten.
- [ ] Implement; full suite; clippy. Commit "Remember view state and window per document".
- [ ] Ask the user to check by hand: reopen restores page, zoom, modes; window
  size and maximized on Wayland.
