# Native theme styling

Date: 2026-11-01 · Status: project, proposed

## Intent

noido draws every control with egui's default colours and metrics. The title
bar and menu use different egui defaults for what looks like the same text,
there is no shared palette, and the reader's system accent colour is ignored.
Make the whole UI follow the OS theme — colours, accent, fonts, strokes,
radii and spacing where egui can carry them — while keeping noido's own
structure (compact title bar, page layout, sidebar).

The theme data comes from [`native-theme`](https://crates.io/crates/native-theme)
0.7, which reads GNOME (XDG portal and gsettings), KDE, macOS and Windows.
We use its data model only, not `native-theme-egui`: the connector pins egui
0.36.2 and noido is on 0.37. The mapping into egui is ours.

## Spike findings

Checked against this GNOME/Wayland machine with `native-theme`'s `linux-portal`,
`watch` and `system-fonts` features:

- `SystemTheme::from_system()` returns the `GNOME` theme over the `adwaita`
  preset, both light and dark resolved, in ~39 ms before fonts load.
- The reader picked up this machine's green accent (`#3a944a`) in the active
  variant; the other variant stays the preset's until it is the active one.
  The GNOME reader reads the active variant only.
- Dark reported `Noto Sans` 13.33 px and `Adwaita Mono` 14.67 px; switching
  to light mode and re-reading reported the same font values and the green
  accent in the light variant.
- `watch::on_theme_change` fired `ThemeChangeEvent::Changed` on both
  direction changes; after `detect::invalidate_caches()` and a re-read the
  new active variant was correct.
- The Adwaita preset states `window.title_bar_font` explicitly as
  `Adwaita Sans`, so it does not inherit the GNOME font. The title bar must
  either use `defaults.font` or fall back when that family is absent.

## Approaches (agreed: 2)

1. **One global egui `Style` per mode.** Map the theme's defaults and the
   common widget data into `Visuals`, `Spacing` and the text styles. Smallest,
   but flattens native per-widget differences: a menu entry looks like a
   normal button, and a dialog cannot have its own padding and radius.
2. **Global style plus scoped roles. Chosen.** The global style covers the
   common case; a `theme::scope(ui, Role, …)` helper applies the native
   definition for the subtree that needs one — menu, dialog, input, sidebar,
   tooltip, button. Only the roles noido uses.
3. **A full connector-equivalent mapping.** All 26 widget themes, role
   variants, focus-ring plugin, surface frames, icon loaders. Closest to the
   connector we decided not to take; largely unused by noido.

The theme code stays a module in `gui-egui`, not its own crate: it depends on
egui, has one consumer today, and is expected to churn while the mapping is
learned. The boundary that matters is that `native_theme`'s types do not leak
into `app.rs`; if a second consumer appears, the module can move to a crate.

## Goals

- Every element noido draws uses the OS theme: title bar, search field, menu,
  sidebar, page-area scroll bars, settings and info dialogs, tooltips,
  links, separators, focus/selection and the custom-painted controls.
- The system accent colour reaches selection, text selection, checked
  controls, the current-page highlight and the settings swatch outline.
- Both light and dark styles are installed; egui follows the OS preference
  without a restart.
- A theme or accent change at run time is read and installed without a
  restart.
- Tests use a fixed bundled preset, so the headless harness is deterministic
  and does not read the developer's desktop.

## Non-goals

- A theme picker, bundled-preset gallery or theme settings in
  `settings.toml`. Compile-time OS-following only.
- `native-theme-egui` or `native-theme-egui-widgets`; no native redraw of
  switches, sliders or combo boxes that egui does not have.
- System icons or SF Symbols; noido's painted title-bar icons stay.
- Full native widget fidelity or the connector's role-variant model
  (selected/hovered/disabled) beyond what noido's widgets use.
- Changing the document's paper colour, page rendering or PDF content.
- A `noido-theme` crate.

## Behaviour

### Startup and theme changes

- Read the system theme once before the window is shown. On failure, log once
  and use the platform preset (`adwaita` on Linux, the macOS/Windows presets
  from `native-theme`) resolved for the current egui mode; never leave the
  UI unstyled.
- Install a `Style` for both `egui::Theme::Light` and `Dark` with
  `Context::set_style_of`, and select the mode `native-theme` reports with
  `Context::set_theme`. egui's `ThemePreference::System` cannot supply it on
  Linux: winit reports no system theme there, so egui falls back to dark.
- Watch the OS theme with `native-theme`'s `watch` feature. The callback runs
  off the UI thread: clear the detection caches, re-read `SystemTheme`, send
  it to the UI thread and ask egui to repaint. The app installs the new theme
  on its next frame.
- The subscription is kept alive for the app's lifetime. The watcher thread
  must not hold a lock the UI thread blocks on when the subscription drops.
- Because the GNOME reader reads only the active variant, the inactive
  variant can briefly be the preset's after a switch. The next switch
  re-reads it. This is accepted; no attempt is made to copy the active
  variant's accent into the inactive one.

### What follows the theme

- **Title bar**: panel fill from the window's title-bar background; title
  text from the title-bar font colour; page/zoom status from the muted
  colour; icon buttons from the button role; window buttons use the theme's
  danger colour when the close button is hovered.
- **Menu**: popup fill, radius, border, shadow, padding and row height from
  the menu theme; entries use the menu font, hover fill/text and disabled
  text; separators use the separator colour.
- **Search field**: input fill, text, placeholder, caret, selection, focus
  border, padding and radius from the input theme.
- **Settings and info dialogs**: fill, border, radius, padding, title/body
  fonts, button order and gap from the dialog theme; the scrim stays a
  noido constant (the native model has no modal-scrim role).
- **Sidebar**: fill, border, selection and hover from the sidebar theme;
  thumbnail labels and result cards use its font and colours.
- **Page area**: scroll-bar track, thumb, hover and active colours, thumb
  width and overlay behaviour from the scrollbar theme. Pages themselves
  keep the paper tint.
- **Tooltips**: fill, text, border, radius, padding, width and delay from
  the tooltip theme.
- **Links**: `hyperlinked_color` from the theme; the existing hover/focus
  outline keeps its geometry and takes the theme colour.
- **Search hits**: the overlay is still an annotation over the document, not
  chrome. Derive the two fills from the theme's warning colour (at the
  existing alphas) instead of fixed amber, so a non-default warning colour
  carries through. Keep the conventional highlight character.
