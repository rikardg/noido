//! A value drawn eased after where it is, for the sidebar's scroll bar
//! thumb following the current page.

/// A value as drawn: when it moved by following something, it is drawn
/// catching up on a critically damped spring, which keeps its speed when
/// the value moves on again (a held key) and never overshoots; when it
/// moved any other way, it is drawn there at once.
/// The spring's stiffness, per second: from rest it is within half a
/// percent of the value after a quarter second, there after a third.
const STIFFNESS: f32 = 30.0;

/// Close enough to count as there.
const NEAR: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default)]
pub struct Eased {
    drawn: Option<f32>,
    /// The speed it is drawn moving at, per second.
    velocity: f32,
    /// Where it is catching up to, while it is.
    target: Option<f32>,
}

impl Eased {
    /// The value to draw for the `actual` one, `dt` seconds after the last
    /// call; `follows` when it moved this frame by following, so it eases.
    pub fn update(&mut self, actual: f32, follows: bool, dt: f32) -> f32 {
        let Some(drawn) = self.drawn else {
            self.drawn = Some(actual);
            return actual;
        };
        if follows {
            self.target = Some(actual);
        } else if self
            .target
            .is_some_and(|target| (target - actual).abs() > NEAR)
        {
            // Moved some other way: no catching up.
            self.target = None;
        }
        let Some(target) = self.target else {
            self.velocity = 0.0;
            self.drawn = Some(actual);
            return actual;
        };
        // One exact step of a critically damped spring: stable for any dt.
        let gap = drawn - target;
        let decay = (-STIFFNESS * dt).exp();
        let push = (self.velocity + STIFFNESS * gap) * dt;
        let mut velocity = (self.velocity - STIFFNESS * push) * decay;
        let mut next = (gap + push) * decay;
        if next * gap < 0.0 {
            // Never past it.
            next = 0.0;
            velocity = 0.0;
        }
        if next.abs() < NEAR && velocity.abs() < STIFFNESS * NEAR {
            self.target = None;
            self.velocity = 0.0;
            self.drawn = Some(target);
            return target;
        }
        self.velocity = velocity;
        self.drawn = Some(target + next);
        target + next
    }

    /// Whether it is still catching up, and wants drawing again.
    pub fn moving(&self) -> bool {
        self.target.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    /// Draws `frames` frames of a value standing at `actual`.
    fn settle(eased: &mut Eased, actual: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|_| eased.update(actual, false, FRAME))
            .collect()
    }

    #[test]
    fn draws_the_value_at_first() {
        let mut eased = Eased::default();
        assert_eq!(eased.update(200.0, true, FRAME), 200.0);
        assert!(!eased.moving());
    }

    #[test]
    fn moves_at_once_when_not_following() {
        let mut eased = Eased::default();
        eased.update(0.0, false, FRAME);
        assert_eq!(eased.update(300.0, false, FRAME), 300.0);
        assert!(!eased.moving());
    }

    #[test]
    fn catches_up_when_following() {
        let mut eased = Eased::default();
        eased.update(0.0, false, FRAME);
        // On its way from the frame it follows, not there at once.
        let first = eased.update(300.0, true, FRAME);
        assert!(first > 0.0 && first < 150.0, "{first}");
        assert!(eased.moving());
        let drawn = settle(&mut eased, 300.0, 20);
        let mut last = first;
        for value in &drawn {
            assert!(*value >= last && *value <= 300.0, "{first} {drawn:?}");
            last = *value;
        }
        // There within a third of a second, and still.
        assert_eq!(last, 300.0, "{first} {drawn:?}");
        assert!(!eased.moving());
    }

    #[test]
    fn a_long_gap_since_the_last_frame_still_eases() {
        let mut eased = Eased::default();
        eased.update(0.0, false, FRAME);
        // The caller hands in a frame's time, not the idle gap: whatever
        // it hands in, a step is never past the value.
        let drawn = eased.update(300.0, true, 5.0);
        assert!(drawn <= 300.0, "{drawn}");
    }

    #[test]
    fn snaps_when_moved_otherwise_midway() {
        let mut eased = Eased::default();
        eased.update(0.0, false, FRAME);
        eased.update(300.0, true, FRAME);
        eased.update(300.0, false, FRAME);
        // The reader scrolls: drawn where the content is.
        assert_eq!(eased.update(250.0, false, FRAME), 250.0);
        assert!(!eased.moving());
        assert_eq!(eased.update(250.0, false, FRAME), 250.0);
    }

    #[test]
    fn following_on_keeps_its_speed() {
        let mut eased = Eased::default();
        eased.update(0.0, false, FRAME);
        let a = eased.update(300.0, true, FRAME);
        let b = eased.update(300.0, false, FRAME);
        // Moved on mid-way, as by a held key: no step back, no restart, and
        // no slower than it was going.
        let c = eased.update(600.0, true, FRAME);
        assert!(c - b >= b - a, "{a} {b} {c}");
        assert!(c < 600.0);
    }

    #[test]
    fn a_held_key_moves_it_evenly() {
        // A step every three frames (20 a second, at 60 Hz).
        let mut eased = Eased::default();
        eased.update(0.0, false, FRAME);
        let mut drawn = vec![0.0];
        for i in 0..60 {
            let actual = 330.0 * (i / 3 + 1) as f32;
            drawn.push(eased.update(actual, i % 3 == 0, FRAME));
        }
        let steps: Vec<f32> = drawn.windows(2).map(|w| w[1] - w[0]).collect();
        // Past the start, the fastest step is less than twice the slowest.
        let steady = &steps[20..];
        let fastest = steady.iter().cloned().fold(f32::MIN, f32::max);
        let slowest = steady.iter().cloned().fold(f32::MAX, f32::min);
        assert!(slowest > 0.0 && fastest < 2.0 * slowest, "{steps:?}");
    }
}
