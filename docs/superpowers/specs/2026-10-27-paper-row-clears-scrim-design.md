# Paper row clears the settings scrim

Date: 2026-10-27 · Status: small change, agreed in conversation

**Why.** The settings page dims the document behind it, so the paper
colour can't be judged while choosing it.

**What.** While the pointer is over the Paper row (label and squares as
one area), the settings page's scrim fades to clear in about 0.15 s, and
fades back when the pointer leaves. Only the scrim changes; the dialog
stays as it is.

**How.** The page remembers whether the row was hovered last frame; the
scrim's alpha (egui's default, 100/255 black) is animated from that with
`animate_bool_with_time` and given to `Modal::backdrop_color`. One frame
of lag is not noticeable.

**Approaches considered.** Clearing on keyboard focus as well (backlog);
moving the dialog away from the page (not now).

**Test.** Harness: hover the row, let the fade finish, no scrim; move
away, the scrim is back.
