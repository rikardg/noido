# A darker desk behind the pages, derived from the theme

Date: 2026-11-01 · Status: small change, agreed in conversation

## Intent

In a light theme the pages have almost no edge: the desk they sit on is the
theme's window background, which is `#fafafb` in Adwaita light against white
paper, a contrast of 1.05:1. Dark mode is fine (`#222226`, about 17:1) and is
to stay exactly as it is.

Rather than fetching another colour from the platform (the themes' darker
entries are control steps, not a workspace, and some themes have none), the
desk is *derived*: the theme's own background with its lightness stepped
down, so a tinted theme keeps its tint.

## Behaviour

- The desk is the theme's `defaults.background_color` with its OKLab
  lightness lowered by **0.12**, a and b left untouched (hue and chroma
  preserved). Adwaita light `#fafafb` becomes `#d2d2d3`, 1.51:1 against white
  paper, next to the 1.05:1 of today.
- Only when that background is light, i.e. its OKLab lightness is above 0.5.
  A dark background is left alone, so Adwaita dark stays `#222226` without
  the mode being consulted at all: the rule is about the colour, not the
  variant.
- The desk is the fill of the central panel only, the area the pages are laid
  out in. The sidebar, title bar, dialogs, menus and tooltips keep their own
  native fills, and `panel_fill` itself is unchanged, so nothing else in the
  window moves by a shade.
- No setting, no state change: the colour follows the theme.

## Approaches (my call: 1)

1. **A `Surface::Central` fill from a `Theme::desk` calculation.** The enum
   variant already exists in `theme` with no caller; `app.rs` builds the
   central frame through `Theme::frame` like the title bar does, and the
   desk is painted by that frame alone. Chosen.
2. Set `style.visuals.panel_fill` to the darkened colour in `global()`. One
   line, but `panel_fill` is egui's generic panel fill and the fallback for
   `noninteractive.bg_fill`; the page area would then be darkening every
   surface nobody gave a fill of its own.
3. A constant grey and its dark twin. Totally predictable, but it drops the
   theme's tint, which is the point of deriving.

## Code shape

- `gui-egui/theme/convert.rs`: `pub(crate) fn darken(colour: Color32, step: f32) -> Color32`,
  the sRGB → linear → LMS → OKLab trip and back, lowering L by `step`.
- `gui-egui/theme/mod.rs`: `const DESK_STEP: f32 = 0.12`,
  `const LIGHT_BACKGROUND: f32 = 0.5` and
  `pub(super) fn desk(background: Color32) -> Color32`, which holds the guard.
- `gui-egui/theme/style.rs`: `Surface::Central` fills with `desk(background)`;
  the margins stay those of `Frame::central_panel`.
- `gui-egui/app.rs`: the page area's frame comes from
  `Theme::current(ui.ctx()).frame(...)` instead of `egui::Frame::central_panel`
  (its `total_margin` use above is unchanged).
- No changes to `native-theme`, settings, state files or the other surfaces.

## Testing

- `darken`: a zero step returns the colour unchanged; the Adwaita light
  background with 0.12 gives `#d2d2d3`; a and b survive within a rounding
  error; black stays black.
- `desk`: Adwaita light `#fafafb` gives `#d2d2d3`, at least 1.35:1 against
  white; a dark background such as Adwaita's `#222226` is returned as it is,
  whatever the variant.
- Harness: in light mode the page area, below the title bar, is filled
  with `#d2d2d3` and the panel fill is not; in dark mode it is still filled
  with the theme background.

## Non-goals

- Changing dark mode, the paper tint, the page outline or adding a page
  shadow.
- A setting for the desk colour.
- Reworking `panel_fill` or any other surface's fill.
