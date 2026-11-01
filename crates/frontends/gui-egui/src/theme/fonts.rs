//! The platform's own interface and monospace faces, when they can be found.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use native_theme::{ResolvedTheme, fonts::SystemFace};

/// The faces loaded for one resolved variant; `Default` keeps egui's own.
#[derive(Clone, Default)]
pub struct Fonts {
    ui: Option<SystemFace>,
    mono: Option<SystemFace>,
    heading: Option<SystemFace>,
}

impl Fonts {
    /// Loads the theme's proportional, monospace and heading faces.
    pub fn load(variant: &ResolvedTheme) -> Self {
        let font = &variant.defaults.font;
        let mono = &variant.defaults.mono_font;
        let heading = &variant.text_scale.dialog_title;
        Self {
            ui: face(&font.family, font.weight, font.style),
            mono: face(&mono.family, mono.weight, mono.style),
            heading: face(&font.family, heading.weight, font.style),
        }
    }

    /// The family egui's heading text style should use.
    pub fn heading_family(&self) -> FontFamily {
        if self.heading.is_some() {
            FontFamily::Name("noido-heading".into())
        } else {
            FontFamily::Proportional
        }
    }
}

/// The font definitions for `fonts`, or `None` when egui's should stay.
pub(super) fn definitions(fonts: &Fonts) -> Option<FontDefinitions> {
    if fonts.ui.is_none() && fonts.mono.is_none() && fonts.heading.is_none() {
        return None;
    }
    let mut definitions = FontDefinitions::default();
    if let Some(face) = &fonts.ui {
        insert(&mut definitions, "noido-ui", face, FontFamily::Proportional);
    }
    if let Some(face) = &fonts.mono {
        insert(&mut definitions, "noido-mono", face, FontFamily::Monospace);
    }
    if let Some(face) = &fonts.heading {
        definitions
            .font_data
            .insert("noido-heading".into(), Arc::new(data(face)));
        let mut chain = vec!["noido-heading".into()];
        chain.extend(
            definitions
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default(),
        );
        definitions
            .families
            .insert(FontFamily::Name("noido-heading".into()), chain);
    }
    Some(definitions)
}

/// Installs the faces; leaves egui's alone when none were found.
pub(super) fn install(ctx: &egui::Context, fonts: &Fonts) {
    if let Some(definitions) = definitions(fonts) {
        ctx.set_fonts(definitions);
    }
}

fn face(family: &str, weight: u16, style: native_theme::theme::FontStyle) -> Option<SystemFace> {
    native_theme::fonts::system_face(family, weight, style).or_else(|| {
        native_theme::fonts::substitute_family_cached(family)
            .and_then(|family| native_theme::fonts::system_face(&family, weight, style))
    })
}

fn insert(definitions: &mut FontDefinitions, key: &str, face: &SystemFace, family: FontFamily) {
    definitions
        .font_data
        .insert(key.into(), Arc::new(data(face)));
    definitions
        .families
        .entry(family)
        .or_default()
        .insert(0, key.into());
}

fn data(face: &SystemFace) -> FontData {
    let mut data = FontData::from_owned(face.data.to_vec());
    data.index = face.index;
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_faces_egui_keeps_its_fonts() {
        let fonts = Fonts::default();
        assert!(definitions(&fonts).is_none());
        assert_eq!(fonts.heading_family(), FontFamily::Proportional);
    }

    #[test]
    fn the_preset_uses_proportional_for_headings() {
        let theme = crate::theme::Theme::preset("adwaita").expect("adwaita");
        let style = theme.style(egui::Theme::Dark);
        assert_eq!(
            style.text_styles[&egui::TextStyle::Heading].family,
            FontFamily::Proportional
        );
    }
}
