# Fullscreen from noido's own menu, off macOS

Date: 2026-11-19. Agreed with the user. A small change: enter fullscreen
from the menu and with F11, and make it a mode with no title bar, off
macOS.

## Intent and why

macOS has a way in already: AppKit's Window → Enter Full Screen (⌃⌘F) and
the green button. Linux and Windows have none of noido's own, and on a
Wayland desktop the compositor cannot put a client into fullscreen unless
the client asks: GNOME's "toggle fullscreen" binding is unset by default.
Leaving fullscreen is already cross-platform (Esc, see
`2026-11-13-esc-leaves-fullscreen-design.md`); only entering was missing.

It is added off macOS only, because AppKit's fullscreen works differently:
its own Spaces transition, its own title bar and toolbar, and the menu
item it already supplies. The old spec's non-goal, "a key of noido's own
to enter fullscreen", is superseded for the other platforms and kept for
macOS.

## Goals

- The title bar menu gets "Full screen" in the same section as "Show
  sidebar", directly below it, checked while the window is fullscreen.
- F11 toggles fullscreen, also while the search field has focus (like
  F3/F9).
- In fullscreen the title bar is gone: the pages get the whole window,
  and with the bar go the search field, the menu and the buttons.
- Esc leaves fullscreen, as the earlier spec has it: one thing per
  press, so a selection is cleared first.
- The search shortcut, Mod+F, leaves fullscreen and puts the cursor in
  the search field, which only comes back with the bar.
- Linux and Windows; nothing on macOS.

## Non-goals

- Any change on macOS: no menu entry, no F11, and its bar stays shown in
  fullscreen as `2026-11-13-macos-fullscreen-title-bar` settled. AppKit's
  Window → Enter Full Screen and the green button stay the way in.
- A button in the title bar.
- New persisted state: eframe already saves and restores the window's
  fullscreen with the rest of the window.

## Approaches

- **Compile-time gate in core (chosen).** `menu()` and
  `Keymap::native()` keep their signatures and branch on
  `cfg!(target_os = "macos")`. The smallest change, and each CI leg
  tests the menu its platform ships. Cost: the macOS menu's shape is
  asserted only on the macOS leg, and core tests carry `cfg`s.
- **An explicit `Platform` argument.** `menu(platform)` and
  `Keymap::native(platform)`, with both shapes testable everywhere.
  Clearer, but it changes every test that builds a keymap or reads the
  menu, for a one-entry difference.
- **A frontend filter.** Core's menu keeps the entry everywhere;
  `titlebar` hides it on macOS and `apply` ignores the action there.
  Least core churn, but the data would carry an entry the platform must
  not show, and the rule would live away from the menu.

## Design

- `noido-core`: `Action::ToggleFullScreen` and `Check::Fullscreen`.
- `menu.rs`: `check(Action::ToggleFullScreen, "Full screen",
  Check::Fullscreen)` directly after "Show sidebar" in that section,
  under `#[cfg(not(target_os = "macos"))]`.
- `keymap.rs`: `Key::F11`, bound plain to `ToggleFullScreen` under
  `cfg(not(macos))`; F11 acts while editing text; `shortcut_label`
  writes "F11".
- `app.rs`:
  - `core_key` maps `F11`.
  - `apply` reads the viewport's fullscreen and sends
    `Fullscreen(!fullscreen)`. Entering drops a pending fit-window
    resize (`shrink`), which would otherwise leave fullscreen again to
    size the window (non-macOS only).
  - `apply` for `FocusSearch`: in fullscreen, off macOS, it sends
    `Fullscreen(false)` first; the focus request waits a frame, until
    the bar is back.
  - `show` skips `title_bar` while the window is fullscreen (off macOS)
    and sets `self.bar` to no rects, so no stale button answers. egui
    drops the focus of a widget it no longer draws, so a field that was
    focused cannot keep Esc for itself.
- `titlebar.rs`: `show` already reads the window's fullscreen for its
  layout; it passes it to `menu_entries`, whose `Check::Fullscreen`
  shows the mark.

## Testing

- Core menu: "Full screen" sits right after "Show sidebar" on non-macOS,
  is checked by the window's fullscreen, and is absent on macOS.
- Core keymap: F11 maps to `ToggleFullScreen`, also while editing; its
  label is "F11"; macOS has no binding.
- Harness, off macOS: F11 sends `Fullscreen(true)` out of fullscreen and
  `Fullscreen(false)` in it; the menu entry does the same; the bar is
  gone in fullscreen (no menu, and the pages lose exactly the bar's
  height) and back after leaving; Mod+F in fullscreen sends
  `Fullscreen(false)` and then focuses the search field; Esc still
  leaves, also after entering fullscreen from the focused search field;
  entering fullscreen while a fit-window request is pending drops it.
- Harness, macOS: unchanged tests still pass, including the bar shown in
  fullscreen.
