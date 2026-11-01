//! Small conversions from `native-theme`'s resolved values to egui's types.

use eframe::egui::{Color32, CornerRadius, Margin, Stroke};
use native_theme::Rgba;

pub(crate) fn color(value: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(value.r, value.g, value.b, value.a)
}

/// The OKLab lightness of `colour`: 0.0 for black, 1.0 for white.
pub(crate) fn lightness(colour: Color32) -> f32 {
    oklab(colour).0
}

/// `colour` with its OKLab lightness lowered by `step`. Hue and chroma are
/// kept as far as the sRGB gamut allows, and lightness stops at black rather
/// than wrapping round.
pub(crate) fn darken(colour: Color32, step: f32) -> Color32 {
    let (l, a, b) = oklab(colour);
    from_oklab((l - step).clamp(0.0, 1.0), a, b, colour.a())
}

/// `colour` as OKLab lightness, green-red and blue-yellow.
fn oklab(colour: Color32) -> (f32, f32, f32) {
    let [r, g, b, _] = colour.to_srgba_unmultiplied();
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

/// OKLab back to sRGB, with `alpha`.
fn from_oklab(l: f32, a: f32, b: f32, alpha: u8) -> Color32 {
    let (long, medium, short) = (
        l + 0.396_337_78 * a + 0.215_803_76 * b,
        l - 0.105_561_346 * a - 0.063_854_17 * b,
        l - 0.089_484_18 * a - 1.291_485_5 * b,
    );
    let (l, m, s) = (
        long * long * long,
        medium * medium * medium,
        short * short * short,
    );
    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
    Color32::from_rgba_unmultiplied(srgb(r), srgb(g), srgb(b), alpha)
}

/// sRGB from 0-255 to linear light.
fn linear(channel: u8) -> f32 {
    let channel = channel as f32 / 255.0;
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light back to sRGB 0-255.
fn srgb(channel: f32) -> u8 {
    let channel = if channel <= 0.003_130_8 {
        12.92 * channel
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    };
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
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

#[cfg(test)]
mod tests {
    use super::{darken, lightness, oklab};
    use eframe::egui::Color32;

    /// Within one step per channel, for float round trips.
    fn close(a: Color32, b: Color32) -> bool {
        [a.r(), a.g(), a.b(), a.a()]
            .iter()
            .zip([b.r(), b.g(), b.b(), b.a()])
            .all(|(a, b)| a.abs_diff(b) <= 1)
    }

    #[test]
    fn darkening_by_nothing_keeps_the_colour() {
        for colour in [
            Color32::BLACK,
            Color32::WHITE,
            Color32::from_rgb(0xfa, 0xfa, 0xfb),
            Color32::from_rgb(0x35, 0x84, 0xe4),
        ] {
            assert!(close(darken(colour, 0.0), colour), "{colour:?}");
        }
    }

    #[test]
    fn adwaita_light_stepped_down_is_a_grey() {
        assert_eq!(
            darken(Color32::from_rgb(0xfa, 0xfa, 0xfb), 0.12),
            Color32::from_rgb(0xd2, 0xd2, 0xd3)
        );
    }

    #[test]
    fn darkening_keeps_the_hue_and_chroma() {
        for colour in [
            Color32::from_rgb(0x35, 0x84, 0xe4),
            Color32::from_rgb(0xf4, 0xec, 0xd8),
        ] {
            let (l, a, b) = oklab(colour);
            let (dark_l, dark_a, dark_b) = oklab(darken(colour, 0.12));
            assert!((l - 0.12 - dark_l).abs() < 0.01, "{colour:?}");
            assert!((a - dark_a).abs() < 0.01, "{colour:?}");
            assert!((b - dark_b).abs() < 0.01, "{colour:?}");
        }
    }

    #[test]
    fn black_stays_black() {
        assert_eq!(darken(Color32::BLACK, 0.12), Color32::BLACK);
    }

    #[test]
    fn lightness_runs_from_black_to_white() {
        assert!(lightness(Color32::BLACK) < 0.01);
        assert!(lightness(Color32::WHITE) > 0.99);
        assert!(lightness(Color32::from_rgb(0xfa, 0xfa, 0xfb)) > 0.5);
        assert!(lightness(Color32::from_rgb(0x22, 0x22, 0x26)) < 0.5);
    }
}
