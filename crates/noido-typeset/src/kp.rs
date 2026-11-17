//! Knuth–Plass line breaking: choose every break in a paragraph together,
//! minimising the total demerits of its lines.

pub(crate) const INFINITY: i32 = 10_000;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Item {
    /// Something that is set as is: `cluster` indexes the caller's clusters
    /// (`None` for the paragraph indent).
    Box { width: f32, cluster: Option<usize> },
    /// A space that may stretch or shrink; a break may follow a box here.
    Glue {
        width: f32,
        stretch: f32,
        shrink: f32,
        cluster: Option<usize>,
    },
    /// A possible break that costs `penalty`; `width` is set only when the
    /// break is taken (the hyphen). Flagged breaks are hyphens: two in a row
    /// cost extra.
    Penalty {
        width: f32,
        penalty: i32,
        flagged: bool,
    },
}

#[derive(Clone, Copy)]
struct Node {
    position: usize,
    fitness: u8,
    width: f32,
    stretch: f32,
    shrink: f32,
    demerits: f64,
    previous: Option<usize>,
}

pub(crate) struct Params {
    /// The largest adjustment ratio allowed: 1 is all of the stretch.
    pub(crate) tolerance: f32,
    pub(crate) line_penalty: f64,
    pub(crate) flagged_demerits: f64,
    pub(crate) fitness_demerits: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            tolerance: 2.0,
            line_penalty: 10.0,
            flagged_demerits: 3000.0,
            fitness_demerits: 100.0,
        }
    }
}

/// The chosen breaks (item indices) and each line's adjustment ratio. The
/// last break is the paragraph's end, so `items` must end with a forced
/// break (a penalty of `-INFINITY`).
///
/// A paragraph with no good breaks is still set: the tolerance is relaxed
/// step by step, and the last pass accepts any line, overfull ones too,
/// taking the least bad.
///
/// A ratio is never below -1: a line that cannot shrink enough overflows
/// the measure instead, and the glue is not squeezed past its shrink. Large
/// positive ratios are returned as they are; only the demerits cap them.
pub(crate) fn breaks(items: &[Item], line_width: f32, params: &Params) -> Vec<(usize, f32)> {
    if items.is_empty() {
        return Vec::new();
    }
    debug_assert!(
        matches!(items[items.len() - 1], Item::Penalty { penalty, .. } if penalty == -INFINITY),
        "a paragraph ends with a forced break"
    );
    for tolerance in [params.tolerance, 4.0, 20.0, f32::INFINITY] {
        if let Some(b) = try_breaks(items, line_width, params, tolerance) {
            return b;
        }
    }
    // Even the last pass found nothing: the paragraph has no forced end.
    let end = items.len() - 1;
    vec![(end, ratio_of(items, 0, end, line_width))]
}

