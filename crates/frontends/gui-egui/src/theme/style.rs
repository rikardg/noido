//! The global egui style built from a resolved native theme.

use eframe::egui::{self, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};
use native_theme::ResolvedTheme;

use super::{Layout, convert};

/// Builds the style for one variant.
pub(super) fn global(theme: &ResolvedTheme, layout: &Layout, mode: egui::Theme) -> egui::Style {
    let mut style = egui::Style {
        visuals: match mode {
            egui::Theme::Light => egui::Visuals::light(),
            egui::Theme::Dark => egui::Visuals::dark(),
        },
        ..Default::default()
    };
    let defaults = &theme.defaults;
    let button = &theme.button;
    let border = &defaults.border;

    style.visuals.panel_fill = convert::color(defaults.background_color);
    style.visuals.window_fill = convert::color(theme.menu.background_color);
    style.visuals.extreme_bg_color = convert::color(theme.input.background_color);
    style.visuals.text_edit_bg_color = Some(convert::color(theme.input.background_color));
    style.visuals.text_cursor.stroke.color = convert::color(theme.input.caret_color);
    style.visuals.selection = egui::style::Selection {
        bg_fill: convert::color(defaults.selection_background),
        stroke: convert::stroke(1.0, defaults.selection_text_color),
    };
    style.visuals.hyperlink_color = convert::color(defaults.link_color);
    style.visuals.warn_fg_color = convert::color(defaults.warning_color);
    style.visuals.error_fg_color = convert::color(defaults.danger_color);
    style.visuals.weak_text_color = Some(convert::color(defaults.muted_color));
    style.visuals.window_stroke = convert::stroke(border.line_width, border.color);
    style.visuals.window_corner_radius = convert::corner_radius(border.corner_radius);
    style.visuals.menu_corner_radius = convert::corner_radius(theme.popover.border.corner_radius);
    style.visuals.window_shadow = shadow(
        style.visuals.window_shadow,
        border.shadow_enabled,
        defaults.shadow_color,
    );
    style.visuals.popup_shadow = shadow(
        style.visuals.popup_shadow,
        border.shadow_enabled,
        defaults.shadow_color,
    );

    let noninteractive = &mut style.visuals.widgets.noninteractive;
    noninteractive.bg_fill = convert::color(defaults.background_color);
    noninteractive.weak_bg_fill = convert::color(defaults.background_color);
    noninteractive.bg_stroke = convert::stroke(border.line_width, border.color);
    noninteractive.fg_stroke = Stroke::new(1.0, convert::color(defaults.text_color));
    noninteractive.corner_radius = convert::corner_radius(border.corner_radius);

    let inactive = &mut style.visuals.widgets.inactive;
    inactive.bg_fill = convert::color(button.background_color);
    inactive.weak_bg_fill = convert::color(button.background_color);
    inactive.bg_stroke = convert::stroke(button.border.line_width, button.border.color);
    inactive.fg_stroke = Stroke::new(1.0, convert::color(button.font.color));
    inactive.corner_radius = convert::corner_radius(button.border.corner_radius);

    let hovered = &mut style.visuals.widgets.hovered;
    hovered.bg_fill = convert::color(button.hover_background);
    hovered.weak_bg_fill = convert::color(button.hover_background);
    hovered.bg_stroke = convert::stroke(button.border.line_width, button.border.color);
    hovered.fg_stroke = Stroke::new(1.5, convert::color(button.hover_text_color));
    hovered.corner_radius = convert::corner_radius(button.border.corner_radius);

    let active = &mut style.visuals.widgets.active;
    active.bg_fill = convert::color(button.active_background.unwrap_or(button.background_color));
    active.weak_bg_fill = active.bg_fill;
    active.bg_stroke = convert::stroke(button.border.line_width, button.border.color);
    active.fg_stroke = Stroke::new(2.0, convert::color(button.active_text_color));
    active.corner_radius = convert::corner_radius(button.border.corner_radius);

    let open = &mut style.visuals.widgets.open;
    open.bg_fill = convert::color(button.checked_background.unwrap_or(button.background_color));
    open.weak_bg_fill = open.bg_fill;
    open.bg_stroke = convert::stroke(button.border.line_width, button.border.color);
    open.fg_stroke = Stroke::new(
        1.0,
        convert::color(button.checked_text_color.unwrap_or(button.font.color)),
    );
    open.corner_radius = convert::corner_radius(button.border.corner_radius);

    let gap = layout.widget_gap.unwrap_or(style.spacing.item_spacing.x);
    style.spacing.item_spacing = Vec2::splat(gap);
    style.spacing.button_padding = Vec2::new(
        button
            .border
            .padding
            .left
            .unwrap_or(style.spacing.button_padding.x),
        button
            .border
            .padding
            .top
            .unwrap_or(style.spacing.button_padding.y),
    );
    style.spacing.interact_size.y = button.min_height;
    style.spacing.menu_margin = convert::margin(
        theme.menu.border.padding.left,
        theme.menu.border.padding.right,
        theme.menu.border.padding.top,
        theme.menu.border.padding.bottom,
        style.spacing.menu_margin,
    );
    style.spacing.window_margin = Margin::same(
        layout
            .window_margin
            .unwrap_or(style.spacing.window_margin.left as f32)
            .round()
            .clamp(0.0, i8::MAX as f32) as i8,
    );
    style.spacing.tooltip_width = theme.tooltip.max_width;
    style.spacing.icon_width = theme.menu.icon_size;
    style.spacing.icon_spacing = theme.menu.icon_text_gap;

    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(defaults.font.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(button.font.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(theme.text_scale.caption.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(defaults.mono_font.size, FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(theme.text_scale.dialog_title.size, FontFamily::Proportional),
    );

    style
}

fn shadow(mut base: Shadow, enabled: bool, color: native_theme::Rgba) -> Shadow {
    if !enabled {
        return Shadow::NONE;
    }
    base.color = convert::color(color);
    base
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;
    use eframe::egui::{self, Color32};

    #[test]
    fn adwaita_dark_maps_the_global_style() {
        let theme = Theme::preset("adwaita").expect("adwaita");
        let style = theme.style(egui::Theme::Dark);
        assert_eq!(
            style.visuals.panel_fill,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
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
        assert_eq!(
            style.visuals.panel_fill,
            Color32::from_rgb(0xfa, 0xfa, 0xfb)
        );
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
