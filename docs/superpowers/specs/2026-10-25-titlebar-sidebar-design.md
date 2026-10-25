# Title bar and sidebar shell

Date: 2026-10-25 · Status: for approval

## Intent

noido needs room for search results, thumbnails and the outline (left) and
later notes (right), plus a few basic controls. Evince's layout is the
reference: controls in the title bar, a sidebar that is invisible until it's
needed, search hits listed in it. This round builds the **shells**: our own
title bar and an empty left sidebar. Search, outline and thumbnails fill the
sidebar in later rounds.

## Behaviour (agreed)

- **Our own title bar on every platform**, no system title bar above it
  (approach A below). Left to right:
  1. sidebar toggle button
  2. a search field, the whole field, not just a button
  3. the document's file name, centred, followed by quiet text
     "12 / 240 · 106 %" (current page, page count, zoom)
  4. a ☰ menu button
  5. on Linux and Windows, our own minimise, maximise and close buttons
  On macOS the native traffic lights stay, at the left, and our bar
  leaves room for them.
- **Page and zoom text is display-only** for now.
- **The ☰ menu holds the controls** to begin with. Controls may move up into
  the bar later.
- **The search field exists from this round**, though search itself comes
  later. It has a reasonable minimum width and never collapses fully, even
  when the sidebar is narrow or hidden. **Enter in it shows the sidebar.**
- **The left sidebar is invisible until needed.** It will hold search
  results, thumbnails and the outline. The right sidebar for notes is a
  later feature (see backlog).

## Proposals (my calls)

- **Search field width:** follows the sidebar's width when the sidebar is
  open, so the field sits right above the results; minimum 160 points;
  200 points when the sidebar is hidden.