fn try_breaks(
    items: &[Item],
    line_width: f32,
    params: &Params,
    tolerance: f32,
) -> Option<Vec<(usize, f32)>> {
    // The last pass takes any line, overfull ones too.
    let anything = tolerance.is_infinite();
    let mut nodes = vec![Node {
        position: 0,
        fitness: 1,
        width: 0.0,
        stretch: 0.0,
        shrink: 0.0,
        demerits: 0.0,
        previous: None,
    }];
    let mut active = vec![0usize];
    let (mut w, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);

    for (i, item) in items.iter().enumerate() {
        let candidate = match *item {
            Item::Box { .. } => None,
            Item::Glue { .. } => matches!(items.get(i.wrapping_sub(1)), Some(Item::Box { .. }))
                .then_some((0, 0.0, false)),
            Item::Penalty {
                width,
                penalty,
                flagged,
            } => (penalty < INFINITY).then_some((penalty, width, flagged)),
        };
        if let Some((penalty, pwidth, flagged)) = candidate {
            let mut best: [Option<(f64, usize)>; 4] = [None; 4];
            let mut keep = Vec::with_capacity(active.len());
            for &a in &active {
                let node = nodes[a];
                let length = w - node.width + pwidth;
                let ratio = if length < line_width {
                    let s = y - node.stretch;
                    if s > 0.0 {
                        (line_width - length) / s
                    } else {
                        f32::INFINITY
                    }
                } else if length > line_width {
                    let s = z - node.shrink;
                    if s > 0.0 {
                        (line_width - length) / s
                    } else {
                        f32::NEG_INFINITY
                    }
                } else {
                    0.0
                };
                if !((ratio < -1.0 && !anything) || penalty == -INFINITY) {
                    keep.push(a);
                }
                if (ratio >= -1.0 || anything) && ratio <= tolerance {
                    // An overfull line is judged by what is left over once
                    // all its glue has shrunk, so that the last pass can tell
                    // bad from worse; a loose one is capped.
                    let judged = if ratio < -1.0 {
                        let left = length - line_width - (z - node.shrink);
                        1.0 + left.max(0.0) / line_width.max(1.0) * 1000.0
                    } else {
                        ratio.min(1000.0)
                    };
                    let badness = 100.0 * (judged.abs() as f64).powi(3);
                    let mut d = (params.line_penalty + badness).powi(2);
                    if penalty >= 0 {
                        d += (penalty as f64).powi(2);
                    } else if penalty > -INFINITY {
                        d -= (penalty as f64).powi(2);
                    }
                    if flagged && was_flagged(items, node.position) {
                        d += params.flagged_demerits;
                    }
                    let fitness = if ratio < -0.5 {
                        0
                    } else if ratio <= 0.5 {
                        1
                    } else if ratio <= 1.0 {
                        2
                    } else {
                        3
                    };
                    if (fitness as i32 - node.fitness as i32).abs() > 1 {
                        d += params.fitness_demerits;
                    }
                    let total = node.demerits + d;
                    let slot = &mut best[fitness as usize];
                    if slot.is_none_or(|(t, _)| total < t) {
                        *slot = Some((total, a));
                    }
                }
            }
            active = keep;
            // What the next line starts with: the glue after the break is
            // dropped from it, so it is counted here to be taken off.
            let (mut tw, mut ty, mut tz) = (w, y, z);
            for item in &items[i..] {
                match *item {
                    Item::Box { .. } => break,
                    Item::Glue {
                        width,
                        stretch,
                        shrink,
                        ..
                    } => {
                        tw += width;
                        ty += stretch;
                        tz += shrink;
                    }
                    Item::Penalty { .. } => {}
                }
            }
            for (fitness, slot) in best.iter().enumerate() {
                if let Some((demerits, previous)) = *slot {
                    nodes.push(Node {
                        position: i,
                        fitness: fitness as u8,
                        width: tw,
                        stretch: ty,
                        shrink: tz,
                        demerits,
                        previous: Some(previous),
                    });
                    active.push(nodes.len() - 1);
                }
            }
            if active.is_empty() {
                return None;
            }
        }
        match *item {
            Item::Box { width, .. } => w += width,
            Item::Glue {
                width,
                stretch,
                shrink,
                ..
            } => {
                w += width;
                y += stretch;
                z += shrink;
            }
            Item::Penalty { .. } => {}
        }
    }

    let last = items.len() - 1;
    let end = active
        .iter()
        .copied()
        .filter(|&a| nodes[a].position == last)
        .min_by(|&a, &b| nodes[a].demerits.total_cmp(&nodes[b].demerits))?;
    let mut out = Vec::new();
    let mut at = Some(end);
    while let Some(n) = at {
        let node = nodes[n];
        if let Some(p) = node.previous {
            out.push((
                node.position,
                ratio_of(items, nodes[p].position, node.position, line_width),
            ));
        }
        at = node.previous;
    }
    out.reverse();
    Some(out)
}

fn was_flagged(items: &[Item], position: usize) -> bool {
    matches!(
        items.get(position),
        Some(Item::Penalty { flagged: true, .. })
    ) && position > 0
}

/// The first item of a line that follows a break at `position`.
pub(crate) fn line_start(items: &[Item], position: usize) -> usize {
    if position == 0 {
        return 0;
    }
    let mut i = position;
    while i < items.len() && !matches!(items[i], Item::Box { .. }) {
        i += 1;
    }
    i
}

