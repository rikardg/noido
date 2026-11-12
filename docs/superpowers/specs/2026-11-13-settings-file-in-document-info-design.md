# The document's settings file, in Document info

Date: 2026-11-13. Agreed with the user. A small change: the Document info
page gets a "Settings" row with the full path of the file the document's
settings are saved in, a button that copies it, and a button that opens its
folder.

## Intent and why

The reader wants to see where a document's settings live — a file named by
a 64-character hash, in a directory named by the platform — and wants to get
at it: copy the path, or open the folder it is in. Nothing in noido shows
that path today.

## Goals

- A **Settings** row in the File section: the full path, `<state dir>/docs/
  <hash>.toml`, wrapping over as many lines as it takes.
- A copy button (clipboard glyph) that puts the path on the clipboard, and a
  folder button that opens the folder in the desktop's file manager.
- The path is the one in use *now*: after an in-place reload, which re-files
  the state under the document's new hash, the row shows the new name.
- The path is shown even before the file has been written: it is where the
  settings are saved, and a document nobody has changed yet has no file.

## Non-goals

- The window file (`<hash>.window.ron`) and the macOS frame file: the row is
  about settings, as asked.
- Opening the *file* itself in an editor, selecting the file on Linux, or
  moving the state directory.
- Any change to what is saved or where.

## Approaches

1. A plain text row. Simplest, but the point of showing a path is usually to
   use it; a 64-character name is not something to retype. Not chosen.
2. **A row with a copy button and a folder button** — chosen by the user. No
   new pattern beyond a button in a dialog, and nothing hidden on hover.
3. Clicking the path to copy it. No precedent in noido (the title bar's
   copies are menu items); discoverable only through hover text.

For opening the folder:

1. **An external command per platform** — chosen by the user: `xdg-open
   <dir>` on the freedesktop desktops, which are the BSDs' as well as
   Linux's; `open -R <file>` on macOS (Finder opens with the file selected;
   `open <dir>` when the file does not exist yet); `explorer <dir>` on
   Windows. No dependency, and it works unattended.
2. The platform APIs: `NSWorkspace` on macOS and the freedesktop
   `FileManager1` D-Bus interface on Linux. Nicer selection on Linux too, but
   it costs a `zbus` dependency and a session bus that not every desktop runs.

The icon is drawn by hand, like every other icon in noido: two outlines, a
plain sheet behind and one in front whose top-right corner is cut with the
fold's triangle drawn in.

## Design

`noido-state`: `Store::state_path(&self, slot: &Slot) -> PathBuf`, the
public face of the private `state_file(&DocKey)`; the only caller that
matters is the dialog.

`gui-egui`:
- A new `icons.rs` module: the `icon_button` helper moved out of
  `titlebar.rs` (which keeps its own painters and imports it), now taking the
  button's side so the dialog can have a smaller one, and the two new
  painters `paint_copy_icon` and `paint_folder_icon`.
- `info::show` gains the settings path, resolved at draw time from the
  window's persistence by `NoidoApp::info_page` (`Option`, so a window
  without a store shows no row). The page returns what the user asked for
  instead of a bare `close` flag; the app then copies the path
  (`ctx.copy_text`) or opens the folder.
- `open::show_folder(path: &Path)` beside the existing `open(path)`: the command
  above, detached, output discarded, errors left silent as `open` does. The
  harness must not launch anything, so the app holds the function it calls
  and a test can replace it with a recorder.

## Tests

- `noido-state`: `state_path` gives `docs/<hex>.toml` under the store's
  directory for a slot.
- Harness: with persistence attached, Document info shows a "Settings" row
  whose text is that path; clicking the copy button puts the path on the
  clipboard; clicking the folder button records the folder it would open.
  A window without a store shows no such row, and the existing info tests
  are unchanged.
- After an in-place reload the row shows the new hash.

## By hand

Open a document, Ctrl+I: the row shows a path under the state directory.
Click the copy button and paste somewhere — the same path. Click the folder
button: on Linux the state directory's `docs/` folder opens; on macOS Finder
opens with the settings file selected.

## Left to the builder

How the page hands its actions back (an enum against a callback), the exact
button size, the row's inner layout so the buttons stay right-aligned while
the path wraps, and the hover wording ("Copy path", "Open folder").
