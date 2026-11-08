//! The system theme, mapped into egui.
//!
//! `native-theme` reads the OS theme; this module converts both resolved
//! variants into egui styles, stores the current one on the context, and
//! offers scoped roles for widgets egui's single global style cannot express.

mod convert;
mod fonts;
mod live;
mod style;

use std::sync::Arc;

use eframe::egui::{self, Color32};
use native_theme::ResolvedTheme;

pub use live::Live;

const THEME_ID: &str = "noido-theme";

/// How far the desk sits below a light theme's background, in OKLab
/// lightness.
const DESK_STEP: f32 = 0.12;

/// The OKLab lightness above which a background counts as light, and so gets
/// a desk of its own.
const LIGHT_BACKGROUND: f32 = 0.5;

#[derive(Clone, Copy, Default)]
struct Layout {
    widget_gap: Option<f32>,
    window_margin: Option<f32>,
}

/// A native widget definition applied to a subtree.
// Some roles are available to future surfaces but not used yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Input,
    Menu,
    Dialog,
    Sidebar,
    Tooltip,
    Separator,
    Scrollbar,
}

/// A surface with its own native frame.
// Some surfaces are available to future callers but not used yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    TitleBar,
    Dialog,
    Menu,
    Tooltip,
    Sidebar,
    Central,
}

/// The parts of the theme the custom-painted chrome needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub panel: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub danger: Color32,
    pub danger_text: Color32,
    pub warning: Color32,
    pub link: Color32,
    pub selection: Color32,
    pub selection_text: Color32,
    pub border: Color32,
    pub title_bar: Color32,
    pub title_text: Color32,
    pub menu_fill: Color32,
    pub menu_hover: Color32,
    pub menu_text: Color32,
    pub input: Color32,
    pub tooltip: Color32,
    pub sidebar: Color32,
    pub sidebar_selection: Color32,
    pub dialog_fill: Color32,
    pub separator: Color32,
}

/// The resolved light and dark themes.
#[derive(Clone)]
pub struct Theme {
    light: ResolvedTheme,
    dark: ResolvedTheme,
    layout: Layout,
    fonts: fonts::Fonts,
    mode: egui::Theme,
    /// The OS asks for less motion: things jump instead of gliding.
    reduce_motion: bool,
}

impl Theme {
    /// A bundled preset, for tests and for the fallback.
    pub fn preset(name: &str) -> Option<Self> {
        let preset = native_theme::Theme::preset(name).ok()?;
        let light = preset.resolve(native_theme::ColorMode::Light).ok()?.variant;
        let dark = preset.resolve(native_theme::ColorMode::Dark).ok()?.variant;
        Some(Self {
            light,
            dark,
            layout: Layout::default(),
            fonts: fonts::Fonts::default(),
            mode: egui::Theme::Dark,
            reduce_motion: false,
        })
    }

    /// Reads the OS theme, falling back to Adwaita with one warning.
    pub fn from_system() -> Self {
        Self::from_system_result(
            native_theme::SystemTheme::from_system().map_err(|e| e.to_string()),
        )
    }

    /// Reads the OS theme without falling back; a failure keeps the current theme.
    fn try_from_system() -> Result<Self, String> {
        native_theme::SystemTheme::from_system()
            .map(Self::from_native)
            .map_err(|error| error.to_string())
    }

    fn from_system_result(result: Result<native_theme::SystemTheme, String>) -> Self {
        match result {
            Ok(system) => Self::from_native(system),
            Err(error) => {
                eprintln!("noido: cannot read the system theme ({error}); using Adwaita");
                Self::preset("adwaita").expect("the bundled Adwaita preset exists")
            }
        }
    }

    fn from_native(system: native_theme::SystemTheme) -> Self {
        let mode = match system.mode {
            native_theme::ColorMode::Dark => egui::Theme::Dark,
            _ => egui::Theme::Light,
        };
        let fonts = match mode {
            egui::Theme::Dark => fonts::Fonts::load(&system.dark),
            egui::Theme::Light => fonts::Fonts::load(&system.light),
        };
        Self {
            light: system.light,
            dark: system.dark,
            layout: Layout {
                widget_gap: system.layout.widget_gap,
                window_margin: system.layout.window_margin,
            },
            fonts,
            mode,
            reduce_motion: system.accessibility.reduce_motion,
        }
    }

    /// Whether the OS asks for reduced motion.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    #[cfg(test)]
    pub fn with_reduce_motion(mut self, reduce_motion: bool) -> Self {
        self.reduce_motion = reduce_motion;
        self
    }

