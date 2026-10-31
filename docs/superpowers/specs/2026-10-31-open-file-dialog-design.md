# Open file dialog

Date: 2026-10-31 · Status: small change, agreed in conversation

## Intent

Opening another document from within noido: an "Open…" menu entry and Mod+O
(Cmd+O on macOS) raise the desktop's own file picker; a picked PDF opens in
a new window.

## Behaviour

- Menu: a new first section with one entry, "Open…"; it shows its shortcut.
- Mod+O picks a file, also while a text field has focus (every Mod key acts
  while editing). Plain O keeps flipping odd-pages-left.
- The picker is the desktop's native dialog — AppKit's open panel, Windows'
  file dialog, GTK's chooser on Linux — via `rfd`, offering PDFs (filter
  "PDF document", `*.pdf`). It runs on a thread of its own: the call blocks
  until the desktop answers, so the window keeps painting while the dialog is
  up.
- Cancel picks nothing; nothing happens.
- Picking a PDF launches a new `noido <path>` process, detached, its output
  discarded. A file already open elsewhere is not opened twice: the new
  process hands over to the window that has it (existing single-instance
  behaviour) and exits. The current window is untouched either way.

## Code shape

- `noido-core`: `Action::OpenDocument`, bound to `command(Key::O)` in
  `Keymap::native()`; `View::apply` passes it to the frontend with the other
  non-view actions; `menu()` grows the section.
- `gui-egui`: workspace dep `rfd = "0.17"`; a new `open.rs` with `pick_pdf`
  (the rfd call, on a thread of its own, handing the pick to a callback) and
  `open(path)` (the detached launch); `NoidoApp` holds the two as hooks `pick`
  and `open` — the real functions by default, so tests press Mod+O and click
  the menu entry against fakes without a dialog or a spawned process.

## Testing

- core: Mod+O maps to `OpenDocument`, also while editing; the menu test gains
  the section; every menu action still has a key.
- gui-egui Harness: Mod+O and the menu entry call the picker, and a picked
  path reaches the opener; a cancelled pick opens nothing. The real picker
  and the launch are thin and checked by hand — including that the window
  keeps painting while the dialog is up.