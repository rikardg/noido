# Settings: sidebar shown or hidden

Date: 2026-10-26 · Status: small change, proposed

## Intent

The settings page chooses how a document noido hasn't seen opens; whether
the sidebar is shown is part of that, and is toggled the same way (F9, the
menu's "Show sidebar").

## Behaviour

- A fifth row on the settings page, after "Odd pages left":
  `Show sidebar (default: off)` [ ].
- A document with no saved state opens with the sidebar shown or hidden as
  set; a document with saved state keeps its own, as for the other rows.
- "Set from current view" copies whether the sidebar is shown, too.
- Only shown/hidden; the sidebar's width stays per document, starting at
  today's default.
- `settings.toml`: `sidebar = true|false` in `[new_documents]`; files
  without it read as hidden.

## Code shape

- `noido-core`: `Defaults.sidebar: bool` (default false);
  `Defaults::from_view(&ViewState, sidebar: bool)`.
- `noido-settings`: the field in `NewDocuments`, `#[serde(default)]` as the
  others.
- `gui-egui`: `with_settings` sets `self.sidebar.open`; the row and
  `from_view` call in `settings.rs`.

## Testing

- Core: `from_view` carries the sidebar.
- Settings: round trip with it; a file without it reads as hidden.
- Harness: a new document opens with the sidebar shown; saved state with
  it hidden wins; the row is drawn and clicking it saves; "Set from current
  view" copies it.
