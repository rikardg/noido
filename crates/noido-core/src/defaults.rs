//! What a document noido hasn't seen before opens with.

use crate::{MAX_ZOOM, MIN_ZOOM, ViewState, ZoomMode};

/// The range of a fixed starting zoom, in percent: zoom's own range.
pub const MIN_ZOOM_PERCENT: u32 = (MIN_ZOOM * 100.0) as u32;
pub const MAX_ZOOM_PERCENT: u32 = (MAX_ZOOM * 100.0) as u32;

/// The zoom a new document starts at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartZoom {
    FitWidth,
    FitPage,
    /// [`Defaults::zoom_percent`].
    Fixed,
}

/// The view a document opens with when it has no saved state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Defaults {
    pub zoom: StartZoom,
    /// The scale for [`StartZoom::Fixed`], in percent (100 is scale 1.0, as
    /// the title bar shows it); kept when another zoom is chosen.
    pub zoom_percent: u32,
    pub continuous: bool,
    pub dual: bool,
    pub odd_left: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            zoom: StartZoom::FitWidth,
            zoom_percent: 100,
            continuous: true,
            dual: false,
            odd_left: true,
        }
    }
}

impl Defaults {
    /// With the percentage within zoom's range.
    pub fn clamped(self) -> Self {
        Self {
            zoom_percent: self.zoom_percent.clamp(MIN_ZOOM_PERCENT, MAX_ZOOM_PERCENT),
            ..self
        }
    }

    /// The defaults that open documents like `state`: a custom zoom becomes a
    /// fixed one at its rounded percentage.
    pub fn from_view(state: &ViewState) -> Self {
        let (zoom, zoom_percent) = match state.zoom {
            ZoomMode::FitWidth => (StartZoom::FitWidth, Self::default().zoom_percent),
            ZoomMode::FitPage => (StartZoom::FitPage, Self::default().zoom_percent),
            ZoomMode::Custom => (StartZoom::Fixed, (state.scale * 100.0).round() as u32),
        };
        Self {
            zoom,
            zoom_percent,
            continuous: state.continuous,
            dual: state.dual,
            odd_left: state.odd_left,
        }
        .clamped()
    }

    /// The scale a fixed zoom starts at.
    pub fn fixed_scale(&self) -> f32 {
        self.clamped().zoom_percent as f32 / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocPoint, ViewState, ZoomMode};

    fn state(zoom: ZoomMode, scale: f32) -> ViewState {
        ViewState {
            zoom,
            scale,
            continuous: false,
            dual: true,
            odd_left: false,
            centre: DocPoint {
                page: 3,
                x: 0.0,
                y: 0.0,
            },
        }
    }

    #[test]
    fn defaults_default_values() {
        assert_eq!(
            Defaults::default(),
            Defaults {
                zoom: StartZoom::FitWidth,
                zoom_percent: 100,
                continuous: true,
                dual: false,
                odd_left: true,
            }
        );
    }

    #[test]
    fn clamped_keeps_percentage_in_range() {
        let with = |zoom_percent| Defaults {
            zoom_percent,
            ..Defaults::default()
        };
        assert_eq!(with(0).clamped().zoom_percent, 10);
        assert_eq!(with(9999).clamped().zoom_percent, 400);
        assert_eq!(with(150).clamped().zoom_percent, 150);
    }

    #[test]
    fn from_view_fit_modes() {
        let defaults = Defaults::from_view(&state(ZoomMode::FitPage, 0.7));
        assert_eq!(
            defaults,
            Defaults {
                zoom: StartZoom::FitPage,
                zoom_percent: 100,
                continuous: false,
                dual: true,
                odd_left: false,
            }
        );
        let defaults = Defaults::from_view(&state(ZoomMode::FitWidth, 0.7));
        assert_eq!(defaults.zoom, StartZoom::FitWidth);
    }

    #[test]
    fn from_view_custom_becomes_fixed() {
        let defaults = Defaults::from_view(&state(ZoomMode::Custom, 1.234));
        assert_eq!(
            (defaults.zoom, defaults.zoom_percent),
            (StartZoom::Fixed, 123)
        );
        let defaults = Defaults::from_view(&state(ZoomMode::Custom, 9.0));
        assert_eq!(defaults.zoom_percent, 400);
    }
}
