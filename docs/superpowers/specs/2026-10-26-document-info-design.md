# Document info

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

Make what a PDF says about itself available: who made it, when, with what,
and in what format. The fonts matter most: the reader is often curious which
typefaces a document uses, to use them or avoid them, so they are shown as
readable family names.

## Approaches (agreed: 1, then changed to 1b while building)

1. The render thread reads it on request. `Document::properties()` in the
   engine trait; the dialog asks the render thread, which owns the open
   document, and shows "Reading…" until the answer comes. A document never
   inspected costs nothing. Agreed, but the render thread's channel carries
   only render requests, and a 600-page font walk there would hold back
   page renders.
1b. **The same, on a short-lived thread** started when the dialog opens: it
   opens its own copy of the document, reads the properties, sends them and
   stops. Still nothing until asked, and renders never wait. Chosen.
2. Read it at load time. Simpler dialog, but every open of a 600-page book
   pays for a font walk, for a window seldom opened.
3. A thread of its own, like search and thumbnails. Never queued behind a
   render, but another thread and another parse of the file; the render
   queue is short.

No dedicated XMP crate: hayro-syntax gives the catalog's `/Metadata` stream,
decoded, and `roxmltree` (already in the lock via the font stack) parses it.
`xmp_toolkit` binds Adobe's C++ SDK, far too heavy.

## Behaviour

- A modal like Settings, opened by "Document info…" in the menu and by
  Ctrl+I (Cmd+I on macOS). Esc and a Close button close it. Only with a
  document open.
- Sections, each row left out when there is nothing to show:
  - **File:** name, folder, size.
  - **Document:** title, author, subject, keywords, creator, producer,
    created, modified (as written, with their UTC offset: no time-zone
    crate). From the info dictionary; XMP fills
    fields the dictionary lacks.
  - **Format:** pages; page size in mm and points, or "mixed"; PDF version;
    tagged; has an outline. (Encryption is left out: hayro doesn't expose
    the trailer.)
  - **Fonts:** grouped by family, each with its styles, e.g. "Minion Pro:
    Regular, Italic, Bold". The `ABCDEF+` subset prefix is dropped. A font
    the PDF only names, not embeds, is marked "not embedded". Text is
    selectable so a name can be copied.

## Non-goals

Font samples (subsets would be spotty), which pages use which font, editing
metadata, encodings and other font internals.

## How

- `noido-engine`: `Properties` (metadata strings, dates, version, tagged,
  outline, fonts as raw names + embedded) and `Document::properties()`,
  empty by default. (`noido_render::DocumentInfo` already names the page
  sizes.)
- `engine-hayro`: hayro's `Metadata`, then XMP; fonts by walking each page's
  resources, including form XObjects, once per object.
- `noido-core`: a pure function from raw font names to families and styles
  (`MinionPro-It` → Minion Pro / Italic, `TimesNewRomanPSMT` → Times New
  Roman / Regular), tested there.
- `noido-render`: a one-shot properties thread.
- `gui-egui`: the modal, tested in the harness.

## Left to the builder

Layout details, date format, the style-abbreviation table.
