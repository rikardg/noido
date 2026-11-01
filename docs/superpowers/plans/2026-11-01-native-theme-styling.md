# Native Theme Styling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every noido UI element follow the OS theme — colours, accent, fonts, strokes, radii and spacing — using `native-theme` as the data source and our own egui mapping.

**Architecture:** A `gui-egui/src/theme` module reads `native_theme::SystemTheme`, converts both resolved variants into egui `Style`s installed with `set_style_of`, and exposes role scopes and frames for the surfaces egui cannot express globally (menu, input, dialog, sidebar, tooltip, scrollbar). A small live watcher re-reads and reinstalls the theme when the OS changes it. `native_theme` types stay inside `theme/`.

**Tech Stack:** Rust 2024, eframe/egui 0.37, `native-theme` 0.7 (data only; not its egui connector), `jj` for commits, nextest/cargo test.

**Spec:** `docs/superpowers/specs/2026-11-01-native-theme-styling-design.md`

## Global Constraints

- Dependency: `native-theme = { version = "0.7", default-features = false, features = ["native", "watch", "system-fonts"] }`. The lockfile pins the exact patch.
- `native_theme` types must not appear outside `crates/frontends/gui-egui/src/theme/`.
- The app installs both `egui::Theme::Light` and `Dark` with `Context::set_style_of` and selects the mode native-theme reports with `Context::set_theme`; the headless harness pins dark. (Revised after review: egui cannot read the OS mode on Linux.)
- Tests must not read the live desktop theme or system fonts; they use the bundled `adwaita` preset and egui's own font faces.
- Keep noido's structural constants where native metrics fight the compact UI: title-bar height, icon-button size, traffic-light inset, search-field compactness, sidebar and page layout.
- `just check` must be clean after every task; commit each task with `jj commit -m "egui: ..."`.
- A theme read failure must leave the app styled with the Adwaita preset and warn once on stderr.

## Review Focus

- A live GNOME theme/accent change while a modal or the menu is open: the open surface must repaint with the new colours, not keep the old ones.
- A missing native font family (for example `Adwaita Sans` on a machine without it): text must still lay out with egui's own faces, not tofu.
- Popup, tooltip and dialog roles leaking into the surrounding UI: scoping must restore the outer style, and the popup must inherit the role style from its parent at show time.
- Native text sizes and padding in the compact title bar: the search field, status and icon buttons must still fit and be clickable.
- Startup theme-read failure: the window must still be styled, with one warning, rather than panicking or drawing unstyled.

---

### Task 1: Theme data and the global egui style

**Files:**
- Modify: `Cargo.toml` (workspace dependencies)
- Modify: `crates/frontends/gui-egui/Cargo.toml`
- Modify: `crates/frontends/gui-egui/src/main.rs` (`mod theme;`, install before the window)
- Create: `crates/frontends/gui-egui/src/theme/mod.rs`
- Create: `crates/frontends/gui-egui/src/theme/convert.rs`
- Create: `crates/frontends/gui-egui/src/theme/style.rs`

**Interfaces:**
- Produces: `theme::Theme::{preset, from_system, install, current, style, palette}`, `theme::Palette`, `Color32`/`Stroke`/`CornerRadius`/`Margin` converters.
- Produces: `Palette { panel, text, muted, danger, danger_text, warning, link, selection, selection_text, border, title_bar, title_text, menu_fill, menu_hover, menu_text, input, tooltip, sidebar, sidebar_selection, dialog_fill, separator }`.

- [ ] **Step 1: Write the failing global-style tests**

In `theme/style.rs`:

