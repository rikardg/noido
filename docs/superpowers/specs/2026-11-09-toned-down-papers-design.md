# Paper colour: sand and two darker greys

Date: 2026-11-09 · Status: small change, agreed in conversation

**Why.** Bisque (`#FFE4C4`) was the most toned-down paper for night
reading, but it is still a full-saturation cream at 88% lightness: in a
dark room it glows. Grey, the other night choice, had nothing below it.
Three papers were added: one in bisque's family and two grey steps.

**What.**

| Name | Colour | Where | Against black |
|---|---|---|---|
| Sand | `#EBCDA8` | after Bisque | 13.8:1 |
| Dark Grey | `#C8C8C8` | after Grey | 12.6:1 |
| Darker Grey | `#C0C0C0` | after Dark Grey | 11.5:1 |

Sand is bisque's hue (33°) with the saturation pulled from 100% to 62%
and the lightness from 88% to 79% — bisque after dark, still creamy.
The two greys keep the hue and saturation of grey and step the lightness
down from 83% to 78% and then 75%. A single `#CCCCCC` step below grey
was tried first and dropped as too small to read as a paper of its own.

The row is therefore: Original, Sepia, Bisque, Sand, Grey, Dark Grey,
Darker Grey. It is a descent with one rise: Sand is darker than the grey
that follows it, because each new colour sits by the one it steps down
from. Seven 20 px squares plus the label still fit the 320 px dialog.

All of them keep at least 11:1 against black, well above the 4.5:1 that
WCAG AA asks of small text (and the 7:1 of AAA), so readability is not
the limit on how far the dimmest paper could go — comfort in a dark room
is.

**Names.** Readable on hover and stable as `settings.toml` values
(`sand`, `dark-grey`, `darker-grey`); existing names and values are
unchanged, so a settings file written by an older build still loads.
A build older than this change loading `dark-grey` falls back to white,
as the contract says any unknown name does.

**How.** `Paper` gains three variants in the order above, each with its
`rgb()` and `name()`; `noido-settings` gains the three TOML names.
Pages tint by multiplication as before, so a scan whose white is 90%
ends up about 10% darker than the swatch: Darker Grey on a scan is
dimmer than it looks in the settings row.

**Tests.** `noido-core`: the new colours, the listed order, that each
added paper is darker than the one it follows, and that all three keep
at least 11:1 against black. `noido-settings`: a table pinning every
paper's spelling in the file, so a rename cannot silently reset users to
white. `gui-egui`: the settings row draws one square per paper, left to
right in the listed order — which also catches the row wrapping.
