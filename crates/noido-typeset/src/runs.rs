//! Runs: a line's page characters grouped by where they came from.

use noido_engine::{Location, Run};

/// A line's page characters in order, each with its source character
/// (None: no source). Returns (offset in the line, Run) pieces: one for
/// each stretch of characters whose sources are consecutive.
pub(crate) fn runs(
    chars: &[Option<usize>],
    part: usize,
    top: f32,
    bottom: f32,
) -> Vec<(usize, Run)> {
    let mut out: Vec<(usize, Run)> = Vec::new();
    for (i, source) in chars.iter().enumerate() {
        let Some(source) = *source else { continue };
        if let Some((offset, run)) = out.last_mut()
            && *offset + run.chars == i
            && run.start.char + run.chars == source
        {
            run.chars += 1;
            continue;
        }
        out.push((
            i,
            Run {
                start: Location { part, char: source },
                chars: 1,
                top,
                bottom,
            },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(runs: Vec<(usize, Run)>) -> Vec<(usize, usize, usize)> {
        runs.into_iter()
            .map(|(offset, r)| (offset, r.start.char, r.chars))
            .collect()
    }

    #[test]
    fn consecutive_sources_make_one_run() {
        let r = runs(&[Some(5), Some(6), Some(7)], 0, 1.0, 2.0);
        assert_eq!(shape(r.clone()), [(0, 5, 3)]);
        assert_eq!(r[0].1.start.part, 0);
        assert_eq!((r[0].1.top, r[0].1.bottom), (1.0, 2.0));
    }

    #[test]
    fn a_marker_gap_starts_a_new_run() {
        let r = runs(&[Some(5), Some(6), Some(9)], 0, 0.0, 1.0);
        assert_eq!(shape(r), [(0, 5, 2), (2, 9, 1)]);
    }

    #[test]
    fn characters_without_source_are_left_out() {
        let r = runs(&[None, Some(3), None], 0, 0.0, 1.0);
        assert_eq!(shape(r), [(1, 3, 1)]);
    }
}
