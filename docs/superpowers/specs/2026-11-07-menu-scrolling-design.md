# Scrolling the menu when it outgrows the window

## The problem

The title bar's menu keeps growing: fit-the-window added two entries,
undo and redo will add two more, and the recent files submenu sits in it
too. In a window shorter than the menu, the last sections — `Document
info…`, `Settings…`, `Close window`, `Quit` — are drawn past the window's
bottom edge. They are not scrolled to and not clipped into view; they are
simply unreachable, and a menu that looks complete while hiding `Quit` is
worse than one that visibly scrolls.

## The change

The menu's entries are drawn inside a vertical `ScrollArea`
(`titlebar.rs`, `menu_entries`), allowed the room from the popup's own top
down to the window's bottom (less the popup's bottom margin; amended
2026-11-22, see below), and floored at `MENU_LOWEST_ROOM` (96 points)
so a very short window still gets a usable menu rather than a sliver. The
cap is set on the popup's own `Ui` (`ui.set_max_height`): a `ScrollArea`
alone is still clipped to the height egui hands the popup, which in a
1200-point window left the menu a third of it.

- **Width** still shrinks to the content, so the menu keeps its narrow
  look and the recent-files submenu keeps the room beside it. (Letting the
  width fill the window made the menu as wide as the window and elided
  recent file names to `…` — the `a_narrow_window_still_keeps_the_submenu_beside_the_menu`
  test caught that.)
- **Height** shrinks to the content too, so a menu that already fits shows
  no scroll bar and behaves exactly as before.
- **When it does not fit**, the menu stops at the window's edge and the
  rest is a wheel away.
- **The wheel over the menu scrolls the menu only.** The pages take the
  wheel when the pointer is over them and not over a popup: wheeling a
  popped-up menu used to turn the page behind it as well (`app.rs`,
  `handle_input`, `ctx.layer_id_at`).

### Amended 2026-11-22: the room starts at the popup, not the window

The budget above was the window's height less the popup's margins, but the
popup hangs below the menu button (34 points down on Linux, 46 on macOS),
so in a short window it measured as tall as the window and egui pulled it
*up* to keep it on screen — over the title bar, over the button that opened
it. The budget now runs from the popup's own top to the window's bottom,
less the bottom margin, and never starts above the button: on a window
shorter than the area egui guesses while sizing, the first frame's
placement is already pushed up, and measuring from there kept it there
(the app's 480 × 300 minimum is such a window).

Because the popup now fits below the button in every window, egui keeps
its right-aligned-under-the-button placement there instead of the old
fallback that clamped it to the window's right edge. One consequence is
the submenu: in windows narrower than about 605 points there is no longer
room beside the menu for the recent files, whose "Clear recent" keeps the
submenu about 150 points wide, so egui draws it below its own entry, over
the menu. Its entries stay on the window; how to place them better is not
part of this change. `a_narrow_window_still_keeps_the_submenu_beside_the_menu`
now runs at 700 × 800, where the submenu is still beside the menu with the
name cut, and `the_narrowest_window_keeps_the_recent_files_on_the_window`
pins the 480-point window above.

## Tests

Harness, a 900 × 600 window, menu opened:

- the last entry the window has room for sits inside the window, and
  `Quit` is absent;
- the first entry sits below the menu button, not over the title bar
  (added 2026-11-22, with the amendment above), in the 900 × 600 window
  and in the app's 480 × 300 minimum;
- after wheeling over the menu, `Quit` appears, inside the window;
- a 900 × 1200 window shows the whole menu without scrolling;
- in paged mode, wheeling over the menu scrolls the menu and leaves the
  page where it was.

Two colour tests that reached for `Settings…` now use a top-of-menu entry
(`Fit width`, `Open…`): they assert theme colours, not reachability, and
`Settings…` now lives below the fold.

## Not this change

Reorganising or shortening the menu, keyboard navigation inside it, and
the menu's exact height per theme.
