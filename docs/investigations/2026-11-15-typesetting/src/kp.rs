//! Knuth–Plass line breaking: choose every break in a paragraph together,
//! minimising the total demerits of its lines.

pub const INFINITY: i32 = 10_000;

#[derive(Clone, Copy, Debug)]
pub enum Item {
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

pub struct Params {
    /// The largest adjustment ratio allowed: 1 is all of the stretch.
    pub tolerance: f32,
    pub line_penalty: f64,
    pub flagged_demerits: f64,
    pub fitness_demerits: f64,
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

/// The chosen breaks (item indices) and each line's adjustment ratio.
pub fn breaks(items: &[Item], line_width: f32, params: &Params) -> Vec<(usize, f32)> {
    for tolerance in [params.tolerance, 4.0, 20.0, f32::INFINITY] {
        if let Some(b) = try_breaks(items, line_width, params, tolerance) {
            return b;
        }
    }
    unreachable!("the last pass accepts any line")
}

fn try_breaks(
    items: &[Item],
    line_width: f32,
    params: &Params,
    tolerance: f32,
) -> Option<Vec<(usize, f32)>> {
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
            Item::Glue { .. } => {
                matches!(items.get(i.wrapping_sub(1)), Some(Item::Box { .. })).then_some((0, 0.0, false))
            }
            Item::Penalty {
                width,
                penalty,
                flagged,
            } => (penalty < INFINITY).then_some((penalty, width, flagged)),
        };
        if let Some((penalty, pwidth, flagged)) = candidate {
            let mut best: [Option<(f64, usize, f32)>; 4] = [None; 4];
            let mut keep = Vec::with_capacity(active.len());
            for &a in &active {
                let node = nodes[a];
                let length = w - node.width + pwidth;
                let ratio = if length < line_width {
                    let s = y - node.stretch;
                    if s > 0.0 { (line_width - length) / s } else { f32::INFINITY }
                } else if length > line_width {
                    let s = z - node.shrink;
                    if s > 0.0 { (line_width - length) / s } else { f32::NEG_INFINITY }
                } else {
                    0.0
                };
                if !(ratio < -1.0 || penalty == -INFINITY) {
                    keep.push(a);
                }
                if ratio >= -1.0 && ratio <= tolerance {
                    let badness = 100.0 * (ratio.abs() as f64).powi(3);
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
                    if slot.is_none_or(|(t, _, _)| total < t) {
                        *slot = Some((total, a, ratio));
                    }
                }
            }
            active = keep;
            // What the next line starts with: the glue and penalties after
            // the break are dropped.
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
                    Item::Penalty { penalty, .. } if penalty == -INFINITY && i > 0 => {}
                    Item::Penalty { .. } => {}
                }
            }
            for (fitness, slot) in best.iter().enumerate() {
                if let Some((demerits, previous, _)) = *slot {
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
            out.push((node.position, ratio_of(items, nodes[p].position, node.position, line_width)));
        }
        at = node.previous;
    }
    out.reverse();
    Some(out)
}

fn was_flagged(items: &[Item], position: usize) -> bool {
    matches!(items.get(position), Some(Item::Penalty { flagged: true, .. })) && position > 0
}

/// The first item of a line that follows a break at `position`.
pub fn line_start(items: &[Item], position: usize) -> usize {
    if position == 0 {
        return 0;
    }
    let mut i = position;
    while i < items.len() && !matches!(items[i], Item::Box { .. }) {
        i += 1;
    }
    i
}

pub fn ratio_of(items: &[Item], from: usize, to: usize, line_width: f32) -> f32 {
    let (mut w, mut y, mut z) = (0.0, 0.0, 0.0);
    for item in &items[line_start(items, from)..to] {
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
        (line_width - w) / z
    } else {
        0.0
    }
}
