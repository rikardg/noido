# Linux: recently opened files in the desktop's recent list

## Intent

When noido opens a document, the desktop's own recent list should show it,
attributed to noido. On Linux that list is the shared XBEL file at
`$XDG_DATA_HOME/recently-used.xbel` (fallback `~/.local/share`), which GNOME
Files' Recent, the GTK file chooser's Recent and Activities read.

Today a noido document reaches that file only when another program puts it
there: GNOME Files records what it opens, and the XDG portal records what is
picked in noido's Open dialog — under `xdg-desktop-portal-gnome`'s name, not
noido's. Files opened through `Open Recent`, from the command line, or by a
launch that hands over never reach it. The issue's write half fixes that;
the read half (offering other applications' recents in noido's menu) is
deliberately not part of this change.

## Approaches

- **Our own XBEL merge, in `gui-egui`** (chosen). No GTK dependency, no
  desktop needed to test it, and every byte outside the bookmark we touch
  can be preserved. `roxmltree` is already a dependency and its positions
  feature gives node and attribute byte ranges; `url` (workspace) makes the
  file URI.
- `GtkRecentManager` through gtk-rs: exactly GTK's semantics, but it links
  libgtk into every Linux install, noido must start on desktops without GTK,
  and the manager flushes its list on GTK's main loop. Rejected.
- The `recently-used-xbel` crate: rewrites the whole document from a fixed
  model and drops titles, groups, icons and unknown metadata; hard-codes
  `~/.local/share` and ignores `XDG_DATA_HOME`; writes non-atomically; and
  its clear empties every application's list. Rejected.
- A new `noido-linux` crate: the format code has no Linux-only dependency —
  the platform part is one path — and `gui-egui` already owns the Linux
  `.desktop` installer. A module beside `desktop.rs` is enough. Rejected.

## The file

- Path: `XDG_DATA_HOME/recently-used.xbel`, else `~/.local/share/recently-used.xbel`,
  from `directories::BaseDirs`. A missing file is created with the XBEL root
  and the two namespaces:

  ```xml
  <?xml version="1.0" encoding="UTF-8"?>
  <xbel version="1.0"
        xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
        xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  ```

- The bookmark is found by `href` = `Url::from_file_path` of the canonical
  path. There is never a second bookmark for one URI.
- Merge rules, mirroring glib's `GBookmarkFile` (what GTK itself writes):
  - **New bookmark:** `added`, `modified` and `visited` set to now;
    `<mime:mime-type type="…"/>` from the extension; one
    `<bookmark:application name="noido" exec="'noido %u'" count="1"
    modified="now"/>` inside `<info><metadata owner="http://freedesktop.org">`.
  - **Existing bookmark:** set the bookmark's `modified` to now; find the
    application named `noido` (add it with count 1 if absent), set its
    `modified` and `exec` to ours, and increment its `count`. Leave
    `visited` alone, as GTK does, and leave the mime type and every other
    application, group, title, icon and unknown element untouched.
- Timestamps: ISO 8601 UTC with microseconds, GTK's
  `%Y-%m-%dT%H:%M:%S.%fZ`.
- MIME by lowercased extension: `.pdf` `application/pdf`, `.epub`
  `application/epub+zip`, `.cbz` `application/vnd.comicbook+zip`, `.md` and
  `.markdown` `text/markdown`, `.txt` `text/plain`; anything else
  `application/octet-stream`.
- Edit by splicing the original text at the parsed ranges: replace the
  `modified` value, insert into (or create) the applications list, and
  append a new bookmark before the root's end tag. Bytes outside what we
  change stay as they were, so another application's entries and metadata
  survive.
- Write a temporary file in the same directory (mode 0600), then rename it
  over the list; keep the mode of an existing file. There is no lock: GTK
  doesn't take one either, and its `GFileMonitor` reloads on an external
  change, so the worst case is a millisecond-wide last-writer-wins race.
- A file that doesn't parse, or whose root isn't `<xbel>`, is left untouched
  and the error is logged. A failed update never fails the document's open.

## Clear recent

`Clear recent` removes the `noido` application from every bookmark, touches
that bookmark's `modified` (as `GBookmarkFile` does when an application is
removed), and removes a bookmark left with no applications. Other
applications' entries and their bookmarks stay. A missing file is success.
The portal's and Files' entries are not noido's to remove and are left.

## Where the code goes

- `gui-egui/src/desktop_recents.rs`: `Recents`, which is a path and `Clone`;
  `Recents::at(path)` for tests and `Recents::for_desktop()` from the
  environment; `note(&self, path)` and `clear(&self)`. The format code is
  platform-independent and tested on every platform; only the wiring below
  is Linux-only.
- `main::with_recent`, after noting `recent.toml`: note the desktop list too
  (Linux). On Linux every document window is a process of its own, so this
  is the one place a document is opened; the shell is the macOS multi-window
  manager and needs no change.
- `NoidoApp::apply(Action::ClearRecent)`, the branch with no shell (a window
  of its own): clear the desktop list too (Linux). The app holds
  `Option<Recents>`; `with_recent` puts it there. Tests pass a temporary
  file, production builds it from the environment.
- A document restored at startup is not noted: `with_recent` is the
  launch-with-a-file path, and restored windows go through the macOS shell.
- No reading of the desktop's list into noido's menu.

## Testing

- Unit tests over a temporary XBEL: creation when missing (root and both
  namespaces); a new bookmark's fields; a repeat bumps the count and
  `modified` without duplicating and without moving `visited`; a bookmark
  registered by another application gains only noido's entry, its mime type
  and its application data untouched; a bookmark missing `metadata` and
  `applications` gains the smallest valid elements; clear removes only
  noido's application, keeps a shared bookmark and removes a noido-only one;
  URI escaping (spaces, `#`, `%`, non-ASCII); a malformed file stays
  byte-identical.
- `gui-egui` tests: `with_recent` notes a launch in a temporary XBEL; the
  no-shell `ClearRecent` clears it.
- By hand: open a document via GNOME Files, noido's Open dialog, the command
  line and `Open Recent`, and check GNOME's Recents and the file chooser's
  Recent show each under noido; Clear recent removes noido's registrations
  and leaves another application's.

## Non-goals

- Reading the desktop's recents into noido's menu (its own issue).
- Pruning the file (GTK clamps it on its own writes), locking, and GTK's
  `gtk-recent-files-enabled` setting.
- Windows jump lists, already a non-goal; macOS already uses
  `NSDocumentController`.
- Noting a restored document, or one another instance already has open.
