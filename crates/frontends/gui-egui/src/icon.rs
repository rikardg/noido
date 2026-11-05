//! The app icon. `assets/noido.svg` is its only source: `build.rs` renders
//! the window icon from it, and `--install` puts the SVG itself where the
//! Linux desktop looks for icons.

use eframe::egui::IconData;

/// The icon's source, as installed for the desktop.
pub const SVG: &str = include_str!("../assets/noido.svg");

/// Side of the window icon in pixels (`build.rs` holds the same number).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const SIDE: u32 = 256;

/// The window icon: straight (not premultiplied) RGBA, row by row.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn window_icon() -> IconData {
    IconData {
        rgba: include_bytes!(concat!(env!("OUT_DIR"), "/icon.rgba")).to_vec(),
        width: SIDE,
        height: SIDE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKGROUND: [u8; 3] = [0x0f, 0x33, 0x02];

    fn pixel(icon: &IconData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * icon.width + x) * 4) as usize;
        icon.rgba[i..i + 4].try_into().unwrap()
    }

    fn near(a: &[u8], b: &[u8]) -> bool {
        a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 2)
    }

    #[test]
    fn window_icon_is_rendered_from_the_svg() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (SIDE, SIDE));
        assert_eq!(icon.rgba.len(), (SIDE * SIDE * 4) as usize);
        // Outside the corner: transparent.
        assert_eq!(pixel(&icon, 0, 0)[3], 0);
        // Near the left edge, halfway down: the green, opaque.
        assert!(near(&pixel(&icon, 8, SIDE / 2), &[0x0f, 0x33, 0x02, 255]));
        // The letter: a thin one, about 7 % of the square is white.
        let white = icon
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| near(*p, &[255, 255, 255, 255]))
            .count();
        assert!(white > (SIDE * SIDE / 20) as usize, "{white} white pixels");
    }

    #[test]
    fn window_icon_tile_has_straight_sides() {
        // Apple's continuous corners: each corner's curve takes 1.6 × its
        // radius of the side (radius 22.5 % of the side, smoothing 60 %),
        // and the side between is straight. At 256 that is rows 93 to 162
        // of the left edge, opaque right out to the first column.
        let icon = window_icon();
        for y in 93..=162 {
            let alpha = pixel(&icon, 0, y)[3];
            assert_eq!(alpha, 255, "row {y}: the side is not straight");
        }
        // Row 50, in the corner's curve: it is 1.2 px in from the edge,
        // where a superellipse (n = 5), whose sides bulge, is 2.2 px in.
        let alpha = pixel(&icon, 1, 50)[3];
        assert!(alpha > 128, "alpha {alpha}: the corner is not Apple's");
    }

    #[test]
    fn window_icon_alpha_is_straight() {
        // The corner's soft edge keeps the green's colour at partial
        // alpha; premultiplied pixels would come out darker.
        let icon = window_icon();
        let edge: Vec<_> = icon
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| (64..=192).contains(&p[3]))
            .collect();
        assert!(!edge.is_empty());
        for p in edge {
            assert!(near(&p[..3], &BACKGROUND), "{p:?}");
        }
    }
}
