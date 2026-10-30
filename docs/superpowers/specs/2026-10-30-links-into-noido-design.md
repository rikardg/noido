# Links into noido: design

Date: 2026-10-30

Background and the tests behind it: `docs/investigations/2026-10-30-links-into-noido.md`.

## Intent

The user keeps links to PDFs and pages outside noido: in Markdown notes, a
wiki, any document. Clicking one should open noido on that page, or bring
forward the window already showing that PDF and jump there. Links come from
noido itself ("copy link to this page"), never typed by hand. The links
should be as standard as possible, so they keep some meaning without noido.

## Goals

- noido opens on a page given by a `file:` or `noido:` URI with an RFC 8118
  fragment, or by `--page N`.
- A link to a PDF that is already open makes that window come forward and
  jump, as a history step.
- `noido --install` makes `file:…#page=` links (when noido is the PDF
  app) and `noido:` links (always) arrive with their fragment.
- Two commands copy a link to the current page: bare, or as Markdown.

## Non-goals (each has an issue)

- Copying a link to a named place (#3). Incoming `#nameddest=` links are
  followed. Only copying is left out.
- A visible "Link copied" notice (#4). Copying is silent for now.
- A visible error when the file can't be opened, e.g. a broken link or a
  broken file (#5). It stays on stderr, as today.
- Following `noido:` or `file:` links found *inside* PDFs, and `/GoToR`.
- macOS and Windows registration of the scheme (Info.plist, registry).
  The parsing and hand-over are cross-platform; only Linux gets the
  desktop entry, as today.
- Fallbacks for moved files, page labels, and RFC 8118 parameters beyond
  `page` and `nameddest`.

## Anti-goals

- Changing the user's default apps or writing `mimeapps.list`. Registering
  the scheme through `MimeType=` in noido's own desktop entry is enough
  (tried: once `update-desktop-database` has run, the only app that lists a
  scheme handles it).
- A plain path being read as a URI: file names may contain `#`, `%` or
  `:`. A fragment is only read from something that is a URI.
- Losing the user's remembered state. Zoom, layout, sidebar and history
  stay remembered; a link only sets the position.

## Approaches

**Link form.** Picked: `noido:` is the `file:` URI with the scheme swapped,
`noido:///abs/path.pdf#page=51`. One parser handles both, and the fragment
is RFC 8118's. Rejected: Zotero-style `noido://open?file=…&page=51`, which
makes room for other commands but is a second format next to the standard
one, and longer in notes. A host part (`noido://something/…`) stays free for
later. Rejected: a reverse-DNS scheme as RFC 7595 asks
(`io.github.rikardg.noido:`), because it reads badly in notes and the apps
it would compete with (zotero, obsidian) don't do it either.

**Hand-over to an open window.** Picked: extend noido's own single-instance
socket (`instance.rs`) to carry the destination. It is small, cross-platform
and tested. Rejected: GNOME's D-Bus `org.freedesktop.Application.Open`,
which is the GNOME idiom but Linux-only, replaces something that works, and
assumes one instance per app where noido has one window per file.

## What gets built

### 1. The argument (pure, in `noido-core`)

`path` stays one positional argument and is read as:

- **`file:` URI** (scheme case-insensitive, starting `file://`): `file:///abs/path` or
  `file://localhost/abs/path`, percent-decoded to a path. Any other host is
  an error ("not a local file"). Fragment parsed as below.
- **`noido:` URI** (starting `noido://`): the same rules with the other scheme.
- **Anything else**: a plain path, taken as is. Nothing is decoded and no
  fragment is read.

On Windows `file:///C:/x.pdf` decodes to `C:\x.pdf`. That case is
unchecked, as is everything else on Windows.

**Fragment** (RFC 8118): parameters `name=value`, separated by `&` or `#`,
taken left to right, values percent-decoded. Understood:

- `page=N`, 1-based, a positive integer
- `nameddest=NAME`

A later understood parameter replaces an earlier one, so `page` and
`nameddest` are one slot. Everything else, including malformed values
(`page=0`, `page=x`), is ignored. Nothing in the fragment is an error.

**`--page N`** (1-based, N ≥ 1, else clap rejects it) gives a page for a
plain path. Together with a URI fragment, `--page` wins.

The result is a path plus an optional destination, `Page(index)` (0-based
inside noido) or `Named(String)`. The parser and its tests live in
`noido-core` (a new module, e.g. `target.rs`). `main.rs` only calls it.

### 2. Opening at the destination

After the document is loaded and the remembered state restored:

- **`Page(i)`**: go to page `min(i, last)`. Same placement as going to a
  page from the thumbnails sidebar today: the top of the page at the top
  of the window.
- **`Named(name)`**: resolve through the engine (a named destination
  lookup; `engine-hayro/src/links.rs` already has `named`) to a
  `Destination`, then `View::go_to_target`. Unknown name: a line on stderr,
  stay at the remembered position.

The jump is a history step: the restored position is left (`leave_here`) so
Back returns to it. If nothing was remembered, there is nothing to go back
to and no entry is added.

### 3. The hand-over

Today a later launch on an open file sends `activate` and gets `ok`. It
becomes:

- `activate` (no destination), or
- `activate page=N` / `activate nameddest=NAME`, using the same
  fragment syntax (percent-encoded), so the running noido parses it with
  the same parser.

The listener passes the destination along with the activation request. The
window comes forward as today, then jumps exactly as in 2, including the
history step from wherever the reader is. Several hand-overs before a frame
runs: the last destination wins. Compatibility with an older running noido
is no concern at this stage.

The message printed by the launch that hands over mentions the page when
there is one.

### 4. The desktop entry

`Exec=<exe> %u` instead of `%f`, and
`MimeType=application/pdf;x-scheme-handler/noido;`. `--install` already
replaces a different existing entry and runs `update-desktop-database`.
The user runs `noido --install` once more.

### 5. Copying links

Two actions, in `noido-core`'s keymap and menu, in a section of their own
right after Back / Forward:

| Menu label | Key | Clipboard |
|---|---|---|
| Copy link to page | Mod+L | `noido:///home/u/books/Some%20Book.pdf#page=51` |
| Copy Markdown link to page | Shift+Mod+L | `[Some Book, p. 51](noido:///home/u/books/Some%20Book.pdf#page=51)` |

- **Page:** the page at the window centre (`View::centre_page`), shown
  1-based, the same number the title bar shows.
- **Path:** the document's canonical absolute path (as `instance::claim`
  already computes it). Percent-encoded per RFC 3986 path rules, with
  parentheses also encoded, so the link survives as a Markdown link target.
  Non-ASCII is encoded as UTF-8. A path that isn't valid UTF-8 is encoded
  byte by byte (Unix).
- **Markdown label:** the name the title bar shows, with a trailing `.pdf`
  removed (case-insensitive), then `, p. N`. `\`, `[`, `]` are
  backslash-escaped.
- The URI builder sits next to the parser in `noido-core`, and the two are
  tested round-trip, including file names with `#`, `%`, spaces,
  parentheses and non-ASCII.
- Copying goes through egui (`ctx.copy_text`). No notice (#4).
- No pages: the actions do nothing.

## Constraints

- TDD throughout. The parser and builder are tested in `noido-core`, the
  hand-over in `instance.rs`'s tests, and opening at a destination and the
  copy actions in the `Harness` (`app.rs`).
- Never launch real apps from tests. The `gio` behaviour was checked by
  hand (see the investigation) and is checked again by the user after
  `--install`.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Settled after the builder check (the author's calls)

- **Named places resolve later.** The app holds no engine document, and
  links are read on the `Linker` thread, so a `nameddest` lookup answers a
  frame or more later. It is applied when it arrives, unless a newer
  destination replaced it meanwhile. It is dropped if the file reloaded
  in between.
- **What counts as a URI:** only `file://…` and `noido://…` (scheme
  case-insensitive). `file:/abs` with one slash, or a file literally named
  `noido:x.pdf`, is a plain path. noido and `gio` both write `file:///`.
- **Arrival looks like a sidebar jump:** `go_to_page`, then set `chosen`,
  with no flash (unlike following a link inside the PDF). An open info
  page or settings dialog is left as it is.

## Left to the builder

- Module and type names, and how the destination travels from the
  listener thread to the app (extend `Activation` or a sibling).
- Whether the named lookup is a new `Document` method or reuses the link
  code. It should be the same resolution code either way.
- Exact wording of stderr messages.

## Checked by hand afterwards

- `noido --install` again. Then from a terminal:
  - `gio open 'noido:///…/x.pdf#page=51'`
  - `gio open 'file:///…/x.pdf#page=51'` (with noido as the PDF app)
  - `noido x.pdf --page 51`
- Clicking a `noido:` link in a Markdown editor and in Firefox.
- A second link into an open PDF jumps, and Back returns.
- Copy link and Copy Markdown link, then paste both somewhere and click.
