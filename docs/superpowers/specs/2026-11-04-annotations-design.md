# Annotations: highlights, regions and notes, with a sidebar to find them

Date: 2026-11-04. Agreed with the user. The overview of a project in four
steps; each step gets its own design (step 1:
`2026-11-04-text-selection-design.md`).

## Intent and why

noido is for working with documents such as academic texts and RPG
rulebooks. The reader marks things on the page and writes notes, and
later finds them again, typically for one of three reasons:

- "How did magic checks work again?": recall by topic.
- "Here's the list of spells": a place to look things up in, again and
  again.
- "An important fact, what I think about it, and that other article":
  a longer note.

The **right sidebar is the centrepiece**: the place where notes are made
and found. Annotations on the page are how they're anchored.

## Goals

- **Text selection and copy**, also on scans with an OCR layer (hayro
  passes invisible OCR text through like any other; search uses it
  today).
- **Highlights** of selected text, in a few colours. A highlight can
  carry a note and tags.
- **Regions**: a rectangle on a page, kept as coordinates and drawn
  live from the PDF, never stored as an image. It can carry a note and
  tags and be copied to the clipboard as an image (for pasting a figure
  elsewhere).
- **Point notes**: a note pinned to a spot on a page.
- **Notes** are freeform text, multi-line, stored as Markdown, shown
  plainly for now. A link to another article is text in the note.
- **Tags**: an optional field on any annotation, not parsed from the note
  text. The field suggests tags already used in the document.
- **The right sidebar** lists notes, regions and highlights that carry a
  note (plain highlights are left out):
  - in document order, grouped under the outline's chapters when the PDF
    has an outline;
  - a filter box matching note text, quoted text and tags; clicking a tag
    filters by it;
  - a region's entry shows the region rendered live, so the sidebar works
    as a cheat sheet;
  - clicking an entry jumps to it;
  - annotations that couldn't be placed (below) are listed, marked so.
- **Undo** for adding, changing and deleting annotations.

## Non-goals (for now)

- Nothing is written into the PDF; everything is document state.
- **Export.** Later; formats to be discussed. The data model below is
  close to the W3C Web Annotation model, so its JSON is an easy target,
  and Markdown is the obvious one for reading.
- **Anything across documents**: no shared library, no cross-document
  links or search.
- **Rendering Markdown documents.** Planned for some time later; the
  anchoring is chosen so it carries over (Markdown is edited often, so
  anchors must survive edits).

## Anti-goals

- Losing an annotation without a trace. One that can't be placed after
  the document changed is shown as such, never dropped.
- Annotations depending on the zoom or layout: all positions are in page
  coordinates (PDF points).

## Anchoring

Each annotation stores several ways of finding it, tried in order (like
W3C's selectors; Hypothesis anchors notes on changing web pages this way):

1. **Position**: page and character range for a highlight; page and
   rectangle for a region; page and point for a point note.
2. **Quote**: the exact text, with about 32 characters before and after.
   Regions and point notes store the text inside the region or the words
   nearest the point, so they can follow the text too.
3. **Fingerprint**: the document's content hash when it was made.

Placing one: with the hash unchanged, the position is trusted. Otherwise
the quote is searched for near the old position with `noido-text`'s
search (folding, accents, de-hyphenation), matches ranked by how well the
surrounding text agrees; failing an exact match, a fuzzy one (bitap or
edit distance) above a threshold; failing that, the annotation is
**orphaned**. A re-placed annotation has its position and fingerprint
updated, so the search happens once. Regions on pages without text (scans
without OCR) keep page and rectangle only.

## Storage

In the existing per-document state file (`noido-state`, TOML, keyed by
the content hash, found by path after an edit in place). The user's call:
it is well contained and can be split out later if needed.

## Selecting (decided for all steps)

Shift+drag selects; a select tool (icon, shortcut, menu item) makes a
plain drag select, and then Shift+drag pans. Releasing shows a small bar
by the selection: Copy in step 1, the highlight colours from step 3.
Details in step 1's design.

## Steps

Each is designed, built and reviewed on its own:

1. **Text selection and copy**, with the select tool.
2. **The annotation model**: types, anchoring and placing, storage, undo;
   pure and tested in `noido-core`/`noido-state`, no UI.
3. **On the page**: making highlights, regions and point notes; drawing
   them; the note and tag editor; copying a region as an image.
4. **The right sidebar**: list, grouping, filter, tags, live region
   previews, jumping, orphans.

## Approaches considered for the whole

- **Saving into the PDF** (standard PDF annotations): ruled out by the
  user; nothing is written to the document.
- **Anchoring by page and coordinates only**: simplest, but annotations
  go wrong silently when a document is edited, and it doesn't carry over
  to Markdown. Chosen instead: position first, quote as a fallback.
- **A separate annotations file**: safer for the user's own work, but
  the state file is well contained; split later if needed.
