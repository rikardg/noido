//! Small conversions from `native-theme`'s resolved values to egui's types.

use eframe::egui::{Color32, CornerRadius, Margin, Stroke};
use native_theme::Rgba;

pub(crate) fn color(value: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(value.r, value.g, value.b, value.a)
}

pub(crate) fn stroke(width: f32, value: Rgba) -> Stroke {
    Stroke::new(width, color(value))
}

pub(crate) fn corner_radius(radius: f32) -> CornerRadius {
    CornerRadius::same(radius.round().clamp(0.0, u8::MAX as f32) as u8)
}

fn side(value: Option<f32>, fallback: i8) -> i8 {
    value.map_or(fallback, |value| {
        value.round().clamp(0.0, i8::MAX as f32) as i8
    })
}

pub(crate) fn margin(
    left: Option<f32>,
    right: Option<f32>,
    top: Option<f32>,
    bottom: Option<f32>,
    fallback: Margin,
) -> Margin {
    Margin {
        left: side(left, fallback.left),
        right: side(right, fallback.right),
        top: side(top, fallback.top),
        bottom: side(bottom, fallback.bottom),
    }
}

pub(crate) fn shadow(
    mut base: eframe::egui::Shadow,
    enabled: bool,
    value: Rgba,
) -> eframe::egui::Shadow {
    if !enabled {
        return eframe::egui::Shadow::NONE;
    }
    base.color = color(value);
    base
}
