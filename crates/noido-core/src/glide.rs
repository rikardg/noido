//! A value drawn eased after where it is, for the sidebar's scroll bar
//! thumb following the current page.

/// A value as drawn: when it moved by following something, it is drawn
/// catching up on a critically damped spring, which keeps its speed when
/// the value moves on again (a held key) and never overshoots; when it
/// moved any other way, it is drawn there at once.
/// How briskly a value catches up with what it follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// After a step, such as a key press: there in a quarter second.
    Quick,
    /// After continuous moves, such as a flick: slow enough to still be
    /// moving when the next comes, there about 0.6 s after the last.
    Gentle,
}

impl Pace {
    /// The spring's stiffness, per second. From rest, `k` is within half a
    /// percent of the value after 7.4/`k` seconds.
    fn stiffness(self) -> f32 {
        match self {
            Pace::Quick => 30.0,
            Pace::Gentle => 12.0,
        }
    }
}

/// Close enough to count as there.
const NEAR: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default)]
pub struct Eased {
    drawn: Option<f32>,
    /// The speed it is drawn moving at, per second.
    velocity: f32,
    /// Where it is catching up to, while it is, and how briskly.
    target: Option<(f32, Pace)>,
}

impl Eased {
    /// The value to draw for the `actual` one, `dt` seconds after the last
    /// call; `follow` when it moved this frame by following, so it eases at
    /// that pace.
    pub fn update(&mut self, actual: f32, follow: Option<Pace>, dt: f32) -> f32 {
        let Some(drawn) = self.drawn else {
            self.drawn = Some(actual);
            return actual;
        };
        if let Some(pace) = follow {
            self.target = Some((actual, pace));
        } else if self
            .target
            .is_some_and(|(target, _)| (target - actual).abs() > NEAR)
        {
            // Moved some other way: no catching up.
            self.target = None;
        }
        let Some((target, pace)) = self.target else {
            self.velocity = 0.0;
            self.drawn = Some(actual);
            return actual;
        };
        // One exact step of a critically damped spring: stable for any dt.
        let stiffness = pace.stiffness();
        let gap = drawn - target;
        let decay = (-stiffness * dt).exp();
        let push = (self.velocity + stiffness * gap) * dt;
        let mut velocity = (self.velocity - stiffness * push) * decay;
        let mut next = (gap + push) * decay;
        if next * gap < 0.0 {
            // Never past it.
            next = 0.0;
            velocity = 0.0;
        }
        if next.abs() < NEAR && velocity.abs() < stiffness * NEAR {
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
            .map(|_| eased.update(actual, None, FRAME))
            .collect()
    }

    #[test]
    fn draws_the_value_at_first() {
        let mut eased = Eased::default();
        assert_eq!(eased.update(200.0, Some(Pace::Quick), FRAME), 200.0);
        assert!(!eased.moving());
    }

    #[test]
    fn moves_at_once_when_not_following() {
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        assert_eq!(eased.update(300.0, None, FRAME), 300.0);
        assert!(!eased.moving());
    }

    #[test]
    fn catches_up_when_following() {
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        // On its way from the frame it follows, not there at once.
        let first = eased.update(300.0, Some(Pace::Quick), FRAME);
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
        eased.update(0.0, None, FRAME);
        // The caller hands in a frame's time, not the idle gap: whatever
        // it hands in, a step is never past the value.
        let drawn = eased.update(300.0, Some(Pace::Quick), 5.0);
        assert!(drawn <= 300.0, "{drawn}");
    }

    #[test]
    fn snaps_when_moved_otherwise_midway() {
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        eased.update(300.0, Some(Pace::Quick), FRAME);
        eased.update(300.0, None, FRAME);
        // The reader scrolls: drawn where the content is.
        assert_eq!(eased.update(250.0, None, FRAME), 250.0);
        assert!(!eased.moving());
        assert_eq!(eased.update(250.0, None, FRAME), 250.0);
    }

    #[test]
    fn following_on_keeps_its_speed() {
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        let a = eased.update(300.0, Some(Pace::Quick), FRAME);
        let b = eased.update(300.0, None, FRAME);
        // Moved on mid-way, as by a held key: no step back, no restart, and
        // no slower than it was going.
        let c = eased.update(600.0, Some(Pace::Quick), FRAME);
        assert!(c - b >= b - a, "{a} {b} {c}");
        assert!(c < 600.0);
    }

    #[test]
    fn a_held_key_moves_it_evenly() {
        // A step every three frames (20 a second, at 60 Hz).
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        let mut drawn = vec![0.0];
        for i in 0..60 {
            let actual = 330.0 * (i / 3 + 1) as f32;
            drawn.push(eased.update(actual, (i % 3 == 0).then_some(Pace::Quick), FRAME));
        }
        let steps: Vec<f32> = drawn.windows(2).map(|w| w[1] - w[0]).collect();
        // Past the start, the fastest step is less than twice the slowest.
        let steady = &steps[20..];
        let fastest = steady.iter().cloned().fold(f32::MIN, f32::max);
        let slowest = steady.iter().cloned().fold(f32::MAX, f32::min);
        assert!(slowest > 0.0 && fastest < 2.0 * slowest, "{steps:?}");
    }

    #[test]
    fn gentle_takes_longer_than_quick() {
        let run = |pace| {
            let mut eased = Eased::default();
            eased.update(0.0, None, FRAME);
            eased.update(300.0, Some(pace), FRAME);
            settle(&mut eased, 300.0, 20).pop().unwrap()
        };
        assert_eq!(run(Pace::Quick), 300.0);
        assert!(run(Pace::Gentle) < 299.0);
    }

    #[test]
    fn gentle_gets_there() {
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        eased.update(300.0, Some(Pace::Gentle), FRAME);
        let drawn = settle(&mut eased, 300.0, 60);
        assert_eq!(*drawn.last().unwrap(), 300.0, "{drawn:?}");
        assert!(!eased.moving());
    }

    /// A flick: the value moves on further apart each time, 80 ms apart at
    /// first. Gently, it never stands still until after the last move.
    #[test]
    fn gentle_keeps_moving_through_a_flick() {
        let mut eased = Eased::default();
        eased.update(0.0, None, FRAME);
        let (mut actual, mut next, mut gap) = (0.0, 0.0, 0.08);
        let mut last_move = 0.0;
        let mut steps = vec![];
        let mut drawn = 0.0;
        for frame in 0..90 {
            let t = frame as f32 * FRAME;
            let follow = (t >= next).then(|| {
                actual += 330.0;
                last_move = t;
                next += gap;
                gap *= 1.35;
                Pace::Gentle
            });
            let now = eased.update(actual, follow, FRAME);
            steps.push((t, now - drawn));
            drawn = now;
        }
        for (t, step) in &steps {
            if *t > FRAME && *t < last_move {
                assert!(*step > 1.0, "still at {t}: {steps:?}");
            }
        }
    }
}
