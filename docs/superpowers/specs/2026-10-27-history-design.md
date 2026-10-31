# History: back and forward between jumps

> **Superseded in part:** entries no longer carry a whole `ViewState`; a
> history entry is a `DocPoint`. See
> [2026-10-31-history-point-only-design.md](2026-10-31-history-point-only-design.md).

## Why

Reading a paper, you follow a link to a footnote or a reference and want to
get back to where you were; in a book, you jump to a chapter from a
thumbnail and later want the old place again. The links design deferred
this on purpose (backlog: "Link history"). It works like a browser's,
including being kept between runs.

## Behaviour

- **Jumps add an entry; nothing else does.** Following an internal link,
  clicking a thumbnail, and a search. Scrolling, paging, zooming and
  arrangement changes never add one.
- **A search counts once.** Going to a hit adds an entry only when the view
  isn't already at a hit (`search_state.at_hit` is false): the first jump of
  a search, or Ctrl+G after reading away from the hits. Stepping from hit to
  hit adds nothing, so Back leaves the search in one step. *(My call: a new
  query typed while still at a hit of the old one adds nothing either.)*
- **An entry is where you left, not where you landed.** It is a
  `ViewState`: zoom, scale, continuous/dual/odd-left and the document point
  at the centre. Follow a link from page 3 to 40, read on to 45, press
  Back: page 3. Forward: page 45.
- **Back and Forward** restore the entry with `View::restore`. Going Back
  remembers the current place so Forward returns to it.
- **A new jump after Back drops the forward entries**, as browsers do.
- **At most 100 entries**; the oldest go first.
- **Kept between runs** with the document's state in `noido-state`, so Back
  works after reopening the file. Entries outside the document (a changed
  file) are clamped by `restore` as today.

## Keys, buttons, menu

- Alt+← and Alt+→. `KeyPress` gains an `alt` flag; plain arrows still
  scroll. While typing in the search field Alt+arrows go to the field, like
  other plain keys (`acts_while_editing` unchanged).
- The mouse's back and forward buttons (egui `PointerButton::Extra1` /
  `Extra2`), always.
- "Back" and "Forward" in the menu, showing Alt+← / Alt+→.

## Approaches

- **A (chosen): a `History` type in `noido-core`**, pure: a list of
  `ViewState`s and a position. The app calls it before each jump (link
  follow, thumbnail click, `go_to_hit`) and on Back/Forward. All rules are
  tested in core; the harness checks each jump kind is wired.
- B: built into `View`'s jump methods. Rejected: `View` can't tell a first
  hit from a later one, and `restore` on load must not record.
- C: all in `app.rs`. Rejected: the rules would only be testable through
  the harness; behaviour lives in core in this project.

## Non-goals

- A visible history list or menu of entries.
- Entries for the outline sidebar or GoToR links (they don't exist yet;
  they call the same `History` when they come).
- Flashing the target on Back/Forward.

## Left to the builder

- `History`'s API and the TOML shape of the saved entries (`DocState` stops
  being `Copy`; old state files without history must still load).
- Where in `app.rs` the mouse buttons are read.

## Testing

- Core: entry on leave, back/forward round trip keeps the read-on place,
  forward dropped on a new jump, the cap, empty back/forward does nothing.
- Keymap: Alt+arrows map to Back/Forward; plain arrows still scroll.
- State: history saves and loads; an old file without it loads.
- Harness: a link, a thumbnail click and a first hit each add one entry;
  stepping hits adds none; Alt+← and the back mouse button go back.