    /// The theme currently installed on `ctx`.
    pub fn current(ctx: &egui::Context) -> Arc<Self> {
        ctx.data(|data| data.get_temp::<Arc<Self>>(egui::Id::unique(THEME_ID)))
            .expect("the noido theme is installed before the UI runs")
    }

    /// Installs both variants and remembers this theme for custom painting.
    pub fn install(&self, ctx: &egui::Context) {
        ctx.set_theme(self.mode);
        fonts::install(ctx, &self.fonts);
        ctx.set_style_of(egui::Theme::Light, self.style(egui::Theme::Light));
        ctx.set_style_of(egui::Theme::Dark, self.style(egui::Theme::Dark));
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::unique(THEME_ID), Arc::new(self.clone()));
        });
    }

    /// The egui style for one variant.
    pub fn style(&self, mode: egui::Theme) -> egui::Style {
        style::global(self.variant(mode), &self.layout, &self.fonts, mode)
    }

    /// Applies one role's definition to a style, for a scoped subtree.
    pub fn apply_role(&self, mode: egui::Theme, style: &mut egui::Style, role: Role) {
        style::role(self.variant(mode), style, role);
    }

    /// The frame for a surface with its own padding, radius or fill.
    pub fn frame(&self, mode: egui::Theme, surface: Surface, style: &egui::Style) -> egui::Frame {
        style::frame(self.variant(mode), surface, style)
    }

    /// The values custom-painted UI reads.
    pub fn palette(&self, mode: egui::Theme) -> Palette {
        let theme = self.variant(mode);
        Palette {
            panel: convert::color(theme.defaults.background_color),
            text: convert::color(theme.defaults.text_color),
            muted: convert::color(theme.defaults.muted_color),
            danger: convert::color(theme.defaults.danger_color),
            danger_text: convert::color(theme.defaults.danger_text_color),
            warning: convert::color(theme.defaults.warning_color),
            link: convert::color(theme.defaults.link_color),
            selection: convert::color(theme.defaults.selection_background),
            selection_text: convert::color(theme.defaults.selection_text_color),
            border: convert::color(theme.defaults.border.color),
            title_bar: convert::color(theme.window.title_bar_background),
            title_text: convert::color(theme.window.title_bar_font.color),
            menu_fill: convert::color(theme.menu.background_color),
            menu_hover: convert::color(theme.menu.hover_background),
            menu_text: convert::color(theme.menu.font.color),
            input: convert::color(theme.input.background_color),
            tooltip: convert::color(theme.tooltip.background_color),
            sidebar: convert::color(theme.sidebar.background_color),
            sidebar_selection: convert::color(theme.sidebar.selection_background),
            dialog_fill: convert::color(theme.dialog.background_color),
            separator: convert::color(theme.separator.line_color),
        }
    }

    fn variant(&self, mode: egui::Theme) -> &ResolvedTheme {
        match mode {
            egui::Theme::Light => &self.light,
            egui::Theme::Dark => &self.dark,
        }
    }
}

/// The colour behind the pages for a window background: a light background
/// is stepped down in OKLab so paper stands out from it, and a dark one is
/// its own desk.
pub(super) fn desk(background: Color32) -> Color32 {
    if convert::lightness(background) > LIGHT_BACKGROUND {
        convert::darken(background, DESK_STEP)
    } else {
        background
    }
}

/// Installs a bundled preset; for tests and the fixed harness baseline.
#[cfg(test)]
pub fn install_preset(ctx: &egui::Context, name: &str) {
    Theme::preset(name)
        .expect("the bundled preset exists")
        .install(ctx);
}

/// Runs `add` with one native role's style.
pub fn scope<R>(ui: &mut egui::Ui, role: Role, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let theme = Theme::current(ui.ctx());
    let mode = ui.ctx().theme();
    ui.scope(|ui| {
        let mut style = (**ui.style()).clone();
        theme.apply_role(mode, &mut style, role);
        ui.set_style(style);
        add(ui)
    })
    .inner
}

/// The room a solid scroll bar takes across, under the scrollbar role;
/// `None` for overlay bars, which take none.
pub fn solid_bar(ui: &egui::Ui) -> Option<f32> {
    let theme = Theme::current(ui.ctx());
    let mut style = (**ui.style()).clone();
    theme.apply_role(ui.ctx().theme(), &mut style, Role::Scrollbar);
    let scroll = style.spacing.scroll;
    (!scroll.floating).then(|| scroll.allocated_width())
}

