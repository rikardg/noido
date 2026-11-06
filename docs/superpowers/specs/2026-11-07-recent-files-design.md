# Recently opened files in the menu

## Intent

Get back to a document opened a moment ago without going through the
file dialog: a short list of the files opened last, at the top of the
menu. On macOS also `File ▸ Open Recent`, and the files are told to the
system so the Dock icon's recents agree.

## Approaches

*Where the list lives.*
- **Our own list in noido's state directory** (chosen), written when a
  document opens. Straightforward, and "recently opened" means exactly
  that.
- Derived from the per-document state files' `path` and write time.
  Rejected: state is saved while a book sits open, so a document open for
  a week keeps looking fresh; and it would mean reading up to a thousand
  files to draw a menu.

*Where it shows.*
- **A. Cross-platform menu section, a macOS `Open Recent` submenu, and a
  donation to `NSDocumentController`** (chosen): nothing is missing on
  any platform, and the macOS surfaces Mac users reach for are there.
- B. Menu section and donation only, no native submenu: less work, but a
  Mac user's `File` menu has no `Open Recent`, which is the first place
  they look.
- C. Native submenu only: drops the cross-platform half.

Rejected outright: adopting AppKit's document architecture so
`NSDocumentController` builds the list itself. noido keeps its own
windows (a shell, frames of its own), so that would be a rewrite, not a
hook.

## The list

- `recent.toml` in the state directory, beside `open.toml` (the
  per-document files are one level down, in `docs/`): `version`, and
  `paths`, newest first.
- Written when a document's window comes up — a document the reader
  opened, not one restored at startup, so an old window left open does
  not jump to the front on every launch.
- At most **10** entries (macOS's own default). Noting a path already in
  the list moves it to the front; paths are compared canonicalised.
- Read at startup and after. Entries whose file no longer exists are
  dropped when the list is read, and the file catches up on the next
  write; a menu never offers a file that has gone.
- "Clear recent" empties it.

## What the reader sees

The hamburger menu's first section becomes `Open…` and, under it, an
`Open Recent` submenu holding the files, newest first, with `Clear
recent` after them:

```
┌ Menu ────────────────────────────┐
│ Open…                            │   ┌ Open Recent ─────────────────────────────────┐
│ Open Recent                  ▸ ──┼───┤ Dylan Johnson Graph Theory and Linear Alg…pdf│
│ ──────────────────────────────── │   │ Brett Slatkin – Effective Python_ 90 Specif…  │
│ Fit width                    W   │   │ ───────────────────────────────────────────── │
│ …                                │   │ Clear recent                                 │
└──────────────────────────────────┘   └──────────────────────────────────────────────┘
```

`Open Recent` is there with no files too, then disabled. Clicking an
entry opens it in a window as `Open…` would, with no dialog in between.

The entry is the file's whole name — no cutting, no extension hidden —
the same text the native menu and the Dock's recents show for the file,
so one name reads the same wherever it is offered. Where a name is too
long for the screen, the frontend that draws it shortens it there: the
popup at the window's edge, AppKit at the screen's. When two entries
share a file name, each gets its folder's name after it (`paper.pdf —
arXiv`); if that collides too, both get their whole path. The entry's
hover text is the file's whole path.

On macOS the File menu gains `Open Recent` after `Open…`, with the same
labels and `Clear Menu` last (AppKit's own wording there); empty, the
item is disabled. The same writes call `noteNewRecentDocumentURL:`, so
the Dock icon's recents and the system's Recent Items list the files
too; clearing calls `clearRecentDocuments:`. A file chosen from the Dock
arrives as an opened URL, which noido already handles, so it opens the
way `Open…` does.

## Where the code goes

- `noido-state`: `Store::note_recent`, `load_recent`, `clear_recent`, and
  the cap.
- `noido-core` (pure, tested here): `menu::recent(&[PathBuf]) ->
  Vec<RecentEntry>` with the labels and the collision rule, and
  `Action::ClearRecent`.
- `gui-egui`: the title bar draws `Open Recent` and its submenu and
  reports a click; the window hands the path to its `OnPicked`, so the
  shell opens it as it opens any other file. Clearing is a shell request, since the store and
  the other windows are the shell's. The shell keeps the list for its
  windows, notes each document as its window comes up, and tells every
  window and (on macOS) `noido-macos` when the list changes.
- `noido-macos`: `set_recent(&[(String, PathBuf)])` refills the submenu
  (main thread; the labels come from core, so this crate keeps its
  dependencies), `note_recent(&Path)` and `clear_recent()`; item clicks go
  into the opened-URL queue noido already drains, and `Clear Menu` is a
  new `MenuCommand`. `objc2-app-kit` gains the `NSDocumentController`
  feature.

## Testing

- `noido-core`: labels, the collision rules, order, empty list.
- `noido-state`: newest first; a repeat move to the front; the cap; a
  missing file dropped on read; clearing; a file written atomically and
  naming its version.
- `gui-egui`, in the harness: the files are behind `Open Recent` and
  shown whole; clicking one opens that document; opening a document puts
  it at the top of another window's menu; a startup restore does not
  reorder the list; `Open Recent` offers nothing, and is disabled, with
  no files; clearing asks the shell to clear and the submenu empties.
- `noido-macos`: the AppKit side is checked by hand — the submenu's
  labels, the disabled empty state, `Clear Menu`, and the Dock's recents
  for the installed bundle. A dev binary with no bundle identity may not
  be donated to at all; that is the system's, not a bug to chase.

## Non-goals

- Windows jump lists.
- The Linux `recently-used.xbel` (its own issue, `enhancement`): reading
  it would offer files noido never opened, and writing it is a shared,
  user-wide file other applications rewrite.
- Numbered shortcuts (⌘1…) for the entries.
- Recently used folders.
- A list that survives a file being moved: the entry keeps the path it
  had.
