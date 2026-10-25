# Title Bar and Sidebar Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** noido draws its own title bar (sidebar toggle, search field, title
with page and zoom, ☰ menu, window controls) and has an empty, resizable,
remembered left sidebar.

**Architecture:** Keys, menu and shortcut labels become data in
`noido-core` (with a per-binding "applies while editing text" flag).
`noido-state` stores a `DocState` (view state + sidebar state). The egui
frontend lays out title bar → sidebar → page view with egui panels, and on
Linux/Windows replaces the system frame (move, maximise, window buttons,
resize band, border); on macOS it uses the full-size content view.

**Tech Stack:** as before (egui/eframe 0.37: `egui::Panel`,
`ViewportCommand::{StartDrag, BeginResize, Maximized, Minimized, Close}`,
`ViewportBuilder::with_decorations / with_fullsize_content_view /
with_titlebar_shown / with_title_shown / with_titlebar_buttons_shown /
with_min_inner_size`).

**Spec:** `docs/superpowers/specs/2026-10-25-titlebar-sidebar-design.md`

## Global Constraints

- `noido-core` stays free of egui, serde and IO.
- Sizes in points: search field min 160, 200 with the sidebar hidden;
  sidebar default 260, min 160, max 50 % of the window; resize band 5;
  traffic-light inset 76 (macOS, not fullscreen); min window 480 × 300.
- Keys: F9 toggle sidebar, Mod+F focus search, Mod+Q quit. While a text
  field has focus only bindings with Mod, function keys and Page Up/Down act.
- Colours from egui's theme (`visuals`), so light and dark both work; close
  button hover red.
- Saved-file compatibility: version 1 entries without sidebar fields load
  with defaults; no version bump.
- fmt, clippy `-D warnings`, `cargo test --workspace` pass after every task.

## Review Focus

1. **A restored sidebar width that no longer fits** (saved 2000 in a 900
   window, or NaN/negative in the file) is clamped to 160..=50 %, never
   hides the page view. → `restored_sidebar_width_is_clamped` (Task 3).
2. **Esc only undoes what Enter did:** a sidebar that was already open
   (F9) stays open after Enter, Esc. → `esc_keeps_sidebar_opened_by_toggle`
   (Task 3).
3. **Maximised windows have no resize band** and the band never steals
   clicks from the window buttons' centres. →
   `no_resize_band_when_maximised`, `close_button_closes` (Task 5).
4. **Pressing on controls never starts a window move**; only empty bar does.
   → `drag_on_search_field_does_not_move_window` (Task 5).
5. **A very long file name at the minimum window size** doesn't push the
   menu button off-screen; the name is elided. →
   `long_name_keeps_menu_on_screen` (Task 4).

---

### Task 1: Keymap, menu and shortcut labels in noido-core

**Files:** `crates/noido-core/src/keymap.rs`; new
`crates/noido-core/src/menu.rs`; `lib.rs`.

**Interfaces — produces:**
- `Key` gains `F9`, `Q`. `Action` gains `ToggleSidebar`, `FocusSearch`, `Quit`.
- `Keymap::action(&self, press: KeyPress, editing: bool) -> Option<Action>`
  (replaces the one-argument form; `editing` = a text field has focus).
  A binding applies while editing when it has Mod, or its key is `F9`,
  `PageUp` or `PageDown`.
- `Keymap::key_for(&self, action: Action) -> Option<KeyPress>`: the first
  binding in declaration order (ZoomIn → Mod+`Equals`, before Mod+`Plus`).
  Bindings are kept in declaration order (a `Vec`, or a map plus order).
- `View::apply` ignores the three new actions (the frontend handles them).
- `pub enum Platform { Mac, Other }`;
  `pub fn shortcut_label(press: KeyPress, platform: Platform) -> String`:
  Other → `"Ctrl+="`, `"W"`, `"F9"`, `"Page Down"`; Mac → `"⌘="`, `"W"`.
- `pub enum Check { Continuous, Dual, Sidebar }`;
  `pub struct MenuEntry { pub action: Action, pub label: &'static str, pub check: Option<Check> }`;
  `pub fn menu() -> Vec<Vec<MenuEntry>>` (sections, separated when drawn):
  1. Fit width, Fit page, Zoom in, Zoom out
  2. Continuous ✓, Dual page ✓
  3. Show sidebar ✓
  4. Quit