/// The room an overlay bar covers across the right edge of its area, under
/// the scrollbar role: lists that draw to the edge keep clear of it. `None`
/// for solid bars, which take their room themselves.
pub fn floating_bar(ui: &egui::Ui) -> Option<f32> {
    let theme = Theme::current(ui.ctx());
    let mut style = (**ui.style()).clone();
    theme.apply_role(ui.ctx().theme(), &mut style, Role::Scrollbar);
    let scroll = style.spacing.scroll;
    scroll
        .floating
        .then(|| scroll.bar_width.max(scroll.allocated_width()))
}

/// `area` showing exactly the solid `bars`, at once: egui would otherwise
/// show a bar from the frame after it is needed, and slide it in, while the
/// content is already laid out beside it. `last` is the area's id from the
/// frame before, if it was shown.
pub fn with_solid_bars(
    ctx: &egui::Context,
    area: egui::ScrollArea,
    bars: noido_core::Bars,
    last: Option<egui::Id>,
) -> egui::ScrollArea {
    if let Some(id) = last {
        // No time to animate in: the bar is there, or not, now.
        ctx.animate_bool_with_time(id.with("h"), bars.horizontal, 0.0);
        ctx.animate_bool_with_time(id.with("v"), bars.vertical, 0.0);
    }
    area.scroll([bars.horizontal, bars.vertical])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
}

/// Paints the grooves the solid `bars` run in, along the right and bottom
/// of `outer`: a strip a shade toward the text colour, with a hairline on
/// its inner edge, as AppKit draws them. Call it under the scrollbar role,
/// before the scroll area, so the bars are drawn over it.
pub fn paint_grooves(ui: &egui::Ui, outer: egui::Rect, bars: noido_core::Bars) {
    let width = ui.style().spacing.scroll.allocated_width();
    let palette = Theme::current(ui.ctx()).palette(ui.ctx().theme());
    let fill = palette.panel.lerp_to_gamma(palette.text, 0.04);
    let line = egui::Stroke::new(1.0 / ui.ctx().pixels_per_point(), palette.border);
    let painter = ui.painter();
    let mut right = outer.max.x;
    if bars.vertical {
        let groove = outer.with_min_x(outer.max.x - width);
        painter.rect_filled(groove, 0.0, fill);
        // Not through the corner, where the horizontal groove meets it.
        let bottom = if bars.horizontal {
            outer.max.y - width
        } else {
            outer.max.y
        };
        painter.vline(groove.min.x, groove.min.y..=bottom, line);
        right = groove.min.x;
    }
    if bars.horizontal {
        let groove = outer.with_min_y(outer.max.y - width).with_max_x(right);
        painter.rect_filled(groove, 0.0, fill);
        painter.hline(groove.x_range(), groove.min.y, line);
    }
}

/// Paints the thumbs of the solid `bars` of the scroll area `shown`: a
/// capsule the theme's thumb width, centred in the groove where egui's own
/// (transparent) thumb is, lit while hovered or dragged. Call it under the
/// scrollbar role, after the scroll area.
pub fn paint_thumbs<R>(
    ui: &egui::Ui,
    shown: &egui::scroll_area::ScrollAreaOutput<R>,
    bars: noido_core::Bars,
) {
    paint_thumbs_at(ui, shown, bars, shown.state.clamped_offset());
}

/// [`paint_thumbs`], placed for the scroll `offset` given rather than the
/// area's own: drawn on the way to it while it eases there.
pub fn paint_thumbs_at<R>(
    ui: &egui::Ui,
    shown: &egui::scroll_area::ScrollAreaOutput<R>,
    bars: noido_core::Bars,
    offset: egui::Vec2,
) {
    let theme = Theme::current(ui.ctx());
    let scrollbar = &theme.variant(ui.ctx().theme()).scrollbar;
    let scroll = ui.style().spacing.scroll;
    let groove = scroll.allocated_width();
    let thumb = scrollbar.thumb_width.min(groove);
    let inner = shown.inner_rect;
    let content = shown.content_size;
    for (d, needed) in [(0, bars.horizontal), (1, bars.vertical)] {
        if !needed {
            continue;
        }
        // Along the bar, as egui places its thumb.
        let (start, end) = (inner.min[d], inner.max[d]);
        let view = inner.size()[d];
        let from_content = |c: f32| {
            if content[d] > 0.0 {
                egui::remap_clamp(c, 0.0..=content[d], start..=end)
            } else {
                start
            }
        };
        let length = (from_content(offset[d] + view) - from_content(offset[d]))
            .max(scroll.handle_min_length)
            .min(end - start);
        let max_offset = content[d] - view;
        let along = if max_offset > 0.0 {
            egui::remap_clamp(offset[d], 0.0..=max_offset, start..=end - length)
        } else {
            start
        };
        // Across it: the whole groove for hovering, the thumb centred in it.
        let groove_start = inner.max[1 - d];
        let across = groove_start + (groove - thumb) / 2.0;
        let rect = |from: f32, to: f32| {
            if d == 0 {
                egui::Rect::from_min_max(egui::pos2(along, from), egui::pos2(along + length, to))
            } else {
                egui::Rect::from_min_max(egui::pos2(from, along), egui::pos2(to, along + length))
            }
        };
        let colour = if ui.ctx().is_being_dragged(shown.id.with(d)) {
            scrollbar
                .thumb_active_color
                .unwrap_or(scrollbar.thumb_hover_color)
        } else if ui.rect_contains_pointer(rect(groove_start, groove_start + groove)) {
            scrollbar.thumb_hover_color
        } else {
            scrollbar.thumb_color
        };
        ui.painter().rect_filled(
            rect(across, across + thumb),
            thumb / 2.0,
            convert::color(colour),
        );
    }
}

