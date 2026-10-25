//! The left sidebar's state, kept per document.

/// Sidebar width, in UI points, until the reader changes it.
pub const SIDEBAR_DEFAULT_WIDTH: f32 = 260.0;
/// The narrowest the sidebar gets.
pub const SIDEBAR_MIN_WIDTH: f32 = 160.0;
/// The widest the sidebar gets, as a fraction of the window width.
pub const SIDEBAR_MAX_FRACTION: f32 = 0.5;
/// The widest the sidebar gets on any window, in UI points.
pub const SIDEBAR_MAX_WIDTH: f32 = 480.0;

/// The widest the sidebar may be in a window `window_width` wide.
pub fn max_width(window_width: f32) -> f32 {
    (window_width * SIDEBAR_MAX_FRACTION).clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarState {
    pub open: bool,
    pub width: f32,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            open: false,
            width: SIDEBAR_DEFAULT_WIDTH,
        }
    }
}

impl SidebarState {
    /// With the width limited to what a window `window_width` wide allows;
    /// a width that isn't finite becomes the default.
    pub fn clamped(self, window_width: f32) -> Self {
        let width = if self.width.is_finite() {
            self.width
        } else {
            SIDEBAR_DEFAULT_WIDTH
        };
        Self {
            width: width.clamp(SIDEBAR_MIN_WIDTH, max_width(window_width)),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(width: f32, window: f32) -> f32 {
        SidebarState { open: true, width }.clamped(window).width
    }

    #[test]
    fn clamped_width() {
        assert_eq!(width(2000.0, 900.0), 450.0);
        assert_eq!(width(50.0, 900.0), 160.0);
        assert_eq!(width(f32::NAN, 900.0), 260.0);
        assert_eq!(width(300.0, 200.0), 160.0);
        assert_eq!(width(300.0, 900.0), 300.0);
    }

    #[test]
    fn wide_window_caps_width() {
        assert_eq!(width(2000.0, 3000.0), 480.0);
        assert_eq!(max_width(3000.0), 480.0);
        assert_eq!(max_width(900.0), 450.0);
        assert_eq!(max_width(200.0), 160.0);
    }

    #[test]
    fn default_is_closed_at_default_width() {
        assert_eq!(
            SidebarState::default(),
            SidebarState {
                open: false,
                width: SIDEBAR_DEFAULT_WIDTH
            }
        );
    }
}
