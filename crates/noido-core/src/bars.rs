//! Solid scroll bars: which ones a scroll area needs, and the room they
//! leave for its content.
//!
//! Solid bars take space from the area they scroll, which changes what is
//! laid out there (a fitted page gets narrower), which can change whether a
//! bar is needed. [`scroll_bars`] settles that before anything is drawn.

use crate::layout::FIT_NOISE;

/// The solid scroll bars a scroll area shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bars {
    /// Along the bottom, for content wider than the area.
    pub horizontal: bool,
    /// Down the right, for content taller than the area.
    pub vertical: bool,
}

impl Bars {
    /// The room left in an `outer` (width, height) area once these bars,
    /// each `bar` thick, have taken theirs.
    pub fn inner(self, outer: (f32, f32), bar: f32) -> (f32, f32) {
        (
            outer.0 - if self.vertical { bar } else { 0.0 },
            outer.1 - if self.horizontal { bar } else { 0.0 },
        )
    }
}

/// The bars an `outer` (width, height) area needs for content whose size,
/// laid out in a given room, is `content`; each bar is `bar` thick. A bar
/// that is needed once stays, so two layouts never alternate.
pub fn scroll_bars(
    outer: (f32, f32),
    bar: f32,
    content: impl Fn((f32, f32)) -> (f32, f32),
) -> Bars {
    let mut bars = Bars::default();
    // Each round adds a bar or ends; two bars is the most there are.
    for _ in 0..3 {
        let room = bars.inner(outer, bar);
        let (width, height) = content(room);
        let needed = Bars {
            horizontal: bars.horizontal || width > room.0 + FIT_NOISE,
            vertical: bars.vertical || height > room.1 + FIT_NOISE,
        };
        if needed == bars {
            break;
        }
        bars = needed;
    }
    bars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_that_fits_needs_no_bars() {
        let bars = scroll_bars((800.0, 600.0), 17.0, |room| room);
        assert_eq!(bars, Bars::default());
    }

    #[test]
    fn tall_content_takes_a_vertical_bar_and_is_laid_out_beside_it() {
        // Fitted to the width it is given: never wider than the room.
        let bars = scroll_bars((800.0, 600.0), 17.0, |(width, _)| (width, 5000.0));
        assert_eq!(
            bars,
            Bars {
                horizontal: false,
                vertical: true
            }
        );
        assert_eq!(bars.inner((800.0, 600.0), 17.0), (783.0, 600.0));
    }

    #[test]
    fn wide_content_takes_a_horizontal_bar_which_can_make_it_too_tall() {
        // 595 high fits 600, not the 583 the horizontal bar leaves.
        let bars = scroll_bars((800.0, 600.0), 17.0, |_| (1200.0, 595.0));
        assert_eq!(
            bars,
            Bars {
                horizontal: true,
                vertical: true
            }
        );
        assert_eq!(bars.inner((800.0, 600.0), 17.0), (783.0, 583.0));
    }

    #[test]
    fn a_bar_once_needed_stays() {
        // Fit to width: beside a vertical bar the content is short enough
        // to need none, without one it is too tall. Keep the bar.
        let bars = scroll_bars((800.0, 600.0), 17.0, |(width, _)| {
            (width, if width < 800.0 { 590.0 } else { 610.0 })
        });
        assert!(bars.vertical);
        assert!(!bars.horizontal);
    }

    #[test]
    fn content_within_half_a_point_of_the_room_fits() {
        let bars = scroll_bars((800.0, 600.0), 17.0, |_| (800.4, 600.4));
        assert_eq!(bars, Bars::default());
    }
}
