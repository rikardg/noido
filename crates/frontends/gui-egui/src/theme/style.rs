//! The global egui style built from a resolved native theme.

use eframe::egui::{self, Color32, FontFamily, FontId, Frame, Margin, Stroke, TextStyle, Vec2};
use native_theme::ResolvedTheme;

use super::{Layout, Role, Surface, convert, fonts::Fonts};

/// Builds the style for one variant.
pub(super) fn global(
    theme: &ResolvedTheme,
    layout: &Layout,
    fonts: &Fonts,
    mode: egui::Theme,
) -> egui::Style {
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
    style.visuals.window_shadow = convert::shadow(
        style.visuals.window_shadow,
        border.shadow_enabled,
        defaults.shadow_color,
    );
    style.visuals.popup_shadow = convert::shadow(
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
        FontId::new(theme.text_scale.dialog_title.size, fonts.heading_family()),
    );

    style
}

/// Applies one role's native widget definition to a scoped style.
pub(super) fn role(theme: &ResolvedTheme, style: &mut egui::Style, role: Role) {
    match role {
        Role::Button => button(theme, style),
        Role::Input => input(theme, style),
        Role::Menu => menu(theme, style),
        Role::Dialog => dialog(theme, style),
        Role::Sidebar => sidebar(theme, style),
        Role::Tooltip => tooltip(theme, style),
        Role::Separator => separator(theme, style),
        Role::Scrollbar => scrollbar(theme, style),
    }
}

fn button(theme: &ResolvedTheme, style: &mut egui::Style) {
    let button = &theme.button;
    let radius = convert::corner_radius(button.border.corner_radius);
    let stroke = convert::stroke(button.border.line_width, button.border.color);
    let text = convert::color(button.font.color);

    let inactive = &mut style.visuals.widgets.inactive;
    inactive.bg_fill = convert::color(button.background_color);
    inactive.weak_bg_fill = inactive.bg_fill;
    inactive.bg_stroke = stroke;
    inactive.fg_stroke = Stroke::new(1.0, text);
    inactive.corner_radius = radius;

    let hovered = &mut style.visuals.widgets.hovered;
    hovered.bg_fill = convert::color(button.hover_background);
    hovered.weak_bg_fill = hovered.bg_fill;
    hovered.bg_stroke = stroke;
    hovered.fg_stroke = Stroke::new(1.5, convert::color(button.hover_text_color));
    hovered.corner_radius = radius;

    let active = &mut style.visuals.widgets.active;
    active.bg_fill = convert::color(button.active_background.unwrap_or(button.background_color));
    active.weak_bg_fill = active.bg_fill;
    active.bg_stroke = stroke;
    active.fg_stroke = Stroke::new(2.0, convert::color(button.active_text_color));
    active.corner_radius = radius;

    let open = &mut style.visuals.widgets.open;
    open.bg_fill = convert::color(button.checked_background.unwrap_or(button.background_color));
    open.weak_bg_fill = open.bg_fill;
    open.bg_stroke = stroke;
    open.fg_stroke = Stroke::new(
        1.0,
        convert::color(button.checked_text_color.unwrap_or(button.font.color)),
    );
    open.corner_radius = radius;

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
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(button.font.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(button.font.size, FontFamily::Proportional),
    );
}

