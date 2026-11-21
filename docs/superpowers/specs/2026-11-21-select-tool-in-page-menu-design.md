# The select tool moves to the page's right-click menu

Date: 2026-11-21. Agreed with the user. A small change: the select tool
leaves the title bar's toolbar and joins the menu a right-click on a page
opens.

## Intent and why

The select tool has an I-beam button in the title bar, beside the sidebar
toggle, lit while the tool is on. The toolbar is the smallest it can be —
one button — and the select tool is worked on a page, not on the window.
The page's right-click menu is where the page's actions already live, so
the tool belongs there, and the title bar keeps only the sidebar toggle
next to the search field.

Shift+drag selects without the tool, so the button was never the only way
in. The `S` shortcut and the application menu's own "Select text" entry
stay.

## Goals

- The I-beam button leaves the title bar: the sidebar toggle, the search
  field, the menu button and the window buttons are as they are, and the
  search field starts where the sidebar button ends.
- The page's right-click menu gains **Select text** at its top, in a
  section of its own above the two Copy link entries, checked while the
  tool is on, with `S` as its shortcut on the right. The mark, the label
  colour and the row shape are the menu's existing ones.
- Choosing the entry toggles the tool and closes the menu, as every action
  entry in that menu does.
- `S` still toggles the tool, and the application menu's "Select text"
  entry stays, checked the same way.
- Selecting, the selection bar, highlights and every other use of the tool
  are unchanged.

## Non-goals

- Changing how a selection is made, what Shift+drag does, or the shortcut.
- The entry in the Pages and Pins tabs' right-click menus, or on the desk
  around the pages; the page menu is the one asked for.
- Another toolbar button for the tool elsewhere.
- Removing the `S` shortcut or the application menu's entry.

## Approaches

- **Top of the page menu, in its own section (chosen).** The moment you
  right-click a page to select from it is the moment you want the tool,
  and a mode that stays on should not sit under a list of one-shot
  actions. Its own section keeps it apart from the links.
- **After the view entries, before Rotation.** Groups it with the menu's
  settings, but puts it below the view mode row and five view entries, so
  the thing the right-click was for is the hardest entry to reach.
- **In the same section as the view entries.** No rule separates the
  pointer's mode from the page's layout, and the section grows to six
  checkable rows that do different things.

## Behaviour

- The entry comes from the same core menu data as the application menu's:
  one `MenuEntry` for `Action::ToggleSelectTool`, its label and
  `Check::SelectTool`, so the two menus cannot drift apart. The page menu
  draws it with the mark helper it already uses.
- `rotation_menu::State` gains `select_tool: Option<bool>`: the page in
  the main view passes `Some(on)`, a thumbnail or pin card passes `None`
  and its menu leaves the entry out. Its `checked` answers
  `Check::SelectTool` from it instead of always false.
- The choice is `Choice::Action(Action::ToggleSelectTool)`, queued and
  applied as the other page-menu actions are.
- The title bar loses its select button, `Rects::select` and
  `paint_select_icon`. `Bar::select_tool` stays, for the application
  menu's check.

## Tests

- The title bar no longer draws the select button, and the sidebar
  toggle, search field and menu button keep their places.
- A right-click on a page offers "Select text" above the Copy link
  entries; unchecked with the tool off, checked with it on.
- The menus on a thumbnail and a pin card keep the entry out.
- Choosing it turns the tool on and closes the menu; choosing it again
  turns the tool off.
- The existing `S` and application menu test loses its button click and
  keeps testing both paths.
