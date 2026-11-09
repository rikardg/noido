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

/// What the sidebar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Thumbnails,
    Pins,
    Outline,
    Results,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarState {
    pub open: bool,
    pub width: f32,
    /// The tab shown.
    pub tab: SidebarTab,
    /// The last tab the reader chose that isn't `Results`; the one the
    /// sidebar returns to when the search ends, and the one that is saved.
    pub last: SidebarTab,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            open: false,
            width: SIDEBAR_DEFAULT_WIDTH,
            tab: SidebarTab::Thumbnails,
            last: SidebarTab::Thumbnails,
        }
    }
}

impl SidebarState {
    /// The reader picks `tab`.
    pub fn choose(&mut self, tab: SidebarTab) {
        self.tab = tab;
        if tab != SidebarTab::Results {
            self.last = tab;
        }
    }

    /// The reader started a new query: its results show.
    pub fn search_started(&mut self) {
        self.tab = SidebarTab::Results;
    }

    /// The search ended: if its results show, the tab before them returns.
    pub fn search_cleared(&mut self) {
        if self.tab == SidebarTab::Results {
            self.tab = self.last;
        }
    }

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
        SidebarState {
            open: true,
            width,
            ..Default::default()
        }
        .clamped(window)
        .width
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
                width: SIDEBAR_DEFAULT_WIDTH,
                ..Default::default()
            }
        );
    }

    fn on(tab: SidebarTab) -> SidebarState {
        let mut state = SidebarState::default();
        state.choose(tab);
        state
    }

    #[test]
    fn choosing_results_keeps_last() {
        let mut state = on(SidebarTab::Outline);
        state.choose(SidebarTab::Results);
        assert_eq!(state.tab, SidebarTab::Results);
        assert_eq!(state.last, SidebarTab::Outline);
    }

    #[test]
    fn search_started_goes_to_results() {
        let mut state = on(SidebarTab::Outline);
        state.search_started();
        assert_eq!(state.tab, SidebarTab::Results);
        assert_eq!(state.last, SidebarTab::Outline);
    }

    #[test]
    fn search_cleared_returns_to_last() {
        let mut state = on(SidebarTab::Outline);
        state.search_started();
        state.search_cleared();
        assert_eq!(state.tab, SidebarTab::Outline);
    }

    #[test]
    fn search_cleared_elsewhere_changes_nothing() {
        let mut state = on(SidebarTab::Outline);
        state.search_started();
        state.choose(SidebarTab::Thumbnails);
        state.search_cleared();
        assert_eq!(state.tab, SidebarTab::Thumbnails);
        assert_eq!(state.last, SidebarTab::Thumbnails);
    }

    #[test]
    fn clamped_keeps_the_tabs() {
        let mut state = on(SidebarTab::Outline);
        state.search_started();
        let clamped = state.clamped(900.0);
        assert_eq!((clamped.tab, clamped.last), (state.tab, state.last));
    }

    #[test]
    fn pins_is_a_tab_that_search_returns_to() {
        let mut state = on(SidebarTab::Pins);
        assert_eq!(state.last, SidebarTab::Pins);
        state.search_started();
        state.search_cleared();
        assert_eq!(state.tab, SidebarTab::Pins);
        state.choose(SidebarTab::Results);
        assert_eq!(state.last, SidebarTab::Pins);
    }
}