fn input(theme: &ResolvedTheme, style: &mut egui::Style) {
    let input = &theme.input;
    let radius = convert::corner_radius(input.border.corner_radius);
    let border = convert::stroke(input.border.line_width, input.border.color);
    let focus = convert::stroke(
        theme.defaults.focus_ring_width,
        input.focus_border_color.unwrap_or(input.border.color),
    );
    let text = convert::color(input.font.color);

    style.visuals.extreme_bg_color = convert::color(input.background_color);
    style.visuals.text_edit_bg_color = Some(convert::color(input.background_color));
    style.visuals.text_cursor.stroke.color = convert::color(input.caret_color);
    style.visuals.selection = egui::style::Selection {
        bg_fill: convert::color(input.selection_background),
        stroke: convert::stroke(1.0, input.selection_text_color),
    };
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);

    let inactive = &mut style.visuals.widgets.inactive;
    inactive.bg_fill = convert::color(input.background_color);
    inactive.weak_bg_fill = inactive.bg_fill;
    inactive.bg_stroke = border;
    inactive.fg_stroke = Stroke::new(1.0, text);
    inactive.corner_radius = radius;

    let hovered = &mut style.visuals.widgets.hovered;
    hovered.bg_fill = convert::color(input.background_color);
    hovered.weak_bg_fill = hovered.bg_fill;
    hovered.bg_stroke = convert::stroke(
        input.border.line_width,
        input.hover_border_color.unwrap_or(input.border.color),
    );
    hovered.fg_stroke = Stroke::new(1.5, text);
    hovered.corner_radius = radius;

    let active = &mut style.visuals.widgets.active;
    active.bg_fill = convert::color(input.background_color);
    active.weak_bg_fill = active.bg_fill;
    active.bg_stroke = focus;
    active.fg_stroke = Stroke::new(2.0, text);
    active.corner_radius = radius;

    style.spacing.button_padding = Vec2::new(
        input
            .border
            .padding
            .left
            .unwrap_or(style.spacing.button_padding.x),
        input
            .border
            .padding
            .top
            .unwrap_or(style.spacing.button_padding.y),
    );
    style.spacing.interact_size.y = input.min_height;
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(input.font.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(input.font.size, FontFamily::Proportional),
    );
}

fn menu(theme: &ResolvedTheme, style: &mut egui::Style) {
    let menu = &theme.menu;
    let radius = convert::corner_radius(menu.border.corner_radius);
    let text = convert::color(menu.font.color);

    style.visuals.window_fill = convert::color(menu.background_color);
    style.visuals.window_stroke = convert::stroke(menu.border.line_width, menu.border.color);
    style.visuals.menu_corner_radius = radius;
    style.visuals.window_shadow = convert::shadow(
        style.visuals.popup_shadow,
        menu.border.shadow_enabled,
        theme.defaults.shadow_color,
    );
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    style.visuals.widgets.noninteractive.bg_stroke = convert::stroke(1.0, menu.separator_color);

    let inactive = &mut style.visuals.widgets.inactive;
    inactive.weak_bg_fill = Color32::TRANSPARENT;
    inactive.bg_fill = Color32::TRANSPARENT;
    inactive.bg_stroke = Stroke::NONE;
    inactive.fg_stroke = Stroke::new(1.0, text);
    inactive.corner_radius = radius;

    let hovered = &mut style.visuals.widgets.hovered;
    hovered.weak_bg_fill = convert::color(menu.hover_background);
    hovered.bg_fill = hovered.weak_bg_fill;
    hovered.bg_stroke = Stroke::NONE;
    hovered.fg_stroke = Stroke::new(1.5, convert::color(menu.hover_text_color));
    hovered.corner_radius = radius;

    let active = &mut style.visuals.widgets.active;
    active.weak_bg_fill = hovered.weak_bg_fill;
    active.bg_fill = hovered.bg_fill;
    active.bg_stroke = Stroke::NONE;
    active.fg_stroke = hovered.fg_stroke;
    active.corner_radius = radius;

    style.spacing.menu_margin = convert::margin(
        menu.border.padding.left,
        menu.border.padding.right,
        menu.border.padding.top,
        menu.border.padding.bottom,
        style.spacing.menu_margin,
    );
    style.spacing.button_padding = Vec2::new(menu.border.padding.left.unwrap_or(12.0), 0.0);
    style.spacing.interact_size.y = menu.row_height.unwrap_or(32.0);
    style.spacing.icon_width = menu.icon_size;
    style.spacing.icon_spacing = menu.icon_text_gap;
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(menu.font.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(menu.font.size, FontFamily::Proportional),
    );
}