```rust
#[cfg(test)]
mod tests {
    use eframe::egui::{self, Color32};
    use crate::theme::Theme;

    #[test]
    fn adwaita_dark_maps_the_global_style() {
        let theme = Theme::preset("adwaita").expect("adwaita");
        let style = theme.style(egui::Theme::Dark);
        assert_eq!(style.visuals.panel_fill, Color32::from_rgb(0x22, 0x22, 0x26));
        assert_eq!(
            style.visuals.widgets.noninteractive.fg_stroke.color,
            Color32::WHITE
        );
        assert_eq!(
            style.visuals.selection.bg_fill,
            Color32::from_rgb(0x35, 0x84, 0xe4)
        );
        assert_eq!(
            style.visuals.widgets.inactive.weak_bg_fill,
            Color32::from_rgb(0x38, 0x38, 0x3c)
        );
    }

    #[test]
    fn adwaita_light_maps_the_global_style() {
        let theme = Theme::preset("adwaita").expect("adwaita");
        let style = theme.style(egui::Theme::Light);
        assert_eq!(style.visuals.panel_fill, Color32::from_rgb(0xfa, 0xfa, 0xfb));
        assert_eq!(
            style.visuals.widgets.noninteractive.fg_stroke.color,
            Color32::from_rgb(0x2e, 0x34, 0x36)
        );
        assert_eq!(
            style.visuals.hyperlink_color,
            Color32::from_rgb(0x1b, 0x6a, 0xcb)
        );
        assert_eq!(
            style.visuals.widgets.inactive.weak_bg_fill,
            Color32::from_rgb(0xe6, 0xe6, 0xe7)
        );
    }
}
```

In `theme/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use eframe::egui::{self, Color32, ThemePreference};
    use super::Theme;

    #[test]
    fn install_sets_both_styles_and_the_current_theme() {
        let ctx = egui::Context::default();
        ctx.set_theme(ThemePreference::Dark);
        let theme = Theme::preset("adwaita").expect("adwaita");
        theme.install(&ctx);
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals.panel_fill,
            Color32::from_rgb(0xfa, 0xfa, 0xfb)
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals.panel_fill,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
        assert_eq!(
            Theme::current(&ctx).palette(egui::Theme::Dark).panel,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }

    #[test]
    fn a_failed_system_read_falls_back_to_adwaita() {
        let theme = Theme::from_system_result(Err("no portal".into()));
        assert_eq!(
            theme.palette(egui::Theme::Dark).panel,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib theme::`
Expected: FAIL — the `theme` module does not exist.

- [ ] **Step 3: Add the dependency and the theme skeleton**

Add to workspace `Cargo.toml`:

```toml
native-theme = { version = "0.7", default-features = false, features = ["native", "watch", "system-fonts"] }
```

and to `gui-egui/Cargo.toml`:

```toml
native-theme.workspace = true
```

Add `mod theme;` to `main.rs`. In `run()`, after the context is created:

```rust
let theme = theme::Theme::from_system();
theme.install(&ctx);
```

Implement `theme/mod.rs`:

