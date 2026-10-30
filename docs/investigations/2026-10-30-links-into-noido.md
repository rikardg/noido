# Links into noido: investigation

Date: 2026-10-30

Status: investigated, not decided.

## Question

How can links to a document and a page be kept outside noido (in notes, a
Markdown file, a wiki) so that clicking one opens noido on that page? It
should be as standard and idiomatic as possible.

## Answer

**Use RFC 8118 fragments (`#page=12`, `#nameddest=sec2`) on two kinds of
URI:**

1. **`file:///home/user/books/x.pdf#page=12`** is the fully standard form
   (RFC 8089 file URI plus RFC 8118 PDF fragment). Firefox, Chrome, Acrobat
   and pdf.js already read it, so the link still works without noido.
   It reaches noido when noido is the PDF app and the click goes through
   the desktop, but browsers open `file:` PDFs themselves.
2. **`noido:///home/user/books/x.pdf#page=12`** is the same URI with the
   scheme swapped. It always reaches noido, from any app that hands unknown
   schemes to the desktop (browsers after one prompt, Markdown editors,
   Obsidian, LibreOffice, terminals that make links clickable), and it
   doesn't depend on which app is the default for PDFs. This is the same
   approach as `zotero://open-pdf/…?page=` and `obsidian://`.

noido accepts both, plus a plain path and `--page N` on the command line,
and offers **"Copy link to this page"** so links never have to be typed by
hand. The `noido:` form is the one to copy by default, because it is the
one that is sure to open noido. Turning one form into the other only
means changing the scheme.

## Where noido stands

- `noido <path>`: one positional path (`main.rs`). No page option, no URI
  parsing.
- The desktop entry (`desktop.rs`) has `Exec=… %f` and
  `MimeType=application/pdf;`.
- A single-instance socket (`instance.rs`): a second launch on an open file
  sends `activate` and exits. It carries no destination.
- The engine already resolves named destinations (`/Names /Dests` and the
  old `/Dests`, `engine-hayro/src/links.rs`), and `View::go_to_target` plus
  history already exist for in-document links.
- Links inside a PDF are opened only for `http`, `https` and `mailto`
  (`links.rs::opens`).

## The standards

- **RFC 8118** (`application/pdf` fragment identifiers): parameters
  `name=value`, separated by `&` or `#`, processed left to right. These are
  defined:
  - `page=` (physical page, **1-based**)
  - `nameddest=`
  - `structelem=`, `comment=`, `ef=`
  - `zoom=scale[,left,top]`, `view=`, `viewrect=`, `highlight=`
  - `search=words`, `fdf=`

  The RFC says nothing about unknown parameters or about `page` and
  `nameddest` together. Ignoring what isn't understood, and letting later
  parameters win, fits "left to right".
- **RFC 8089** `file:` URIs: `file:///abs/path` with spaces etc.
  percent-encoded.
- **Desktop Entry spec**: `%f` is a local path, and `%u` is a URL *or* a
  local path. Scheme handlers are MIME types named `x-scheme-handler/<scheme>`.
- **RFC 7595** asks private schemes to use reverse-domain names
  (`io.github.rikardg.noido:`). Few apps do (zotero, obsidian, vscode,
  slack all use short names). A short `noido:` reads better in notes.

## Tried on this machine (GNOME, GLib 2.88)

A dummy handler that logs its arguments, in a scratch `XDG_DATA_HOME` and
`XDG_CONFIG_HOME`:

| Opened with `gio open` / `xdg-open` | `Exec=… %f` gets | `Exec=… %u` gets |
|---|---|---|
| `file:///…/my%20doc.pdf#page=3` | `/…/my doc.pdf` (fragment **lost**) | `file:///…/my%20doc.pdf#page=3` (intact) |
| `noido:///…/my%20doc.pdf#page=3` | not launched | `noido:///…/my%20doc.pdf#page=3` |
| `/…/my doc.pdf` (a path) | path | path |

- **`%f` must become `%u`.** Then the argument is a path *or* a URI, and
  noido must accept both.
- An app that lists `x-scheme-handler/noido` in `MimeType` becomes the
  handler **without anything written to `mimeapps.list`**, once
  `update-desktop-database` has run (which `--install` already does). Before
  that, `gio` says "location is not supported". So `--install` can register
  the scheme without changing the user's defaults.

## Where links are clicked

- **Markdown editors, Obsidian, VS Code, LibreOffice, GTK apps** pass
  links they don't handle to `xdg-open`/`gio` (Electron's
  `shell.openExternal`, `gtk_show_uri`). Both forms work, and `file:`
  works only if noido is the default PDF app.