fn dialog(theme: &ResolvedTheme, style: &mut egui::Style) {
    button(theme, style);
    let dialog = &theme.dialog;

    style.visuals.window_fill = convert::color(dialog.background_color);
    style.visuals.window_stroke = convert::stroke(dialog.border.line_width, dialog.border.color);
    style.visuals.window_corner_radius = convert::corner_radius(dialog.border.corner_radius);
    style.visuals.window_shadow = convert::shadow(
        style.visuals.window_shadow,
        dialog.border.shadow_enabled,
        theme.defaults.shadow_color,
    );
    style.spacing.window_margin = convert::margin(
        dialog.border.padding.left,
        dialog.border.padding.right,
        dialog.border.padding.top,
        dialog.border.padding.bottom,
        style.spacing.window_margin,
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(dialog.body_font.size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(dialog.title_font.size, FontFamily::Proportional),
    );
}

fn sidebar(theme: &ResolvedTheme, style: &mut egui::Style) {
    let sidebar = &theme.sidebar;
    style.visuals.panel_fill = convert::color(sidebar.background_color);
    style.visuals.selection = egui::style::Selection {
        bg_fill: convert::color(sidebar.selection_background),
        stroke: convert::stroke(1.0, sidebar.selection_text_color),
    };
    style.visuals.widgets.hovered.weak_bg_fill = convert::color(sidebar.hover_background);
    style.visuals.widgets.hovered.bg_fill = style.visuals.widgets.hovered.weak_bg_fill;
    style.visuals.widgets.noninteractive.bg_stroke =
        convert::stroke(sidebar.border.line_width, sidebar.border.color);
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(sidebar.font.size, FontFamily::Proportional),
    );
}

fn tooltip(theme: &ResolvedTheme, style: &mut egui::Style) {
    let tooltip = &theme.tooltip;
    style.visuals.window_fill = convert::color(tooltip.background_color);
    style.visuals.window_stroke = convert::stroke(tooltip.border.line_width, tooltip.border.color);
    style.visuals.menu_corner_radius = convert::corner_radius(tooltip.border.corner_radius);
    style.visuals.window_shadow = convert::shadow(
        style.visuals.popup_shadow,
        tooltip.border.shadow_enabled,
        theme.defaults.shadow_color,
    );
    style.visuals.widgets.noninteractive.fg_stroke =
        Stroke::new(1.0, convert::color(tooltip.font.color));
    style.spacing.menu_margin = convert::margin(
        tooltip.border.padding.left,
        tooltip.border.padding.right,
        tooltip.border.padding.top,
        tooltip.border.padding.bottom,
        style.spacing.menu_margin,
    );
    style.spacing.tooltip_width = tooltip.max_width;
    style.interaction.tooltip_delay = f32::from(tooltip.show_delay.unwrap_or(500)) / 1000.0;
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(tooltip.font.size, FontFamily::Proportional),
    );
}

fn separator(theme: &ResolvedTheme, style: &mut egui::Style) {
    let separator = &theme.separator;
    style.visuals.widgets.noninteractive.bg_stroke =
        convert::stroke(separator.line_width, separator.line_color);
}

fn scrollbar(theme: &ResolvedTheme, style: &mut egui::Style) {
    let scrollbar = &theme.scrollbar;
    style.spacing.scroll.floating = scrollbar.overlay_mode;
    style.spacing.scroll.bar_width = scrollbar.groove_width;
    style.spacing.scroll.floating_width = scrollbar.thumb_width;
    style.spacing.scroll.handle_min_length = scrollbar.min_thumb_length;
    style.spacing.scroll.foreground_color = true;
    style.visuals.widgets.inactive.fg_stroke =
        Stroke::new(1.0, convert::color(scrollbar.thumb_color));
    style.visuals.widgets.hovered.fg_stroke =
        Stroke::new(1.0, convert::color(scrollbar.thumb_hover_color));
    style.visuals.widgets.active.fg_stroke = Stroke::new(
        1.0,
        convert::color(
            scrollbar
                .thumb_active_color
                .unwrap_or(scrollbar.thumb_hover_color),
        ),
    );
}

