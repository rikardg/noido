# Settings

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

The view options set by keys and the menu (W/F, C, D, O) are per document,
and every new document starts from hardcoded values. A settings page lets the
reader choose what a document noido hasn't seen before opens with. More
settings will follow, so settings get their own crate from the start.

## Approaches (agreed: 3)

1. `Defaults` in `noido-core`, the settings file as a module in
   `noido-state`, the dialog in `gui-egui`. Fewer crates, but `noido-state`
   is about per-document state.
2. Everything in `gui-egui`: `View` keeps hardcoding its defaults and the
   frontend patches them after construction; the first-open logic is hard
   to test.
3. **A `noido-settings` crate for the settings file**, `Defaults` in
   `noido-core`, the dialog in `gui-egui`. Chosen: more settings are coming.

No new third-party crate: `directories` (already used) gives the config
directory on every platform, `serde` + `toml` the format. `confy` wraps the
same crates but hides the path and I/O, which tests need to control.

## Behaviour

- One page, opened by "Settings…" at the end of the menu (its own section,
  before Quit) and by Ctrl+, (Cmd+, on macOS).
- Rows, each showing the built-in default:
  - Zoom: Fit width / Fit page / Fixed [100] % (radio buttons; the number
    field takes 10–400, the range zoom already has), `(default: fit width)`.
    The percentage is the one the title bar shows.
  - Continuous `(default: on)` [x]
  - Dual page `(default: off)` [ ]
  - Odd pages left `(default: on)` [x]
- A "Set from current view" button copies the open document's zoom
  (a custom zoom becomes Fixed at its rounded percentage), continuous, dual
  and odd-left.
- Changes are saved as soon as they are made; no OK/Cancel. A close button
  and Esc close the page. If saving fails, the page says so in a line under
  the rows and keeps the values for this run.
- The settings only apply to documents with no saved state. A document
  opened before keeps its own view; the open document never changes.
- While the page is open, view shortcuts (W, D, …) do nothing; the page is
  modal.
- Each noido reads the settings when it starts (one process per file).

## Storage

- `ProjectDirs::config_dir()/settings.toml`: `~/.config/noido/` on Linux
  (honours `XDG_CONFIG_HOME`), `~/Library/Application Support/noido/` on
  macOS, `%APPDATA%\noido\config\` on Windows.
- Versioned like the state files; a `[new_documents]` table with `zoom`
  (`"fit-width"` / `"fit-page"` / `"fixed"`), `zoom_percent` (used when
  fixed; clamped to 10–400 on load), `continuous`, `dual`, `odd_left`.
- Missing file or fields give the built-in defaults; an unreadable file gives
  the defaults and a warning on stderr, and is overwritten only when the
  reader changes a setting. Written atomically (temp file, rename).

## Code shape

- `noido-core`: `Defaults { zoom, zoom_percent, continuous, dual,
  odd_left }` with `Default` (fit width, 100, on, off, on) and
  `Defaults::from_view(&ViewState)`. `View::new(page_sizes, &Defaults)`; a
  fixed zoom starts as `ZoomMode::Custom` at that scale, as a restored custom
  zoom does.
  `Action::OpenSettings`; keymap binding; menu entry.
- `noido-settings` (new crate, depends on `noido-core`): `Settings` with
  `new_documents: Defaults`; `SettingsFile::at(dir)` / `open_default()`,
  `load()`, `save()`.
- `gui-egui/settings.rs`: the dialog (`egui::Modal`); `app.rs` holds the
  settings and the file, builds new views from them, routes the action.

## Testing

- Core: `View::new` with each default, fixed zoom included and clamped;
  `from_view` for each zoom mode;
  keymap and menu entries.
- Settings: round trip; missing file; missing fields; out-of-range
  percentage; unreadable file;
  atomic save into a temp dir.
- Harness: a new document opens with the settings; a document with saved
  state ignores them; Ctrl+, opens the page; toggling a row saves; "Set from
  current view"; Esc closes; D does nothing while it is open.