/// The adjustment ratio of the line from a break at `from` to one at `to`.
fn ratio_of(items: &[Item], from: usize, to: usize, line_width: f32) -> f32 {
    let (mut w, mut y, mut z) = (0.0, 0.0, 0.0);
    // A break just before the end can leave nothing for the line.
    let start = line_start(items, from).min(to);
    for item in &items[start..to] {
        match *item {
            Item::Box { width, .. } => w += width,
            Item::Glue {
                width,
                stretch,
                shrink,
                ..
            } => {
                w += width;
                y += stretch;
                z += shrink;
            }
            Item::Penalty { .. } => {}
        }
    }
    if let Item::Penalty { width, .. } = items[to] {
        w += width;
    }
    if w < line_width {
        if y > 0.0 { (line_width - w) / y } else { 0.0 }
    } else if z > 0.0 {
        ((line_width - w) / z).max(-1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(width: f32) -> Item {
        Item::Box {
            width,
            cluster: None,
        }
    }

    /// A space of 4: stretch half of it, shrink a third.
    fn g() -> Item {
        Item::Glue {
            width: 4.0,
            stretch: 2.0,
            shrink: 4.0 / 3.0,
            cluster: None,
        }
    }

    fn pen(width: f32, penalty: i32, flagged: bool) -> Item {
        Item::Penalty {
            width,
            penalty,
            flagged,
        }
    }

    /// The finishing glue and the forced break.
    fn end(items: &mut Vec<Item>) {
        items.push(Item::Glue {
            width: 0.0,
            stretch: 1000.0,
            shrink: 0.0,
            cluster: None,
        });
        items.push(pen(0.0, -INFINITY, false));
    }

    fn words(widths: &[f32]) -> Vec<Item> {
        let mut items = Vec::new();
        for (n, &w) in widths.iter().enumerate() {
            if n > 0 {
                items.push(g());
            }
            items.push(b(w));
        }
        end(&mut items);
        items
    }

    #[test]
    fn one_line_when_it_fits() {
        let items = words(&[10.0, 10.0, 10.0]);
        let got = breaks(&items, 100.0, &Params::default());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, items.len() - 1);
    }

    #[test]
    fn breaks_balance_lines() {
        let items = words(&[
            10.0, 11.0, 9.0, 10.0, 12.0, 8.0, 10.0, 11.0, 9.0, 10.0, 10.0, 11.0, 9.0, 10.0, 12.0,
            8.0, 10.0, 11.0,
        ]);
        let got = breaks(&items, 52.0, &Params::default());
        assert!(got.len() > 2);
        for &(_, ratio) in &got[..got.len() - 1] {
            assert!((-1.0..=1.0).contains(&ratio), "ratio {ratio}");
        }
    }

    #[test]
    fn a_hyphen_is_taken_when_it_lowers_demerits() {
        // Without the hyphen the first line would have to stretch to 3.
        let mut items = vec![
            b(30.0),
            g(),
            b(20.0),
            pen(3.0, 50, true),
            b(25.0),
            g(),
            b(10.0),
        ];
        end(&mut items);
        let got = breaks(&items, 60.0, &Params::default());
        assert_eq!(got[0].0, 3);
    }

    #[test]
    fn two_hyphens_in_a_row_cost_more() {
        // Each break can be a flagged or an unflagged penalty at the same
        // place; the lines are equal, so only the flags tell them apart.
        let mut items = vec![b(20.0), b(20.0), pen(0.0, 0, true), pen(0.0, 0, false)];
        items.extend([b(20.0), b(20.0), pen(0.0, 0, true), pen(0.0, 0, false)]);
        items.extend([b(20.0), b(20.0)]);
        end(&mut items);
        let got = breaks(&items, 40.0, &Params::default());
        assert_eq!(got.len(), 3);
        let flagged = |at: usize| matches!(items[at], Item::Penalty { flagged: true, .. });
        assert!(!(flagged(got[0].0) && flagged(got[1].0)));
    }

    #[test]
    fn an_impossible_paragraph_is_still_set() {
        let items = words(&[30.0, 200.0, 30.0, 20.0]);
        let got = breaks(&items, 100.0, &Params::default());
        assert_sane(&items, &got);
        // The wide box is on a line of its own.
        assert!(got.len() >= 2, "{got:?}");
    }

    #[test]
    fn a_loose_line_is_accepted_as_a_last_resort() {
        // A forced break after two short words: far beyond any tolerance.
        let mut items = vec![b(10.0), g(), b(10.0), pen(0.0, -INFINITY, false)];
        items.extend([b(10.0), g(), b(10.0)]);
        end(&mut items);
        let got = breaks(&items, 100.0, &Params::default());
        assert_eq!(got.iter().map(|b| b.0).collect::<Vec<_>>(), [3, 8]);
    }

    fn assert_sane(items: &[Item], got: &[(usize, f32)]) {
        assert_eq!(got.last().map(|b| b.0), Some(items.len() - 1), "{items:?}");
        assert!(got.windows(2).all(|p| p[0].0 < p[1].0), "{got:?}");
        for &(_, ratio) in got {
            assert!(ratio >= -1.0, "ratio {ratio} in {got:?}");
        }
    }

    #[test]
    fn an_overfull_line_never_squeezes_glue_past_its_shrink() {
        for widths in [
            &[30.0, 200.0, 30.0, 20.0][..],
            &[30.0, 130.0, 30.0, 20.0],
            &[90.0, 90.0, 90.0],
        ] {
            let items = words(widths);
            let got = breaks(&items, 100.0, &Params::default());
            assert_sane(&items, &got);
        }
    }

    #[test]
    fn trailing_glue_after_an_overfull_box_is_set() {
        let mut items = vec![b(18.0), b(101.0), g()];
        end(&mut items);
        let got = breaks(&items, 50.0, &Params::default());
        assert_sane(&items, &got);
    }

    #[test]
    fn random_paragraphs_are_always_set() {
        let mut seed = 12345u64;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        for _ in 0..3000 {
            let mut items = Vec::new();
            for _ in 0..next(14) + 1 {
                items.push(match next(5) {
                    0 | 1 => b(next(120) as f32 + 1.0),
                    2 => Item::Glue {
                        width: next(8) as f32,
                        stretch: next(5) as f32,
                        shrink: next(3) as f32,
                        cluster: None,
                    },
                    3 => pen(
                        next(4) as f32,
                        [-50, 0, 50, INFINITY][next(4) as usize],
                        next(2) == 0,
                    ),
                    _ => b(next(20) as f32 + 1.0),
                });
            }
            end(&mut items);
            let width = 20.0 + next(80) as f32;
            let got = breaks(&items, width, &Params::default());
            assert_sane(&items, &got);
        }
    }

    /// First-fit: each line takes as many words as fit within its shrink.
    fn greedy(items: &[Item], line_width: f32) -> Vec<(usize, f32)> {
        let last = items.len() - 1;
        let mut out = Vec::new();
        let mut from = 0;
        while from < last {
            let mut chosen = None;
            for at in from + 1..=last {
                let candidate = match items[at] {
                    Item::Glue { .. } => matches!(items[at - 1], Item::Box { .. }),
                    Item::Penalty { penalty, .. } => penalty < INFINITY,
                    Item::Box { .. } => false,
                };
                if !candidate {
                    continue;
                }
                let ratio = ratio_of(items, from, at, line_width);
                if ratio > -1.0 || chosen.is_none() {
                    chosen = Some((at, ratio));
                }
                if ratio <= -1.0 {
                    break;
                }
            }
            let (at, ratio) = chosen.unwrap();
            out.push((at, ratio));
            from = at;
        }
        out
    }

    #[test]
    fn the_whole_paragraph_is_weighed_not_each_line_in_turn() {
        // Filling the first line (to ratio -0.6) leaves a line of ratio 4.5;
        // breaking earlier keeps every line within 1.
        let items = words(&[10.0, 8.0, 9.0, 11.0, 8.0, 8.0, 12.0, 20.0, 12.0, 19.0]);
        let max = |v: &[(usize, f32)]| v.iter().fold(0.0f32, |m, r| m.max(r.1.abs()));
        let first_fit = greedy(&items, 70.0);
        let got = breaks(&items, 70.0, &Params::default());
        assert_ne!(
            got.iter().map(|b| b.0).collect::<Vec<_>>(),
            first_fit.iter().map(|b| b.0).collect::<Vec<_>>()
        );
        assert!(max(&first_fit) > 4.0, "{first_fit:?}");
        assert!(max(&got) <= 1.0, "{got:?}");
    }
}