- [ ] Failing tests (keymap.rs / menu.rs), existing tests updated to
  `action(press, false)`:
  - `f9_toggles_sidebar`, `mod_f_focuses_search` (plain F stays FitPage),
    `mod_q_quits`.
  - `editing_blocks_plain_keys`: `action(W, editing=true)` → None; C, D, F
    plain → None.
  - `editing_keeps_mod_function_and_page_keys`: Mod+= → ZoomIn, F9 →
    ToggleSidebar, PageDown → ScrollScreenDown, all with `editing=true`.
  - `key_for_zoom_in_is_mod_equals`; `every_menu_action_has_a_key` (all
    entries in `menu()`).
  - `labels_per_platform`: Mod+Equals → "Ctrl+=" / "⌘="; F9 → "F9" on both;
    plain W → "W"; Mod+Q → "Ctrl+Q" / "⌘Q".
  - `menu_sections`: four sections, labels in the order above, checks on
    exactly Continuous, Dual page, Show sidebar.
- [ ] Implement; frontend `core_key` maps `egui::Key::F9`, `Q`, and
  `handle_input` passes `editing = false` for now (keeps its early return
  until Task 3). `cargo test --workspace`; clippy.
- [ ] Commit "Keymap: sidebar, search and quit keys; menu and labels as data".

### Task 2: Sidebar state stored with the document

**Files:** `crates/noido-core/src/view.rs` (or a new `sidebar.rs`);
`crates/noido-state/src/lib.rs`, `tests/store.rs`;
`crates/frontends/gui-egui/src/{app.rs,main.rs}`.

**Interfaces — produces:**
- core: `#[derive(Clone, Copy, Debug, PartialEq)] pub struct SidebarState { pub open: bool, pub width: f32 }`,
  `Default` = closed, `SIDEBAR_DEFAULT_WIDTH` (260). Constants
  `SIDEBAR_MIN_WIDTH` (160), `SIDEBAR_MAX_FRACTION` (0.5).
  `SidebarState::clamped(self, window_width: f32) -> SidebarState`:
  width into `SIDEBAR_MIN_WIDTH ..= max(min, window_width × fraction)`;
  non-finite → default width.
- state: `#[derive(Clone, Copy, Debug, PartialEq)] pub struct DocState { pub view: ViewState, pub sidebar: SidebarState }`.
  `Store::load(&DocKey) -> Option<DocState>`;
  `Store::save(&DocKey, &DocState) -> io::Result<()>`. `Entry` gains
  `#[serde(default)] sidebar_open: bool` and `sidebar_width` (default 260).
- app: `with_persistence(store, key, restored: Option<DocState>)`;
  `NoidoApp` gains `sidebar: SidebarState`; `Persistence` debounces the
  whole `DocState` (save when either part changes).

- [ ] Failing tests:
  - store: `sidebar_round_trips`; `v1_file_without_sidebar_loads_defaults`
    (write a TOML with only the old fields → `sidebar == SidebarState::default()`).
  - core: `clamped_width` — 2000 in 900 → 450; 50 in 900 → 160; NaN → 260;
    300 in 200 (window narrower than 2 × min) → 160.
  - app: `saves_after_idle_delay` and `restored_state_shows_saved_page`
    adapted to `DocState`; `sidebar_change_is_saved` (set
    `app.sidebar.open = true`, 150 frames → `store.load` has it open).
- [ ] Implement; suite; clippy. Commit "Store sidebar state with the document".

### Task 3: Layout with the sidebar; keys while editing

**Files:** `crates/frontends/gui-egui/src/app.rs`; new
`crates/frontends/gui-egui/src/sidebar.rs`.

**Interfaces — consumes:** Task 1 keymap, Task 2 `SidebarState`.
**Produces:** per frame `show` lays out a top panel `"titlebar"` (empty
placeholder bar of fixed height for now, filled in Task 4), then
`egui::Panel::left("sidebar")` when `sidebar.open` (resizable, default and
min/max per Global Constraints, width read back into `sidebar.width` each
frame, restored width `clamped` to the window), then the page view in the
remaining area. Sidebar body: muted text "Search results, thumbnails and the
outline will appear here". App fields: `search: String`,
`search_focus_requested: bool`, `sidebar_opened_by_search: bool`.
`handle_input` drops the early return and passes
`editing = ctx.text_edit_focused()`; `ToggleSidebar` flips `sidebar.open`;
`FocusSearch` requests focus; `Quit` sends `ViewportCommand::Close`.

Harness additions: records the last frame's root viewport commands
(`commands: Vec<egui::ViewportCommand>`), and a `typed(text)` helper sending
`Event::Text`.

- [ ] Failing headless tests:
  - `f9_opens_sidebar_and_page_view_narrows`: fit width, F9 → the view's
    viewport width shrinks by about the sidebar width and the scale drops;
    F9 again → back.
  - `restored_sidebar_width_is_clamped`: restore width 2000 → drawn sidebar
    ≤ 450 (window 900).
  - `mod_q_sends_close`.