- **Search field keys:** Mod+F focuses it (the design document's Find key).
  Esc in the field leaves it: focus returns to the page, and if Enter
  opened the sidebar, the sidebar closes again.
- **Keys while the search field has focus (agreed):** shortcuts with Mod,
  function keys (F9) and Page Up/Down still act; plain letters, digits and
  Space go into the field and never trigger page keys (W, F, C, D…). The
  keymap therefore knows, per binding, whether it applies while text is
  being edited, rather than the frontend dropping all keys; the vim layer
  will later build on the same distinction.
- **Sidebar shell content:** a placeholder line saying what will appear
  ("Search results, thumbnails and the outline will appear here"), muted.
  No views or tabs yet.
- **Sidebar layout:** opens on the left and pushes the page aside (the page
  view gets narrower; fit-width re-fits). Resizable by dragging its edge,
  between 160 and 50 % of the window; default 260 points.
- **Remembered:** sidebar open/closed and its width, per document, in the
  existing per-document state (`noido-state`). New documents start closed
  at the default width.
- **Sidebar toggle shortcut:** F9 (Evince's key), shown in the menu.
- **☰ menu contents**, each with its shortcut shown, drawn from the keymap
  so menu and keys never disagree:
  - Fit width (W), Fit page (F), Zoom in (Mod+=), Zoom out (Mod+-)
  - Continuous (C), Dual page (D), shown as checkmarks
  - Show sidebar (F9), as a checkmark
  - Quit (Mod+Q)
- **Menu as data in `noido-core`:** a list of entries (action, label, kind:
  plain or checkmark) and a keymap lookup from action to its key, as the
  design document asks. The frontend only draws it.
- **New actions in `noido-core`:** `ToggleSidebar`, `FocusSearch`, `Quit`,
  with keys F9, Mod+F, Mod+Q. The sidebar's open state lives in the
  frontend's UI state, not in `View`, since `View` is about the page
  canvas.
- **Window title** (task switcher, alt-tab) stays "name — noido"; the bar
  shows the file name only.

## Linux and Windows: the window frame

The system frame is turned off (`with_decorations(false)`). We then provide
what it did:

- **Move:** pressing on empty parts of the bar starts a window move
  (`ViewportCommand::StartDrag`).
- **Maximise:** double-click on empty parts of the bar toggles maximised.
- **Window buttons:** minimise, maximise/restore, close, at the right,
  drawn as flat icons that highlight on hover; close turns red on hover.
- **Resize:** an invisible band about 5 points wide along each edge and
  corner shows the matching resize cursor and starts a native resize
  (`ViewportCommand::BeginResize(direction)`). Not active while maximised
  or fullscreen.
- **Border:** a 1-pixel border in a subtle colour, so the window's edge is
  visible against same-coloured windows.
- **Known losses on GNOME:** no drop shadow and no rounded corners. Accepted;
  Super+arrow tiling keeps working, since GNOME does it.
- **Minimum window size:** large enough that the bar never overlaps itself
  (sidebar toggle, minimum search field, menu and window buttons): about
  480 × 300 points.

**Windows (my call):** same code path as Linux; builds but is not checked by
hand this round (no Windows machine). Quit stays Mod+Q there too for now;
Alt+F4 is handled by Windows itself even without the system frame. Checking
snapping and edges on Windows goes to the backlog.

## macOS: the window frame

- Full-size content view, title and title bar hidden, title bar buttons
  shown (`with_fullsize_content_view`, `with_title_shown(false)`,
  `with_titlebar_shown(false)`, `with_titlebar_buttons_shown(true)`). Our bar
  draws under the transparent title bar strip.
- Our bar leaves about 76 points at its left for the traffic lights, except
  in fullscreen, where they are hidden.
- Moving: as on Linux, via `StartDrag` on empty parts of the bar.
  Double-click toggles maximise (zoom); the system preference for
  double-click is not read yet.
- **Not in this round, and untested:** centring the traffic lights in a
  taller bar (the empty-`NSToolbar` trick, `objc2`) and translucency. Both
  need a Mac; the code builds for macOS but is checked by hand later. Added
  to the backlog.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. Our own title bar everywhere: system frame off on Linux and Windows, full-size content view on macOS | The look the user wants; on GNOME we draw resize edges, move, maximise and window buttons ourselves, and lose the drop shadow and rounded corners | **Chosen** ("no guts no glory"), without a spike first |
| B. Keep the system frame, our bar just under it | Everything native for free; two stacked bars on Linux, the arrangement the user wanted to avoid | Rejected |
| C. A on macOS, B on Linux until A has been tried | Safe start; two code paths, and the less-liked look on the platform the user works on | Rejected |

## Design

### `noido-core`

- `Action` gains `ToggleSidebar`, `FocusSearch`, `Quit`; `Key` gains `F9` and
  `Q` (Mod+F uses the existing `F` with `command: true`).
- `Keymap::key_for(action) -> Option<KeyPress>` for menu labels (when an
  action has several keys, a stable first one, e.g. Mod+= over Mod++).
- `menu() -> Vec<MenuEntry>`; `MenuEntry { action, label, check: Option<Check> }`
  where `Check` names which state the checkmark shows (continuous, dual,
  sidebar).
- A plain shortcut label formatter (`"Ctrl+="` / `"⌘="`) taking the platform
  as a parameter, so it is testable on any OS.

### `noido-state`

- `Entry` gains optional `sidebar_open` and `sidebar_width`. Missing fields
  read as defaults, so version 1 files keep working (no version bump).

### Frontend (`gui-egui`)

- **Layout per frame:** title bar (top panel) → sidebar (left panel, if
  open) → page view (central). The existing page view code is unchanged
  except that it now sits in the central area.
- **Title bar** as one function drawing the parts above, plus the frame
  behaviour for the platform (window buttons and resize edges only on
  Linux and Windows, traffic-light inset only on macOS), chosen with
  `cfg!(target_os = "macos")`.
- **Keys:** the new actions go through the keymap like the existing ones.
  `handle_input` no longer returns early while a text field has focus; it
  asks the keymap which bindings apply while editing. Mod+F,
  Esc and Enter in the search field are handled by the field.
- **Menu:** egui's menu button, entries from `noido_core::menu()`, labels
  with shortcut text right-aligned, actions applied exactly like key
  presses.

## Non-goals

- Search itself, the text API, thumbnails strip, outline (later rounds).
- The right sidebar and notes.
- Clickable/editable page number or zoom.
- Native menus (`muda`), the macOS traffic-light centring and translucency.
- Drop shadow and rounded corners on Linux.
- Reading the system's double-click and button-layout preferences (GNOME
  can put window buttons on the left; we always put them right).

## Testing

- **`noido-core` unit tests:** new keys map to the new actions; `key_for`
  for every menu action; every menu entry has a key (or is deliberately
  without one); shortcut labels on both platforms.
- **`noido-state`:** a version 1 file without sidebar fields loads with
  defaults; sidebar fields round-trip.
- **Headless app tests (existing harness):**
  - F9 opens and closes the sidebar; the page view gets narrower, and in
    fit-width the zoom changes accordingly.
  - Typing "w" in the focused search field doesn't change the zoom mode,
    but Mod+= still zooms and F9 still toggles the sidebar.
  - Enter in the search field opens the sidebar; Esc closes it again and
    leaves the field.
  - The search field never gets narrower than its minimum, sidebar open or
    closed, at the minimum window size.
  - A menu action (e.g. Dual page) has the same effect as its key.
  - Sidebar state is saved and restored with the document.
- **By hand on GNOME:** move by dragging the bar, double-click maximise,
  resize from all edges and corners, window buttons, Super+arrow tiling,
  and that it looks acceptable without a shadow.

## Left to the builder

- Exact sizes, colours and icons (within egui's theme colours, so light
  and dark both work).
- How the search field's width follows the sidebar (same panel width, or
  measured the previous frame).
- How resize hit-testing is structured.