- **Firefox and Chrome** open `file:` PDFs in their own viewer (which does
  go to the page). For `noido:` they ask once "open with noido?" and then
  hand over. Browsers often refuse to follow `file:` links from `http(s)`
  pages anyway.
- **Flatpak apps** go through the OpenURI portal. Custom schemes are
  passed to the host's handler. `file:` URIs go through the portal's
  file chooser and may lose the fragment (not tried).
- **macOS** (unchecked): a custom scheme is `CFBundleURLTypes` in the
  `.app`'s `Info.plist` and arrives as a Get URL Apple event (winit/eframe
  need a hook for it). Opening a document through Launch Services drops
  fragments, so there only `noido:` works.
- **Windows** (unchecked): `HKCU\Software\Classes\noido` with
  `URL Protocol` and `shell\open\command = "noido.exe" "%1"`. File
  associations pass a path, not a fragment, so again only `noido:` works.

So `noido:` is the only form that opens noido when clicked on all three
platforms. `file:` is the portable fallback that other readers understand.

## What noido would need

1. **Arguments.** `path` accepts a plain path, a `file:` URI or a `noido:`
   URI. The fragment is parsed only on URIs. A plain path is taken as is,
   since file names may contain `#`. Add `--page N` (1-based, as shown in
   the UI) and maybe `--dest NAME` for the terminal. The URI parser
   (percent-decoding, `page`, `nameddest`, ignoring the rest) is pure and
   belongs in `noido-core`, tested there.
2. **Destination on open.** An explicit destination wins over the
   remembered position and goes through history, so Back returns to where
   the document was last left. An out-of-range page is clamped. An unknown
   named destination is ignored with a message on stderr.
3. **Hand-over.** The single-instance protocol sends the destination along
   (e.g. `activate page=12` / `activate nameddest=x`). The running window
   comes forward and jumps, adding a history entry. Without this, clicking
   a second link into an open book only raises the window.
4. **Desktop entry.** `Exec=… %u`, and
   `MimeType=application/pdf;x-scheme-handler/noido;`. Re-running
   `--install` replaces the old entry (it already handles that).
5. **Making links.** "Copy link to this page" in the menu (and a key),
   giving `noido:///abs/path.pdf#page=N`. Possible variants:
   - a Markdown version: `[Title, p. N](noido:///…#page=N)`
   - the `file:` form
   - later, a link to the place (`zoom=…,left,top`, which RFC 8118
     already has) or to a selection (`search=`, `highlight=`).
6. **Optional, later:** follow `noido:` and `file:…pdf#page=` links found
   *inside* PDFs (`/URI` actions, and `/GoToR` remote go-to actions, which
   are the PDF-native form of the same thing). Today `opens` allows only
   web and mail.

## Things to watch

- **`page=` is the physical page**, 1-based. Books often print other numbers
  (page labels: "xii", "37" on physical page 51). noido shows physical
  numbers today. If it ever shows labels, links stay physical, and a
  non-standard `pagelabel=` could come later.
- **Absolute paths break when files move.** `nameddest` survives a new
  edition of the document, but a move breaks it anyway. An idea for later
  is a fallback that looks the file name up among remembered documents
  (`noido-state` knows the paths and `DocKey`s it has seen), or a
  content-hash form such as `noido:doc/<hash>#page=12`. Neither is
  standard. Not worth it now.
- **Percent-encoding:** the copied link must encode spaces, `#`, `%`, `?`
  and non-ASCII in the path, and the parser must decode them. A `#` in a
  file name is the trap.
- **Security:** a `noido:` link can only make noido open a local PDF
  read-only on some page, so the risk is low. Don't add parameters that
  write (RFC 8118 `fdf=` imports form data, so leave it out).
- **Other readers** use their own CLI flags (`evince --page-index`,
  `okular file.pdf#3`, `zathura -P`), so no CLI convention is worth
  copying. The fragment is the shared standard.

## Sources

- RFC 8118, The application/pdf Media Type (fragment identifiers, §3).
- RFC 8089, The "file" URI Scheme. RFC 7595, URI scheme guidelines.
- freedesktop Desktop Entry Specification (`%f`, `%u`) and Shared MIME-info
  / mime-apps spec (`x-scheme-handler/*`).
- Adobe, "Parameters for Opening PDF Files" (where the fragment syntax
  came from).
- Tried locally: `gio open` / `xdg-open` with logging handlers, GLib 2.88.4,
  GNOME.