- `Theme { light: ResolvedTheme, dark: ResolvedTheme, active: egui::Theme }`.
- `Theme::preset(name) -> Option<Self>`: resolve the bundled preset in both modes; `active` is dark.
- `Theme::from_system() -> Self`: `Self::from_system_result(SystemTheme::from_system().map_err(|e| e.to_string()))`.
- `from_system_result`: on `Ok`, use `sys.light`/`sys.dark` and map `sys.mode` to `egui::Theme`; on `Err`, warn once and use `Theme::preset("adwaita")`.
- `Theme::install`: `ctx.set_style_of(Light, self.style(Light))`, the same for dark, then store `Arc<Self>` in `ctx.data_mut` under a fixed `egui::Id`.
- `Theme::current(ctx) -> Arc<Self>`: read the stored theme; `expect("the noido theme is installed before the UI runs")`.
- `Theme::palette(mode) -> Palette` from the resolved fields.
- `convert.rs`: `color(Rgba) -> Color32`, `stroke`, `corner_radius(f32)`, `margin(ResolvedPadding)`, `text_style`.
- `style.rs`: build `Style` starting from `Visuals::light()`/`dark()` (to keep egui's text transfer function), then map the fields from the spec's table: `panel_fill`, `window_fill`, `extreme_bg_color`, `text_edit_bg_color`, `text_cursor`, `selection`, `hyperlink_color`, `warn_fg_color`, `error_fg_color`, `window_stroke`, `window_corner_radius`, `menu_corner_radius`, shadows, widget visuals from the button theme, `spacing.item_spacing`, `button_padding`, `interact_size.y`, `menu_margin`, `window_margin`, `tooltip_width`, and `text_styles` (`Body`, `Button`, `Small`, `Monospace`, `Heading`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib theme::`
Expected: PASS.

- [ ] **Step 5: Run the project check**

Run: `just check`
Expected: fmt, clippy and tests clean.

- [ ] **Step 6: Commit**

```bash
jj commit -m "egui: map the system theme into egui's global style"
```

---

### Task 2: Role scopes and surface frames

**Files:**
- Modify: `crates/frontends/gui-egui/src/theme/mod.rs`
- Modify: `crates/frontends/gui-egui/src/theme/style.rs`
- Modify: `crates/frontends/gui-egui/src/theme/convert.rs`

**Interfaces:**
- Produces: `theme::Role { Button, Input, Menu, Dialog, Sidebar, Tooltip, Separator, Scrollbar }`.
- Produces: `theme::Surface { TitleBar, Dialog, Menu, Tooltip, Sidebar, Central }`.
- Produces: `Theme::apply_role(mode, &mut Style, Role)`, `Theme::frame(mode, Surface, &Style) -> Frame`.
- Produces: `theme::scope(ui, Role, |ui| ...)`, `theme::hover_text(response, text)`, `theme::hover_text_at_pointer(response, text)`.

- [ ] **Step 1: Write the failing role and frame tests**

In `theme/style.rs`:

```rust
use crate::theme::{Role, Surface};

#[test]
fn menu_role_uses_the_menu_theme() {
    let theme = Theme::preset("adwaita").expect("adwaita");
    let mut style = theme.style(egui::Theme::Dark);
    theme.apply_role(egui::Theme::Dark, &mut style, Role::Menu);
    assert_eq!(style.visuals.window_fill, Color32::from_rgb(0x22, 0x22, 0x26));
    assert_eq!(style.visuals.widgets.inactive.weak_bg_fill, Color32::TRANSPARENT);
    assert_eq!(
        style.visuals.widgets.hovered.weak_bg_fill,
        Color32::from_rgb(0x3a, 0x3a, 0x3e)
    );
    assert_eq!(
        style.visuals.widgets.inactive.fg_stroke.color,
        Color32::WHITE
    );
}

#[test]
fn input_role_uses_the_input_theme() {
    let theme = Theme::preset("adwaita").expect("adwaita");
    let mut style = theme.style(egui::Theme::Dark);
    theme.apply_role(egui::Theme::Dark, &mut style, Role::Input);
    assert_eq!(
        style.visuals.extreme_bg_color,
        Color32::from_rgb(0x38, 0x38, 0x3c)
    );
    assert_eq!(style.visuals.text_cursor, Color32::WHITE);
    assert_eq!(style.spacing.interact_size.y, 34.0);
}

#[test]
fn sidebar_role_uses_the_sidebar_theme() {
    let theme = Theme::preset("adwaita").expect("adwaita");
    let mut style = theme.style(egui::Theme::Dark);
    theme.apply_role(egui::Theme::Dark, &mut style, Role::Sidebar);
    assert_eq!(style.visuals.panel_fill, Color32::from_rgb(0x2e, 0x2e, 0x32));
    assert_eq!(
        style.visuals.widgets.hovered.weak_bg_fill,
        Color32::from_rgb(0x36, 0x36, 0x3a)
    );
}

#[test]
fn tooltip_role_uses_the_tooltip_theme() {
    let theme = Theme::preset("adwaita").expect("adwaita");
    let mut style = theme.style(egui::Theme::Dark);
    theme.apply_role(egui::Theme::Dark, &mut style, Role::Tooltip);
    assert_eq!(
        style.visuals.window_fill,
        Color32::from_rgb(0x3a, 0x3a, 0x3e)
    );
    assert_eq!(style.spacing.tooltip_width, 360.0);
}

#[test]
fn dialog_and_menu_frames_use_the_surface_theme() {
    let theme = Theme::preset("adwaita").expect("adwaita");
    let style = theme.style(egui::Theme::Dark);
    let dialog = theme.frame(egui::Theme::Dark, Surface::Dialog, &style);
    assert_eq!(dialog.fill, Color32::from_rgb(0x22, 0x22, 0x26));
    assert_eq!(dialog.corner_radius, egui::CornerRadius::same(18));
    let menu = theme.frame(egui::Theme::Dark, Surface::Menu, &style);
    assert_eq!(menu.fill, Color32::from_rgb(0x22, 0x22, 0x26));
}
```

In `theme/mod.rs`:

```rust
#[test]
fn a_role_scope_restores_the_outer_style() {
    let ctx = egui::Context::default();
    let theme = Theme::preset("adwaita").expect("adwaita");
    theme.install(&ctx);
    let mut inside = None;
    let mut outside = None;
    let _ = ctx.run_ui(Default::default(), |ui| {
        outside = Some(ui.visuals().panel_fill);
        scope(ui, Role::Sidebar, |ui| inside = Some(ui.visuals().panel_fill));
    });
    assert_ne!(inside, outside);
    assert_eq!(outside, Some(Color32::from_rgb(0x22, 0x22, 0x26)));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib theme::`
Expected: FAIL — `Role`, `Surface`, `apply_role`, `frame` and `scope` are missing.

- [ ] **Step 3: Implement roles, frames and scopes**

In `theme/style.rs`, add `apply_role`:

- `Button`: button theme fills, hover/active fills and text, padding.
- `Input`: input fill, placeholder, caret, selection, border/focus stroke, min height, font.
- `Menu`: transparent inactive row, menu hover fill/text, menu font, separators, `spacing.menu_margin`, `interact_size.y = menu.row_height`.
- `Dialog`: dialog fill/border/radius, `spacing.window_margin` from dialog padding, Body/Heading font sizes.
- `Sidebar`: sidebar fill, selection, hover, border, font.
- `Tooltip`: tooltip fill/border/radius, `spacing.menu_margin`, `spacing.tooltip_width`, `interaction.tooltip_delay`, tooltip font.
- `Separator`: `widgets.noninteractive.bg_stroke` from separator.
- `Scrollbar`: `spacing.scroll.bar_width`/`floating_width`, thumb colours in `widgets.*.fg_stroke`, `foreground_color = true`.

In `theme/mod.rs`, add `frame`:

- `TitleBar`: fill `window.title_bar_background`, no border, zero margin.
- `Dialog`: fill `dialog.background_color`, stroke/radius/margin/shadow from `dialog.border`.
- `Menu`: fill `menu.background_color`, menu radius/margin/shadow.
- `Tooltip`: fill `tooltip.background_color`, tooltip border/radius/margin/shadow.
- `Sidebar`: fill `sidebar.background_color`, `Frame::side_top_panel` geometry, zero margin.
- `Central`: `Frame::central_panel(style)`.

Add `scope` (clone the current style, apply the role, set it on a child scope) and `hover_text`/`hover_text_at_pointer` using `egui::Tooltip` with `.popup.style(...)` applying `Role::Tooltip`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib theme::`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: scope native styles to menu, input, dialog and other roles"
```

---

### Task 3: Native fonts

**Files:**
- Create: `crates/frontends/gui-egui/src/theme/fonts.rs`
- Modify: `crates/frontends/gui-egui/src/theme/mod.rs`
- Modify: `crates/frontends/gui-egui/src/theme/style.rs`

**Interfaces:**
- Produces: `theme::fonts::Fonts` (`Default` = egui's faces), `Fonts::load(&ResolvedTheme) -> Fonts`, `Fonts::heading_family() -> egui::FontFamily`, `fonts::definitions(&Fonts) -> Option<FontDefinitions>`, `fonts::install(&Context, &Fonts)`.
- `Theme` gains `fonts: Fonts`; `Theme::preset` uses `Fonts::default()`, `Theme::from_native` uses `Fonts::load(active_variant)`.

- [ ] **Step 1: Write the failing font tests**

In `theme/fonts.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_faces_egui_keeps_its_fonts() {
        let fonts = Fonts::default();
        assert!(definitions(&fonts).is_none());
        assert_eq!(fonts.heading_family(), egui::FontFamily::Proportional);
    }

    #[test]
    fn the_preset_uses_proportional_for_headings() {
        let theme = crate::theme::Theme::preset("adwaita").expect("adwaita");
        let style = theme.style(egui::Theme::Dark);
        assert_eq!(
            style.text_styles[&egui::TextStyle::Heading].family,
            egui::FontFamily::Proportional
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib theme::fonts::`
Expected: FAIL — the `fonts` module does not exist.

- [ ] **Step 3: Implement font loading and installation**

In `fonts.rs`:

- `Fonts { ui: Option<SystemFace>, mono: Option<SystemFace>, heading: Option<SystemFace> }`.
- `Fonts::load(variant)`: find `variant.defaults.font`, `mono_font`, and a heading face at `variant.text_scale.dialog_title.weight`; on a missing family try `native_theme::fonts::substitute_family_cached`.
- `Fonts::heading_family()`: `FontFamily::Name("noido-heading".into())` when a heading face exists, otherwise `Proportional`.
- `definitions(&Fonts)`: return `None` when every face is `None`; otherwise start from `FontDefinitions::default()`, insert the faces as `FontData::from_blob(data, index)` at the front of `Proportional` and `Monospace`, and register `noido-heading` as the heading face followed by the proportional chain.
- `install(ctx, fonts)`: `ctx.set_fonts(defs)` only when `definitions` returns `Some`.

In `theme/mod.rs`, store `Fonts` on `Theme`, load it in `from_native`, and call `fonts::install` first in `install`.

In `style.rs`, use `fonts.heading_family()` for `TextStyle::Heading` and the theme's dialog-title size.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib theme::`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: use the system fonts for the interface and headings"
```

---

### Task 4: Harness baseline

**Files:**
- Modify: `crates/frontends/gui-egui/src/app.rs` (harness and existing tests)

**Interfaces:**
- `Harness::build_with` installs the fixed `adwaita` preset and pins `ThemePreference::Dark` before the first frame.
- Produces: `Harness::theme() -> Arc<theme::Theme>` and `Harness::text_color(&str) -> Option<Color32>`.

- [ ] **Step 1: Write the failing harness-baseline tests**

```rust
#[test]
fn the_harness_uses_the_fixed_theme() {
    let h = Harness::new();
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        Color32::from_rgb(0x22, 0x22, 0x26)
    );
    assert_eq!(
        h.theme().palette(egui::Theme::Dark).selection,
        h.ctx.global_style().visuals.selection.bg_fill
    );
}

#[test]
fn changing_the_theme_changes_the_global_style() {
    let mut h = Harness::new();
    h.ctx.set_theme(egui::ThemePreference::Light);
    h.frames(1, vec![]);
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        Color32::from_rgb(0xfa, 0xfa, 0xfb)
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib the_harness_uses_the_fixed_theme changing_the_theme_changes_the_global_style`
Expected: FAIL — the harness still uses egui defaults.

- [ ] **Step 3: Install the fixed theme and add the helpers**

- In `Harness::build_with`, after creating the context:
  ```rust
  crate::theme::install_preset(&ctx, "adwaita");
  ctx.set_theme(egui::ThemePreference::Dark);
  ```
- Add `Harness::theme()` returning `crate::theme::Theme::current(&self.ctx)`.
- Add `Harness::text_color(&str)`: find the `Shape::Text` whose galley text matches, and return `override_text_color.unwrap_or(fallback_color)`, recursing into `Shape::Vec`.
- Run the whole suite and update tests whose expected positions or colours were tied to egui's default text sizes/palette. Do not change the mapping to make old assertions pass; update the assertion to the new intended value.

- [ ] **Step 4: Run the whole suite to verify it passes**

Run: `cargo test -p gui-egui`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: run the harness on a fixed native theme"
```

---

### Task 5: Title bar and search field

**Files:**
- Modify: `crates/frontends/gui-egui/src/titlebar.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`
- Test: `crates/frontends/gui-egui/src/app.rs` (harness tests)

**Interfaces:**
- Consumes: `theme::Theme::current`, `Palette`, `Role::Input`, `Surface::TitleBar`.
- Produces (test helpers): `Harness::theme() -> Arc<Theme>`, `Harness::text_color(&str) -> Option<Color32>`.

- [ ] **Step 1: Write the failing title-bar tests**

In `app.rs` tests:

```rust
#[test]
fn title_bar_uses_theme_colours() {
    let mut h = Harness::new();
    h.app.name = "test.pdf".into();
    h.frames(1, vec![]);
    let palette = h.theme().palette(egui::Theme::Dark);
    let status = titlebar::status_text(
        h.app.current_page().unwrap_or(0),
        h.app.view.layout().page_count(),
        h.app.view.scale(),
    );
    assert_eq!(h.text_color("test.pdf"), Some(palette.title_text));
    assert_eq!(h.text_color(&status), Some(palette.muted));
    assert!(!filled_with(&h, palette.title_bar).is_empty());
}

#[test]
fn search_field_uses_the_input_fill() {
    let h = Harness::new();
    let palette = h.theme().palette(egui::Theme::Dark);
    assert!(!filled_with(&h, palette.input).is_empty());
}

#[test]
fn close_hover_uses_the_danger_colour() {
    let mut h = Harness::new();
    let palette = h.theme().palette(egui::Theme::Dark);
    h.frames(1, vec![Event::PointerMoved(h.app.bar.close.center())]);
    assert!(!filled_with(&h, palette.danger).is_empty());
}
```

`text_color`, `theme()` and `filled_with` already exist from Task 4.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib title_bar_uses_theme_colours search_field_uses_the_input_fill close_hover_uses_the_danger_colour`
Expected: FAIL — the colours are still egui defaults.

- [ ] **Step 3: Use the theme in the title bar**

- `app.rs`: give the top panel `theme::frame(ctx.theme(), Surface::TitleBar, ui.style())`.
- `titlebar::paint_title`: draw the name with `palette.title_text` and the page/zoom with `palette.muted`.
- `titlebar::window_buttons`: use `palette.danger` and `palette.danger_text` for the close hover.
- `titlebar::show`: wrap the search `TextEdit` in `theme::scope(ui, Role::Input, ...)`, keeping its `SEARCH_HEIGHT`.
- Replace the title-bar `on_hover_text` calls with `theme::hover_text`, so tooltips use `Role::Tooltip`.
- `titlebar::menu_entries`: keep the checkmark using `ui.visuals().text_color()`; Role::Menu will make that the menu text colour in Task 6.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib title_bar_uses_theme_colours search_field_uses_the_input_fill close_hover_uses_the_danger_colour`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: draw the title bar and search field from the system theme"
```

---

### Task 6: Menu

**Files:**
- Modify: `crates/frontends/gui-egui/src/titlebar.rs`
- Test: `crates/frontends/gui-egui/src/app.rs` (harness tests)

**Interfaces:**
- Consumes: `Role::Menu`, `Surface::Menu`, `Palette::{menu_fill, menu_hover, menu_text}`.

- [ ] **Step 1: Write the failing menu tests**

```rust
#[test]
fn menu_uses_the_menu_fill_and_hover() {
    let mut h = Harness::new();
    let palette = h.theme().palette(egui::Theme::Dark);
    h.click(h.app.bar.menu.center());
    h.frames(1, vec![]);
    assert!(!filled_with(&h, palette.menu_fill).is_empty());
    let entry = h.text_rect("Settings…").expect("settings entry");
    h.frames(1, vec![Event::PointerMoved(entry.center())]);
    assert!(!filled_with(&h, palette.menu_hover).is_empty());
}

#[test]
fn menu_labels_use_the_menu_text_colour() {
    let mut h = Harness::new();
    let palette = h.theme().palette(egui::Theme::Dark);
    h.click(h.app.bar.menu.center());
    h.frames(1, vec![]);
    assert_eq!(h.text_color("Settings…"), Some(palette.menu_text));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib menu_uses_the_menu_fill_and_hover menu_labels_use_the_menu_text_colour`
Expected: FAIL — the popup still uses the global style.

- [ ] **Step 3: Apply the menu role to the popup**

In `titlebar::show`, replace

```rust
egui::Popup::menu(&menu_button).show(|ui| ...)
```

with a popup whose style applies both egui's `menu_style` and our role:

```rust
let theme = theme::Theme::current(ui.ctx());
let mode = ui.ctx().theme();
egui::Popup::menu(&menu_button)
    .style(move |style| {
        egui::menu::menu_style(style);
        theme.apply_role(mode, style, theme::Role::Menu);
    })
    .show(|ui| menu_entries(ui, bar, &mut output.actions));
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib menu_`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: style the menu from the system menu theme"
```

---

### Task 7: Settings and info dialogs

**Files:**
- Modify: `crates/frontends/gui-egui/src/settings.rs`
- Modify: `crates/frontends/gui-egui/src/info.rs`
- Test: `crates/frontends/gui-egui/src/app.rs` (harness tests)

**Interfaces:**
- Consumes: `Surface::Dialog`, `Role::Dialog`, `Palette`/theme frame.

- [ ] **Step 1: Write the failing dialog tests**

```rust
#[test]
fn settings_uses_the_dialog_fill() {
    let mut h = Harness::new();
    let palette = h.theme().palette(egui::Theme::Dark);
    let ctx = h.ctx.clone();
    h.app.apply(&ctx, None, Action::OpenSettings);
    h.frames(1, vec![]);
    assert!(!filled_with(&h, palette.dialog_fill).is_empty());
}

#[test]
fn info_uses_the_dialog_fill() {
    let mut h = Harness::new();
    show_info(&mut h);
    let palette = h.theme().palette(egui::Theme::Dark);
    assert!(!filled_with(&h, palette.dialog_fill).is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib settings_uses_the_dialog_fill info_uses_the_dialog_fill`
Expected: FAIL — the modals still use `Frame::popup`.

- [ ] **Step 3: Apply the dialog frame and role**

- `settings.rs`: `egui::Modal::new(id).backdrop_color(scrim).frame(theme::frame(...))`; wrap the modal body in `theme::scope(ui, Role::Dialog, ...)`.
- `info.rs`: replace `Frame::popup(&ctx.global_style())` with the dialog surface frame; preserve the existing right-margin-to-zero trick by reading the frame's right inner margin first; wrap the contents in `Role::Dialog`.
- Replace the settings swatch and info `on_hover_text` calls with `theme::hover_text`.
- Keep the scrim constant.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib settings_uses_the_dialog_fill info_uses_the_dialog_fill`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: draw the settings and info dialogs from the dialog theme"
```

---

### Task 8: Sidebar, scroll bars, links and search hits

**Files:**
- Modify: `crates/frontends/gui-egui/src/sidebar.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`
- Modify: `crates/frontends/gui-egui/src/search.rs`
- Test: `crates/frontends/gui-egui/src/app.rs` (harness tests)

**Interfaces:**
- Consumes: `Surface::Sidebar`, `Role::Sidebar`, `Role::Scrollbar`, `Palette::{sidebar, sidebar_selection, warning}`.
- Produces: `search::highlight_fill(warning: Color32) -> Color32`, `search::current_fill(warning: Color32) -> Color32`.

- [ ] **Step 1: Write the failing surface tests**

```rust
#[test]
fn sidebar_uses_the_sidebar_fill_and_selection() {
    let mut h = Harness::new();
    h.app.sidebar.open = true;
    h.frames(2, vec![]);
    let palette = h.theme().palette(egui::Theme::Dark);
    assert!(!filled_with(&h, palette.sidebar).is_empty());
    assert!(!filled_with(&h, palette.sidebar_selection).is_empty());
}

#[test]
fn search_hits_use_the_warning_colour() {
    let warning = Color32::from_rgb(0xe5, 0xa5, 0x0a);
    assert_eq!(
        search::highlight_fill(warning),
        Color32::from_rgba_unmultiplied(0xe5, 0xa5, 0x0a, 90)
    );
    assert_eq!(
        search::current_fill(warning),
        Color32::from_rgba_unmultiplied(0xe5, 0xa5, 0x0a, 140)
    );
}
```

Keep the existing link-outline test and add an assertion that the outline colour equals `h.theme().palette(egui::Theme::Dark).link`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib sidebar_uses_the_sidebar_fill_and_selection search_hits_use_the_warning_colour`
Expected: FAIL — the sidebar still uses the global panel fill and the hit fills are constants.

- [ ] **Step 3: Apply the sidebar and scrollbar roles**

- `sidebar::show`: use `theme::frame(mode, Surface::Sidebar, ui.style())`; wrap the panel body in `theme::scope(ui, Role::Sidebar, ...)`.
- `sidebar::scrolled`: wrap the `ScrollArea::show_viewport` call in `theme::scope(ui, Role::Scrollbar, ...)`.
- `app.rs`: wrap the document `scroll_area.show_viewport(...)` in `Role::Scrollbar`.
- `search.rs`: replace `HIGHLIGHT_FILL`/`CURRENT_FILL` with the two functions, using `from_rgba_unmultiplied` at alphas 90 and 140.
- `app.rs`: draw hits with `search::highlight_fill(ui.visuals().warn_fg_color)` and the current hit with `search::current_fill(...)`.
- Replace the link's `on_hover_text_at_pointer` with `theme::hover_text_at_pointer`.
- Update the existing hit tests to call the functions instead of the constants.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib sidebar_ search_hits_ highlight`
Expected: PASS.

- [ ] **Step 5: Run the project check and commit**

Run: `just check`
Expected: clean.

```bash
jj commit -m "egui: theme the sidebar, scroll bars, links and search hits"
```

---

### Task 9: Follow the system theme while running

**Files:**
- Create: `crates/frontends/gui-egui/src/theme/live.rs`
- Modify: `crates/frontends/gui-egui/src/theme/mod.rs`
- Modify: `crates/frontends/gui-egui/src/main.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`

**Interfaces:**
- Produces: `theme::Live::start(ctx: &egui::Context) -> Live`, `Live::poll(&mut self, ctx: &egui::Context) -> bool`.
- `NoidoApp` gains `live: Option<theme::Live>` and `with_theme(live) -> Self`.

- [ ] **Step 1: Write the failing live-theme test**

In `theme/live.rs`:

```rust
#[test]
fn poll_installs_the_queued_theme_once() {
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Theme::preset("adwaita").expect("adwaita")).unwrap();
    let mut live = Live { rx, _subscription: None };
    assert!(live.poll(&ctx));
    assert!(!live.poll(&ctx));
    assert_eq!(
        Theme::current(&ctx).palette(egui::Theme::Dark).panel,
        egui::Color32::from_rgb(0x22, 0x22, 0x26)
    );
}
```

Add a harness test that changes the installed theme without restarting:

```rust
#[test]
fn changing_the_theme_repaints_without_restarting() {
    let mut h = Harness::new();
    let dark = h.theme().palette(egui::Theme::Dark).title_text;
    h.ctx.set_theme(egui::ThemePreference::Light);
    h.frames(1, vec![]);
    let light = h.theme().palette(egui::Theme::Light).title_text;
    assert_ne!(dark, light);
    h.app.name = "test.pdf".into();
    h.frames(1, vec![]);
    assert_eq!(h.text_color("test.pdf"), Some(light));
}

#[test]
fn a_theme_change_reaches_an_open_dialog() {
    let mut h = Harness::new();
    let ctx = h.ctx.clone();
    h.app.apply(&ctx, None, Action::OpenSettings);
    h.frames(1, vec![]);
    h.ctx.set_theme(egui::ThemePreference::Light);
    h.frames(1, vec![]);
    let light = h.theme().palette(egui::Theme::Light).dialog_fill;
    assert!(!filled_with(&h, light).is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gui-egui --lib poll_installs_the_queued_theme_once changing_the_theme_repaints_without_restarting a_theme_change_reaches_an_open_dialog`
Expected: FAIL — `Live` does not exist.

- [ ] **Step 3: Implement the watcher and app integration**

- `Live { rx: Receiver<Theme>, _subscription: Option<ThemeSubscription> }`.
- `Live::start(ctx)`: `on_theme_change` sends a signal; in the callback call `detect::invalidate_caches()`, `Theme::from_system()`, send the theme, then `ctx.request_repaint()`. Keep the subscription.
- `Live::poll`: drain `try_recv`, install the last theme, return whether one was installed.
- In `main`, after creating the context: `let theme = Theme::from_system(); theme.install(&ctx); let live = theme::Live::start(&ctx);` and attach `live` to `NoidoApp`.
- In `NoidoApp::show`, poll the live theme before drawing anything.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gui-egui --lib theme:: changing_the_theme_repaints_without_restarting a_theme_change_reaches_an_open_dialog`
Expected: PASS.

- [ ] **Step 5: Remove leftovers, run the full check and build**

- Remove `CLOSE_HOVER` from `titlebar.rs` if it is still there.
- Confirm no hardcoded UI colours remain outside `theme/` except the paper tint and the modal scrim:
  `rg -n "Color32::|from_rgb|from_gray|from_black" crates/frontends/gui-egui/src`
- Run: `just check`
  Expected: fmt, clippy and all tests clean.
- Run: `just build`
  Expected: release binary builds.

- [ ] **Step 6: Commit**

```bash
jj commit -m "egui: follow the system theme while the app is running"
```

---

## Manual verification

After the plan is executed, run the release binary with a scratch state dir on GNOME:

- Switch the system between light and dark; the window, title bar, menus, dialogs and selection change without a restart.
- Change the GNOME accent colour; selection, checked controls, the sidebar highlight and the settings swatch outline follow it.
- Open the menu, settings and document info in both modes; check the menu hover, dialog fill/padding and button appearance.
- Open a document with search hits; check the hit overlay still reads as a highlight and follows the warning colour.
- Check stderr for font or theme warnings; there should be none on this machine.
