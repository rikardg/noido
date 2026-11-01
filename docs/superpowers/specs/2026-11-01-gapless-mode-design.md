# Gapless mode

Date: 2026-11-01 · Status: small change, agreed in conversation

## Intent

The reader can show pages without the 12 pt `PAGE_GAP` between them: no
vertical gap between rows in continuous mode, and no horizontal gap between
the two pages of a spread in dual mode. It is a per-document view setting
like continuous, dual and odd pages left, with a default for new documents
in the settings page.

## Approaches (agreed: A)

1. **A boolean in `LayoutMode`** (`gapless: bool`). `Layout` decides both
   gaps: `spread_gap` returns 0 for a spread, and row spacing uses 0 instead
   of `PAGE_GAP`. Fit-width and fit-page already ask `spread_gap`, so their
   scale follows. The field threads through `ViewState`, the saved `Place`,
   `Defaults`, `settings.toml`, the settings page and the title bar. Chosen.
2. Explicit gap values passed into `arrange`: keeps `LayoutMode` purely
   about which pages pair up, but every call site in `view.rs` must pass
   them and the fit maths needs the value too. More plumbing, no gain.
3. A numeric gap: more general than asked for, and wants a different UI than
   a checkbox.

## Behaviour

- **Gapless mode** on removes `PAGE_GAP` in both directions: rows touch in
  continuous mode, and the two pages of a spread touch in dual mode. The
  `MARGIN` around the column of pages stays.
- **Menu:** "Gapless mode", checked while on, in the section with
  Continuous, Dual page and Odd pages left. Bound to plain `G` (Ctrl+G stays
  "Next match"). Like other plain keys it does nothing while typing in the
  search field.
- Toggling re-arranges as the other mode toggles do: in continuous mode the
  document point at the viewport centre stays put; in paged mode the current
  spread is shown from the top. Fit width and fit page recompute, so pages
  take the space the gap used to take; a custom zoom keeps its scale.
- **Settings page:** a "Gapless mode" checkbox under "New documents open
  with:", `(default: off)`, and "Set from current view" includes it.
- **Per document:** saved with the rest of the view. State files written
  before this setting existed open with gaps on, as they do today.
- The setting does nothing visible in single-page paged mode, where only one
  page is shown, but it is still recorded and applies when the arrangement
  changes.

## Code shape

- `noido-core/layout.rs`: `LayoutMode::gapless` (default `false`);
  `spread_gap` gives 0 when gapless; row spacing and row count use a
  `row_gap` that gives 0 when gapless.
- `noido-core/view.rs`: `View::gapless()`; `ViewState::gapless`;
  `Action::ToggleGapless` carried out by `switch_mode`, which takes the
  gapless flag; `state` and `restore` carry it.
- `noido-core/defaults.rs`: `Defaults::gapless` (default `false`);
  `from_view` copies it.
- `noido-core/keymap.rs`: `Key::G` bound to `Action::ToggleGapless`.
  `noido-core/menu.rs`: `Check::Gapless` and the menu entry.
- `noido-state`: `Place::gapless`, `#[serde(default)]` false.
- `noido-settings`: `NewDocuments::gapless`, default false, in
  `settings.toml` as `gapless`.
- `gui-egui`: checkbox in `settings.rs`; `Bar::gapless` and the check in
  `titlebar.rs`; `same_place` compares it; the harness covers the key, the
  menu check and saving.

## Testing

- Layout: gapless stacks rows without `PAGE_GAP` and puts a spread's pages
  side by side without a gap; margins and centring are unchanged; fit scale
  grows to use the gap's space.
- View: toggling gapless keeps the centre point in continuous mode and shows
  the current spread from the top in paged mode; state round-trips.
- Defaults: default off; `from_view` copies the flag.
- Keymap and menu: `G` → `ToggleGapless`, label "Gapless mode", checked.
- State: a file without the field loads with gaps on; the field round-trips.
- Settings: default off; round-trips; missing field takes the default.
- Harness: `G` removes and restores the gap, the menu shows the check, the
  setting survives reload, and it is not saved into another document's
  state.

## Non-goals

- Changing `MARGIN`, or a gap other than `PAGE_GAP`.
- A numeric gap or a per-side choice.
- A title-bar icon button; the menu and the key are enough, as for the
  other mode toggles.