- **Focus and selection**: `selection.bg_fill`/`stroke` from the theme's
  selection colours, so checked boxes, radios, text selection and the
  sidebar share the accent.
- **Separators and window border**: separator and window-border colours and
  widths from the theme.

### Metrics

Adopt the theme's fonts, colours, strokes, corner radii, button padding and
minimum control heights. Keep noido's structural constants: title-bar height,
icon-button size, traffic-light inset, search-field compactness, sidebar
geometry and page layout. Where a native metric would break the compact title
bar (for example the 34 px input height against the 40 px bar), the structural
constant wins and the theme supplies everything else.

### Fonts

Load the theme's proportional and monospace faces through
`native_theme::fonts::system_face`, and install them as egui's `Proportional`
and `Monospace` families. If a face is absent, keep egui's own face and say so
once on stderr.

Where a bold face exists, register it as a named family and map
`TextStyle::Heading` to it, so dialog titles look native. egui's `FontId`
carries no weight, so `RichText::strong()` stays the native text colour rather
than becoming heavier; note this as a known limitation rather than replacing
every `.strong()` call site in this change.

## Mapping

The conversion is centralised in `theme`; the rest of the frontend asks for
semantic values rather than reading `ResolvedTheme` fields.

| Theme data | egui |
|---|---|
| `defaults.background_color` | `visuals.panel_fill` |
| `defaults.text_color`, `defaults.muted_color` | `visuals.text_color()`, `weak_text_color` |
| `defaults.selection_background`, `selection_text_color` | `visuals.selection.bg_fill`, `.stroke` |
| `defaults.link_color` | `visuals.hyperlink_color` |
| `defaults.warning_color`, `danger_color` | `warn_fg_color`, `error_fg_color` |
| `input.caret_color` | `visuals.text_cursor` |
| `input.background_color` | `extreme_bg_color`, `text_edit_bg_color` |
| `window.background_color`, `title_bar_background` | panel fills, title-bar frame |
| `window.border`, `popover.border` | `window_stroke`, window/menu corner radius |
| `defaults.shadow_color` | shadow colours, keeping egui's offsets/blur |
| `button.*` | `visuals.widgets.{inactive,hovered,active,open}`, button padding |
| `menu.*` | menu role: fill, font, hover, row height, padding, separators |
| `input.*` | input role: fill, text, caret, selection, focus, padding |
| `dialog.*` | dialog role: frame, padding, title/body fonts, button order/gap |
| `sidebar.*` | sidebar role: fill, border, selection, hover, font |
| `tooltip.*` | tooltip role: fill, text, border, padding, width, delay |
| `scrollbar.*` | scroll style: bar/floating width, thumb colours |
| `defaults.font`, `mono_font`, `text_scale` | text styles and font definitions |
| `layout.widget_gap`, `container_margin`, `window_margin` | item spacing, margins |