- [ ] Implement; suite; clippy. Commit "Left sidebar shell".

### Task 4: Title bar contents

**Files:** new `crates/frontends/gui-egui/src/titlebar.rs`; `app.rs`, `main.rs`.

**Interfaces — consumes:** Task 1 `menu()`, `shortcut_label`, `key_for`;
Task 3 layout and fields. **Produces:** `NoidoApp::with_name(String)`
(main passes the file name). The bar draws, left to right: toggle button
(highlighted when open), search field (`TextEdit::singleline`, hint
"Search", width = sidebar width minus the space before the field when open,
`max` 160; 200 when hidden), the name centred and elided, followed by muted
"{page} / {count} · {zoom} %" (centre page + 1, `round(scale × 100)`), then
the ☰ menu (egui menu button; entries from `menu()`, checkmarks from view
and sidebar, shortcut right-aligned via `shortcut_label` for the current
platform; clicking applies the action exactly like the key and closes the
menu). Enter in the field opens the sidebar (and sets
`sidebar_opened_by_search` if it was closed); Esc in the field surrenders
focus and closes the sidebar if `sidebar_opened_by_search`.

- [ ] Failing headless tests:
  - `typing_w_in_search_keeps_zoom`: Mod+F, type "w" → zoom mode
    unchanged, `search == "w"`; then Mod+= still zooms in and F9 still
    toggles.
  - `enter_opens_sidebar_and_esc_closes_it`.
  - `esc_keeps_sidebar_opened_by_toggle`: F9, Mod+F, Enter, Esc → still open.
  - `search_field_keeps_min_width`: window 480 wide, sidebar open at 160
    and closed → field width ≥ 160 (read from the field's response rect,
    exposed for tests).
  - `menu_dual_matches_key`: applying the menu's Dual page entry (through
    the same function the menu calls) → `view.dual()` true.
  - `status_text`: on page 3 of 10 at scale 1.06 → "3 / 10 · 106 %".
  - `long_name_keeps_menu_on_screen`: 200-character name, window 480 → the
    menu button's rect lies inside the window.
- [ ] Implement; suite; clippy. Commit "Title bar: sidebar toggle, search field, status and menu".
- [ ] Note in the ledger the user's open question: does the field being
  wider with the sidebar hidden (200) than with a narrow sidebar (160) look
  odd? Ask when they try it.

### Task 5: Window frame

**Files:** new `crates/frontends/gui-egui/src/frame.rs`; `titlebar.rs`,
`main.rs`; `docs/backlog.md`.

**Interfaces — produces:**
`fn resize_direction(window: Rect, pointer: Pos2, band: f32) -> Option<egui::ResizeDirection>`
(corners win over edges where bands overlap; `None` inside). Linux and
Windows (`!cfg!(target_os = "macos")`): `ViewportBuilder` with
`with_decorations(false)`, `with_min_inner_size([480.0, 300.0])`; window
buttons at the bar's right (minimise → `Minimized(true)`, maximise ↔
restore → `Maximized(!maximized)`, close → `Close`); the resize band (cursor
icon on hover, `BeginResize` on press) unless maximised or fullscreen; a
1-pixel border in a theme stroke colour unless maximised. All platforms:
press-and-drag on empty bar area → `StartDrag`; double-click there →
`Maximized(!maximized)`. macOS: `with_fullsize_content_view(true)`,
`with_titlebar_shown(false)`, `with_title_shown(false)`,
`with_titlebar_buttons_shown(true)`, and a 76-point inset before the toggle
unless fullscreen.

- [ ] Failing tests:
  - unit: `resize_direction` — pointer 2 from the left edge mid-height →
    West; 2 from top-right corner → NorthEast; 20 inside → None; exactly on
    the band boundary (5) → None.
  - headless: `close_button_closes` (click at the close button's centre →
    `Close` in commands), `double_click_empty_bar_maximises`,
    `drag_on_empty_bar_starts_move`,
    `drag_on_search_field_does_not_move_window`,
    `no_resize_band_when_maximised` (input with `maximized = true`, press
    at the window edge → no `BeginResize`).
- [ ] Implement; suite; clippy.
- [ ] Backlog: macOS traffic-light centring in a taller bar (empty
  `NSToolbar`, `objc2`) and translucency (`window-vibrancy`), unchecked
  until a Mac; reading the macOS double-click preference; checking the
  frame on Windows (snapping, edges); GNOME's button-layout setting.
- [ ] Commit "Own window frame: move, maximise, window buttons, resize".
- [ ] Launch for the user (scratch `XDG_STATE_HOME`) and ask them to check
  by hand: drag to move, double-click, resize from all edges and corners,
  window buttons, Super+arrow tiling, the look without a shadow, and the
  search field width question.
