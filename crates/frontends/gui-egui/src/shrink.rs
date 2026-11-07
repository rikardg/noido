//! Fitting the window to the page: the inverse of fit width and fit page.
//!
//! The window is the page area plus the chrome around it — the title bar, the
//! sidebar, the margins and the scroll bars — but only the window knows how
//! much chrome it drew, so a request carries the page area wanted and the
//! window size is worked out from the room the window gave last.

use eframe::egui::Vec2;

/// How many times a request is sent: a scroll bar coming or going changes
/// the page area by its thickness, so the first resize can land a bar's width
/// off and the next frame corrects it.
const SENDS: u8 = 2;

/// The page area a window should give, the sends left, and whether a
/// maximized window was already asked to restore.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shrink {
    viewport: (f32, f32),
    sends: u8,
    restored: bool,
}

impl Shrink {
    /// The page area to give the pages: the spread's size, or its width with
    /// the page area's height now when only the width is asked for.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            viewport: (width, height),
            sends: SENDS,
            restored: false,
        }
    }

    /// One send fewer; `None` once they are spent.
    pub fn spend(self) -> Option<Self> {
        (self.sends > 0).then(|| Self {
            sends: self.sends - 1,
            ..self
        })
    }

    /// Records that a maximized window was asked to restore.
    pub fn asked_restore(&mut self) {
        self.restored = true;
    }

    /// Whether a restore was asked already.
    pub fn restored(self) -> bool {
        self.restored
    }
}

/// The window as the target is worked out, its numbers all from one frame.
pub struct Room {
    /// The window's inner size now.
    pub size: Vec2,
    /// The page area it gives now.
    pub area: (f32, f32),
    /// The sidebar's width while it is open.
    pub sidebar: Option<f32>,
    /// The smallest the window may be.
    pub min: Vec2,
    /// The screen the window is on, when egui says.
    pub screen: Option<Vec2>,
}

/// The window's inner size for `shrink` in `room`: the page area plus the
/// chrome `room` measures, no narrower than twice an open sidebar's width
/// (below that egui narrows the sidebar), no smaller than the window's
/// minimum, and no bigger than the screen.
pub fn target(shrink: Shrink, room: &Room) -> Vec2 {
    let (width, height) = shrink.viewport;
    let mut size = Vec2::new(
        width + room.size.x - room.area.0,
        height + room.size.y - room.area.1,
    );
    if let Some(sidebar) = room.sidebar {
        size.x = size.x.max(2.0 * sidebar);
    }
    // The screen's size, not the room left where the window sits: a page
    // taller than the screen scrolls, and capping by position would take
    // that room away from pages that do fit on it.
    size.max(room.min)
        .min(room.screen.unwrap_or(Vec2::INFINITY))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    /// A window 900 × 1000 whose page area is 876 × 960: 24 points of chrome
    /// across and 40 down (the title bar).
    fn room() -> Room {
        Room {
            size: vec2(900.0, 1000.0),
            area: (876.0, 960.0),
            sidebar: None,
            min: vec2(480.0, 300.0),
            screen: None,
        }
    }

    #[test]
    fn the_chrome_comes_along() {
        // A 612 × 792 page plus the chrome the room measures.
        assert_eq!(
            target(Shrink::new(612.0, 792.0), &room()),
            vec2(636.0, 832.0)
        );
    }

    #[test]
    fn width_keeps_the_height_it_was_given() {
        assert_eq!(
            target(Shrink::new(612.0, 960.0), &room()),
            vec2(636.0, 1000.0)
        );
    }

    #[test]
    fn an_open_sidebar_is_counted_and_keeps_its_width() {
        let room = Room {
            area: (616.0, 960.0),
            sidebar: Some(260.0),
            ..room()
        };
        // A page narrower than twice the sidebar: the window stops where
        // egui would start narrowing it.
        assert_eq!(
            target(Shrink::new(200.0, 300.0), &room),
            vec2(2.0 * 260.0, 340.0)
        );
        // Room enough, and the sidebar is only counted.
        assert_eq!(target(Shrink::new(600.0, 300.0), &room), vec2(884.0, 340.0));
    }

    #[test]
    fn the_window_minimum_wins_over_a_small_page() {
        assert_eq!(target(Shrink::new(10.0, 10.0), &room()), vec2(480.0, 300.0));
    }

    #[test]
    fn the_screen_caps_a_page_bigger_than_it() {
        let room = Room {
            screen: Some(vec2(800.0, 700.0)),
            ..room()
        };
        assert_eq!(
            target(Shrink::new(2000.0, 2000.0), &room),
            vec2(800.0, 700.0)
        );
    }

    #[test]
    fn a_request_runs_out_of_sends() {
        let twice = Shrink::new(612.0, 792.0).spend().and_then(Shrink::spend);
        assert!(twice.is_some());
        assert_eq!(twice.unwrap().spend(), None);
    }

    #[test]
    fn a_restore_is_remembered() {
        let mut shrink = Shrink::new(612.0, 792.0);
        assert!(!shrink.restored());
        shrink.asked_restore();
        assert!(shrink.restored());
        // Spending sends doesn't forget it.
        assert!(shrink.spend().unwrap().restored());
    }
}