/// The frame for a surface with its own padding, radius or fill.
#[allow(dead_code)]
pub(super) fn frame(theme: &ResolvedTheme, surface: Surface, style: &egui::Style) -> Frame {
    match surface {
        Surface::TitleBar => Frame::NONE
            .fill(convert::color(theme.window.title_bar_background))
            .inner_margin(0),
        Surface::Dialog => Frame::NONE
            .fill(convert::color(theme.dialog.background_color))
            .stroke(convert::stroke(
                theme.dialog.border.line_width,
                theme.dialog.border.color,
            ))
            .corner_radius(convert::corner_radius(theme.dialog.border.corner_radius))
            .inner_margin(convert::margin(
                theme.dialog.border.padding.left,
                theme.dialog.border.padding.right,
                theme.dialog.border.padding.top,
                theme.dialog.border.padding.bottom,
                style.spacing.window_margin,
            ))
            .shadow(convert::shadow(
                style.visuals.window_shadow,
                theme.dialog.border.shadow_enabled,
                theme.defaults.shadow_color,
            )),
        Surface::Menu => Frame::NONE
            .fill(convert::color(theme.menu.background_color))
            .stroke(convert::stroke(
                theme.menu.border.line_width,
                theme.menu.border.color,
            ))
            .corner_radius(convert::corner_radius(theme.menu.border.corner_radius))
            .inner_margin(convert::margin(
                theme.menu.border.padding.left,
                theme.menu.border.padding.right,
                theme.menu.border.padding.top,
                theme.menu.border.padding.bottom,
                style.spacing.menu_margin,
            ))
            .shadow(convert::shadow(
                style.visuals.popup_shadow,
                theme.menu.border.shadow_enabled,
                theme.defaults.shadow_color,
            )),
        Surface::Tooltip => Frame::NONE
            .fill(convert::color(theme.tooltip.background_color))
            .stroke(convert::stroke(
                theme.tooltip.border.line_width,
                theme.tooltip.border.color,
            ))
            .corner_radius(convert::corner_radius(theme.tooltip.border.corner_radius))
            .inner_margin(convert::margin(
                theme.tooltip.border.padding.left,
                theme.tooltip.border.padding.right,
                theme.tooltip.border.padding.top,
                theme.tooltip.border.padding.bottom,
                style.spacing.menu_margin,
            ))
            .shadow(convert::shadow(
                style.visuals.popup_shadow,
                theme.tooltip.border.shadow_enabled,
                theme.defaults.shadow_color,
            )),
        Surface::Sidebar => Frame::side_top_panel(style)
            .fill(convert::color(theme.sidebar.background_color))
            .inner_margin(0),
        Surface::Central => Frame::central_panel(style),
    }
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

    use crate::theme::{Role, Surface};

    #[test]
    fn menu_role_uses_the_menu_theme() {
        let theme = Theme::preset("adwaita").expect("adwaita");
        let mut style = theme.style(egui::Theme::Dark);
        theme.apply_role(egui::Theme::Dark, &mut style, Role::Menu);
        assert_eq!(
            style.visuals.window_fill,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
        assert_eq!(
            style.visuals.widgets.inactive.weak_bg_fill,
            Color32::TRANSPARENT
        );
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
        assert_eq!(style.visuals.text_cursor.stroke.color, Color32::WHITE);
        assert_eq!(style.spacing.interact_size.y, 34.0);
    }

    #[test]
    fn sidebar_role_uses_the_sidebar_theme() {
        let theme = Theme::preset("adwaita").expect("adwaita");
        let mut style = theme.style(egui::Theme::Dark);
        theme.apply_role(egui::Theme::Dark, &mut style, Role::Sidebar);
        assert_eq!(
            style.visuals.panel_fill,
            Color32::from_rgb(0x2e, 0x2e, 0x32)
        );
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
