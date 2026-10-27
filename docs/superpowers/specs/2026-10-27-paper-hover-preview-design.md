# Paper colour: preview on hover

Date: 2026-10-27 · Status: small change, asked for in conversation

**What.** While the pointer is over a paper square on the settings page,
the document (pages and sidebar thumbnails) is drawn on that paper. Moving
off the square goes back to the chosen paper. Only a click chooses and
saves; hovering saves nothing.

**How.** The settings page remembers which square was hovered on its last
frame (`Page::preview`); the app draws with that paper while the page is
open, and with `settings.paper` otherwise. The switch is instant, not
faded (my call): it is a comparison, and the scrim already clears on the
row. One frame of lag is not noticeable.

**Test.** Harness: hover the sepia square, pages tinted sepia, nothing
saved; move away, white again; click, sepia stays.
