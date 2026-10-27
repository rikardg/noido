# Paper row: no gaps, and a fade between colours

Date: 2026-10-27 · Status: small change, asked for in conversation

**What.**
- Moving the pointer between two paper squares never passes through the
  chosen paper: each square's hover area reaches halfway to its
  neighbour, so the squares' areas touch. The squares look as before.
- The colour the document is shown on fades to a new paper instead of
  switching: on hover, on leaving the row and on a click. 0.15 s, as the
  scrim (my call). A document opens on its paper without a fade.

**How.** Each square takes a cell as wide as the square plus the old gap,
with no spacing between cells, and is painted centred in it. The app
animates the tint channel by channel towards the paper to show
(`animate_value_with_time`); the pages and the sidebar both use it.

**Test.** Harness: the point midway between two squares previews one of
them; a frame after a hover the tint is between the two colours, and
settles on the new one.