Fields with no egui equivalent (dialog entrance animation, per-widget role
variants, icon theme, active/inactive window distinction) are left out rather
than faked.

## Code shape

- `Cargo.toml` (workspace) and `gui-egui/Cargo.toml`: add
  `native-theme = { version = "0.7", default-features = false, features = ["native", "watch", "system-fonts"] }`.
  The lockfile pins the exact version.
- `gui-egui/src/theme/mod.rs`: the public surface: `Role`, `Theme`/data
  handle, `install`, `install_preset` (tests), `scope`, frame helpers,
  `current` accessor, and the live watcher.
- `gui-egui/src/theme/style.rs`: `Rgba` → `Color32`, strokes, radii, margins,
  global style, role styles.
- `gui-egui/src/theme/fonts.rs`: system faces and `FontDefinitions`.
- `gui-egui/src/theme/live.rs`: system read, fallback, watcher channel and
  installation on change.
- `gui-egui/src/main.rs`: read and install the theme before the window;
  attach the live watcher to the app.
- `gui-egui/src/app.rs`: poll for theme updates; retrieve the current theme
  data for custom frames; install a fixed preset in the headless harness.
- `titlebar.rs`, `settings.rs`, `info.rs`, `sidebar.rs`, `search.rs`,
  `frame.rs`: use theme frames, roles and colours; remove hardcoded UI
  colours.
- `theme` tests: presets in both modes, role scopes, font fallback, watcher
  polling. Harness tests: painted colours and fills for each surface.

## Testing

- Unit: Adwaita light and dark produce the expected `panel_fill`,
  `selection`, button visuals, menu/input/dialog/sidebar/tooltip values,
  radii, scrollbar widths and text sizes.
- Unit: a role scope changes only its subtree; the surrounding style is
  unchanged.
- Unit: missing fonts keep egui's faces and report once.
- Unit: the watcher's queued theme is installed exactly once; a failed read
  keeps the previous theme.
- Harness: with a fixed dark preset, the title text uses the theme's text
  colour and the status the muted colour; the menu entry hover uses the menu
  hover fill; the search field uses the input fill; the settings dialog uses
  the dialog fill/padding; the sidebar current page uses the accent; a link
  uses the link colour; the close button hover uses the danger colour; search
  hit fills derive from the warning colour.
- Harness: switching the installed preset from dark to light changes the
  painted colours without restarting the app.
- Manual: GNOME light/dark and accent changes; macOS and Windows builds are
  not checked by hand (as for the rest of the frontend).

## Known limitations and risks

- `native-theme` is young (0.7, single maintainer). It is pinned and isolated
  behind `theme`; dropping it later means replacing one module.
- The GNOME reader reads the active variant only; the inactive variant can
  briefly show the preset's accent/font after a switch, until it is read.
- egui's `FontId` has no weight; native bold is approximated by a named
  heading family where available. `.strong()` stays a colour change.
- Per-widget native themes that egui's `Style` cannot express are flattened
  into the nearest role; this is not a native-control toolkit.
- Reading the theme and loading system fonts adds startup work; it is done
  before the window and must stay well under a noticeable delay.
