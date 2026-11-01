//! The system theme, mapped into egui.
//!
//! `native-theme` reads the OS theme; this module converts both resolved
//! variants into egui styles, stores the current one on the context, and
//! offers scoped roles for widgets egui's single global style cannot express.

mod convert;
mod style;

use std::sync::Arc;

use eframe::egui::{self, Color32};
use native_theme::ResolvedTheme;

const THEME_ID: &str = "noido-theme";

#[derive(Clone, Copy, Default)]
struct Layout {
    widget_gap: Option<f32>,
    window_margin: Option<f32>,
}

/// The parts of the theme the custom-painted chrome needs.
// Used by the surfaces in the tasks after the global style lands.
#[allow(dead_code)]
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
        })
    }

    /// Reads the OS theme, falling back to Adwaita with one warning.
    pub fn from_system() -> Self {
        Self::from_system_result(
            native_theme::SystemTheme::from_system().map_err(|e| e.to_string()),
        )
    }

    fn from_system_result(result: Result<native_theme::SystemTheme, String>) -> Self {
        match result {
            Ok(system) => Self {
                light: system.light,
                dark: system.dark,
                layout: Layout {
                    widget_gap: system.layout.widget_gap,
                    window_margin: system.layout.window_margin,
                },
            },
            Err(error) => {
                eprintln!("noido: cannot read the system theme ({error}); using Adwaita");
                Self::preset("adwaita").expect("the bundled Adwaita preset exists")
            }
        }
    }

    /// The theme currently installed on `ctx`.
    #[allow(dead_code)]
    pub fn current(ctx: &egui::Context) -> Arc<Self> {
        ctx.data(|data| data.get_temp::<Arc<Self>>(egui::Id::unique(THEME_ID)))
            .expect("the noido theme is installed before the UI runs")
    }

    /// Installs both variants and remembers this theme for custom painting.
    pub fn install(&self, ctx: &egui::Context) {
        ctx.set_style_of(egui::Theme::Light, self.style(egui::Theme::Light));
        ctx.set_style_of(egui::Theme::Dark, self.style(egui::Theme::Dark));
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::unique(THEME_ID), Arc::new(self.clone()));
        });
    }

    /// The egui style for one variant.
    pub fn style(&self, mode: egui::Theme) -> egui::Style {
        style::global(self.variant(mode), &self.layout, mode)
    }

    /// The values custom-painted UI reads.
    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::Theme;
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
    fn a_failed_system_read_falls_back_to_adwaita() {
        let theme = Theme::from_system_result(Err("no portal".into()));
        assert_eq!(
            theme.palette(egui::Theme::Dark).panel,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }
}