/// Shows `text` over `response` with the tooltip role.
pub fn hover_text(response: egui::Response, text: impl Into<egui::WidgetText>) -> egui::Response {
    tooltip(response, text.into(), false)
}

/// Shows `text` at the pointer with the tooltip role.
pub fn hover_text_at_pointer(
    response: egui::Response,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    tooltip(response, text.into(), true)
}

fn tooltip(response: egui::Response, text: egui::WidgetText, at_pointer: bool) -> egui::Response {
    let ctx = response.ctx.clone();
    let theme = Theme::current(&ctx);
    let mode = ctx.theme();
    let mut tooltip = egui::Tooltip::for_enabled(&response);
    if at_pointer {
        tooltip = tooltip.at_pointer();
    }
    tooltip.popup = tooltip.popup.style(move |style: &mut egui::Style| {
        theme.apply_role(mode, style, Role::Tooltip);
    });
    tooltip.show(|ui| {
        ui.label(text);
    });
    response
}

#[cfg(test)]
mod tests {
    use super::{Role, Theme, desk, scope};
    use eframe::egui::{self, Color32, ThemePreference};

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
    fn a_role_scope_restores_the_outer_style() {
        let ctx = egui::Context::default();
        let theme = Theme::preset("adwaita").expect("adwaita");
        theme.install(&ctx);
        let mut inside = None;
        let mut outside = None;
        let output = ctx.run_ui(Default::default(), |ui| {
            outside = Some(ui.visuals().panel_fill);
            scope(ui, Role::Sidebar, |ui| {
                inside = Some(ui.visuals().panel_fill)
            });
        });
        output.drop_without_applying_deltas();
        assert_ne!(inside, outside);
        assert_eq!(outside, Some(Color32::from_rgb(0x22, 0x22, 0x26)));
    }

    #[test]
    fn install_selects_the_theme_mode() {
        let ctx = egui::Context::default();
        ctx.set_theme(ThemePreference::Dark);
        let mut theme = Theme::preset("adwaita").expect("adwaita");
        theme.mode = egui::Theme::Light;
        theme.install(&ctx);
        assert_eq!(ctx.theme(), egui::Theme::Light);
    }

    #[test]
    fn a_failed_system_read_falls_back_to_adwaita() {
        let theme = Theme::from_system_result(Err("no portal".into()));
        assert_eq!(
            theme.palette(egui::Theme::Dark).panel,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }

    /// The WCAG contrast ratio of two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn luminance(colour: Color32) -> f32 {
            let channel = |value: u8| {
                let value = value as f32 / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(colour.r())
                + 0.7152 * channel(colour.g())
                + 0.0722 * channel(colour.b())
        }
        let (dark, light) = {
            let (a, b) = (luminance(a), luminance(b));
            if a < b { (a, b) } else { (b, a) }
        };
        (light + 0.05) / (dark + 0.05)
    }

    #[test]
    fn a_light_background_gets_a_darker_desk() {
        let desk = desk(Color32::from_rgb(0xfa, 0xfa, 0xfb));
        assert_eq!(desk, Color32::from_rgb(0xd2, 0xd2, 0xd3));
        assert!(contrast(desk, Color32::WHITE) >= 1.35, "{desk:?}");
    }

    #[test]
    fn a_dark_background_is_its_own_desk() {
        assert_eq!(
            desk(Color32::from_rgb(0x22, 0x22, 0x26)),
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }
}
