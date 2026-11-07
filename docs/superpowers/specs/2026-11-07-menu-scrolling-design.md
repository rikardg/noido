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
(`titlebar.rs`, `menu_entries`), allowed up to the window's full height
(less the popup's margins), and floored at `MENU_LOWEST_ROOM` (96 points)
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

## Tests

Harness, a 900 × 600 window, menu opened:

- the last entry the window has room for sits inside the window, and
  `Quit` is absent;
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
