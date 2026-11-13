# Esc leaves fullscreen

Date: 2026-11-13. Agreed with the user. A small change: in fullscreen,
Esc leaves fullscreen when it has nothing else to do.

## Intent and why

Preview and QuickTime on macOS, and Firefox and GNOME's image viewer on
Linux, leave fullscreen on Esc. In noido, Esc in fullscreen does nothing
unless something is selected; the way out is the system's (⌃⌘F, the
green button, the window manager).

## Goals

- In fullscreen, Esc leaves it when no field has focus, no menu or
  dialog (Document info, Settings) is open, and nothing is selected and
  no highlight is being edited.
- What Esc does now comes first, one thing per press: with a selection,
  the first Esc clears it and the second leaves fullscreen; in the
  search field Esc is the field's, and leaving takes an Esc once the
  field has let go.
- Only a plain Esc, pressed afresh: a held Esc clears the selection and
  stops there, and Esc with Shift, Ctrl, Alt or ⌘ does nothing new.
- Not while a mouse button is down or a drag is under way: the window
  would shrink under the pointer.
- On every platform.

## Non-goals

- A key of noido's own to enter fullscreen.
- The pagelet: Esc does not close it, and leaves fullscreen with it open.

## Design

Where Esc on the pages clears the selection (`App`'s key handling, the
branch that already checks for a field with focus and an open menu):
with nothing selected or being edited, no modal open, and the window
fullscreen, it sends `ViewportCommand::Fullscreen(false)`.

## Testing

Harness: Esc in fullscreen sends `Fullscreen(false)`; it does not when
it clears a selection or stops editing a highlight first, while the
search field or a button focused with Tab has focus (the next Esc
does), while the menu or Document info is open, when held, with a
modifier, with the button down, or out of fullscreen.
