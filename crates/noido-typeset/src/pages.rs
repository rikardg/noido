//! Page breaking: places the lines of a sequence of flows onto pages.
//! Pure; knows nothing of fonts. Heights are in points.
//!
//! Rules, in the order they apply:
//! - a paragraph never leaves exactly one line at the foot of a page,
//!   nor exactly one line (its last) at the head of the next;
//!   a paragraph of three lines or fewer therefore never splits (on a
//!   fresh page it may, when nothing else fits);
//! - a heading is never split, and keeps (chaining through any headings
//!   that follow it) at least the first two lines of
//!   what follows (all of it if shorter) on its page, else it moves on;
//! - a `new_page` flow starts a page unless it is already first on one;
//! - space before is dropped at a page's top; space after is not carried
//!   over a page break;
//! - flows without lines are skipped.
//!
//! When a rule cannot be met because the page is already empty (a line or
//! a heading taller than the page, a page holding a single line), the page
//! is filled instead: at least one line (a heading: all of it, overflowing)
//! is placed, so the loop always advances.

pub(crate) struct Flow {
    /// Height of each line.
    pub lines: Vec<f32>,
    pub before: f32,
    pub after: f32,
    pub heading: bool,
    pub new_page: bool,
}

pub(crate) struct Placed {
    pub flow: usize,
    pub line: usize,
    pub page: usize,
    /// Top of the line on its page.
    pub y: f32,
}

const EPS: f32 = 1e-3;

/// How many of `lines`, starting at `start`, go on this page.
/// `splittable` false: all or nothing. `fresh`: the page is empty, so
/// rules that cannot be met give way to filling the page.
fn take(lines: &[f32], start: f32, limit: f32, splittable: bool, fresh: bool) -> usize {
    let n = lines.len();
    let mut y = start;
    let mut fit = 0;
    for h in lines {
        if y + h > limit + EPS {
            break;
        }
        y += h;
        fit += 1;
    }
    if fit == n {
        return n;
    }
    if !splittable {
        return if fresh { n } else { 0 };
    }
    let mut k = fit;
    if n - k == 1 {
        k = k.saturating_sub(1);
    }
    if k < 2 {
        k = 0;
    }
    if k == 0 && fresh {
        k = fit.max(1);
    }
    k
}

pub(crate) fn paginate(flows: &[Flow], top: f32, height: f32) -> (Vec<Placed>, usize) {
    let limit = top + height;
    let mut out = Vec::new();
    let mut page = 0;
    let mut y = top;
    let mut fresh = true;

    for (i, f) in flows.iter().enumerate() {
        let n = f.lines.len();
        if n == 0 {
            continue;
        }
        if f.new_page && !fresh {
            page += 1;
            y = top;
            fresh = true;
        }
        let mut done = 0;
        while done < n {
            let before = if done == 0 && !fresh { f.before } else { 0.0 };
            let start = y + before;
            let rest = &f.lines[done..];
            let mut k = take(rest, start, limit, !f.heading, fresh);
            if k > 0 && f.heading && !fresh {
                // Keep with what follows, through further headings, up to
                // a paragraph's first two lines.
                let mut end = start + rest.iter().sum::<f32>() + f.after;
                for next in flows[i + 1..].iter().filter(|n| !n.lines.is_empty()) {
                    if next.new_page {
                        break;
                    }
                    let at = end + next.before;
                    if next.heading {
                        let got = take(&next.lines, at, limit, false, false);
                        if got == 0 {
                            k = 0;
                            break;
                        }
                        end = at + next.lines.iter().sum::<f32>() + next.after;
                    } else {
                        let need = next.lines.len().min(2);
                        if take(&next.lines, at, limit, true, false) < need {
                            k = 0;
                        }
                        break;
                    }
                }
            }
            if k == 0 {
                page += 1;
                y = top;
                fresh = true;
                continue;
            }
            y = start;
            for (j, h) in rest[..k].iter().enumerate() {
                out.push(Placed {
                    flow: i,
                    line: done + j,
                    page,
                    y,
                });
                y += h;
            }
            fresh = false;
            done += k;
            if done < n {
                page += 1;
                y = top;
                fresh = true;
            } else {
                y += f.after;
            }
        }
    }
    (out, page + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: f32 = 14.0;

    fn para(n: usize) -> Flow {
        Flow {
            lines: vec![L; n],
            before: 0.0,
            after: 0.0,
            heading: false,
            new_page: false,
        }
    }

    fn heading(new_page: bool) -> Flow {
        Flow {
            lines: vec![L],
            before: 0.0,
            after: 0.0,
            heading: true,
            new_page,
        }
    }

    fn run(flows: &[Flow]) -> (Vec<Placed>, usize) {
        paginate(flows, 0.0, 10.0 * L)
    }

    fn per_page(placed: &[Placed], pages: usize) -> Vec<usize> {
        (0..pages)
            .map(|p| placed.iter().filter(|l| l.page == p).count())
            .collect()
    }

    fn of_flow(placed: &[Placed], flow: usize) -> Vec<usize> {
        let mut v = vec![0; placed.iter().map(|l| l.page).max().map_or(1, |m| m + 1)];
        for l in placed.iter().filter(|l| l.flow == flow) {
            v[l.page] += 1;
        }
        v
    }

    #[test]
    fn no_paragraph_leaves_one_line_at_the_foot() {
        let (placed, pages) = run(&[para(9), para(3)]);
        assert_eq!(per_page(&placed, pages), [9, 3]);
    }

    #[test]
    fn no_paragraph_leaves_one_line_at_the_head() {
        // 7 + 4: three fit, which would leave one behind; two go.
        let (placed, pages) = run(&[para(7), para(4)]);
        assert_eq!(per_page(&placed, pages), [9, 2]);
        assert_eq!(of_flow(&placed, 1), [2, 2]);
    }

    #[test]
    fn a_short_paragraph_that_would_split_moves_whole() {
        let (placed, pages) = run(&[para(8), para(3)]);
        assert_eq!(per_page(&placed, pages), [8, 3]);
        let (placed, pages) = run(&[para(9), para(2)]);
        assert_eq!(per_page(&placed, pages), [9, 2]);
    }

    #[test]
    fn a_single_line_that_does_not_fit_moves() {
        let (placed, pages) = run(&[para(10), para(1)]);
        assert_eq!(per_page(&placed, pages), [10, 1]);
    }

    #[test]
    fn a_heading_takes_two_lines_with_it() {
        // Heading on line 9 with room for one more line: it moves.
        let (placed, pages) = run(&[para(8), heading(false), para(3)]);
        assert_eq!(per_page(&placed, pages), [8, 4]);
        assert_eq!(placed[8].page, 1);
        assert_eq!(placed[8].flow, 1);
    }

    #[test]
    fn a_heading_stays_when_two_lines_follow_it() {
        let (placed, pages) = run(&[para(6), heading(false), para(5)]);
        assert_eq!(per_page(&placed, pages), [10, 2]);
        assert_eq!(placed[6].page, 0);
    }

    #[test]
    fn a_heading_before_a_short_paragraph_needs_all_of_it() {
        let (placed, pages) = run(&[para(8), heading(false), para(1)]);
        assert_eq!(per_page(&placed, pages), [10]);
        let (placed, pages) = run(&[para(9), heading(false), para(1)]);
        assert_eq!(per_page(&placed, pages), [9, 2]);
    }

    #[test]
    fn a_level_one_heading_starts_a_page() {
        let (placed, pages) = run(&[para(2), heading(true), para(2)]);
        assert_eq!(per_page(&placed, pages), [2, 3]);
        assert_eq!(placed[2].page, 1);
        assert_eq!(placed[2].y, 0.0);
    }

    #[test]
    fn a_level_one_heading_first_on_its_page_adds_no_page() {
        let (placed, pages) = run(&[heading(true), para(2)]);
        assert_eq!(pages, 1);
        assert_eq!(per_page(&placed, pages), [3]);
        // And after a page break caused by a full page.
        let (_, pages) = run(&[para(10), heading(true), para(2)]);
        assert_eq!(pages, 2);
    }

    #[test]
    fn a_paragraph_longer_than_a_page_splits() {
        let (placed, pages) = run(&[para(25)]);
        assert_eq!(per_page(&placed, pages), [10, 10, 5]);
        assert!(placed.windows(2).all(|w| w[0].line + 1 == w[1].line));
        let (placed, pages) = run(&[para(21)]);
        assert_eq!(per_page(&placed, pages), [10, 9, 2]);
    }

    #[test]
    fn nothing_gives_one_blank_page() {
        let (placed, pages) = run(&[]);
        assert!(placed.is_empty());
        assert_eq!(pages, 1);
    }

    #[test]
    fn lines_without_content_are_skipped() {
        let (placed, pages) = run(&[heading(true), para(0), para(1)]);
        assert_eq!((placed.len(), pages), (2, 1));
    }

    #[test]
    fn space_before_is_dropped_at_the_top_and_kept_elsewhere() {
        let mut spaced = para(2);
        spaced.before = 7.0;
        spaced.after = 3.0;
        let mut second = para(2);
        second.before = 7.0;
        let (placed, _) = run(&[spaced, second]);
        assert_eq!(placed[0].y, 0.0);
        assert_eq!(placed[2].y, 2.0 * L + 3.0 + 7.0);
    }

    #[test]
    fn space_before_is_dropped_after_a_page_break() {
        let mut second = para(3);
        second.before = 7.0;
        let (placed, _) = run(&[para(9), second]);
        assert_eq!(placed[9].page, 1);
        assert_eq!(placed[9].y, 0.0);
    }

    #[test]
    fn a_top_offset_shifts_every_line() {
        let (placed, _) = paginate(&[para(2)], 56.0, 10.0 * L);
        assert_eq!(placed[0].y, 56.0);
        assert_eq!(placed[1].y, 56.0 + L);
    }

    #[test]
    fn a_line_taller_than_the_page_is_placed_alone_not_looped_on() {
        let flows = [Flow {
            lines: vec![200.0, 200.0, 200.0],
            ..para(0)
        }];
        let (placed, pages) = run(&flows);
        assert_eq!(placed.len(), 3);
        assert_eq!(pages, 3);
    }

    #[test]
    fn a_heading_taller_than_the_page_overflows_it() {
        let flows = [
            para(1),
            Flow {
                lines: vec![100.0, 100.0],
                heading: true,
                ..para(0)
            },
            para(2),
        ];
        let (placed, pages) = run(&flows);
        assert_eq!(placed.len(), 5);
        assert_eq!(pages, 3);
        assert_eq!(placed[1].page, 1);
        assert_eq!(placed[2].page, 1);
    }

    #[test]
    fn chained_headings_move_together_with_the_paragraph() {
        // h2 at line 9, h3 at line 10, paragraph does not fit.
        let (placed, pages) = run(&[para(8), heading(false), heading(false), para(3)]);
        assert_eq!(per_page(&placed, pages), [8, 5]);
        assert_eq!(placed[8].page, 1);
    }

    #[test]
    fn a_heading_looks_past_an_empty_flow() {
        let (placed, pages) = run(&[para(9), heading(false), para(0), para(3)]);
        assert_eq!(per_page(&placed, pages), [9, 4]);
    }

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n
        }
    }

    #[test]
    fn invariants_hold_over_many_shapes() {
        let heights = [14.0, 14.0, 14.0, 30.0, 200.0];
        let (top, height) = (20.0, 140.0);
        let mut rng = Lcg(7);
        for case in 0..400 {
            let count = 1 + rng.next(7);
            let flows: Vec<Flow> = (0..count)
                .map(|_| {
                    let heading = rng.next(3) == 0;
                    let n = rng.next(13);
                    Flow {
                        lines: (0..if heading { n.min(2) } else { n })
                            .map(|_| heights[rng.next(heights.len())])
                            .collect(),
                        before: [0.0, 7.0, 14.0][rng.next(3)],
                        after: [0.0, 7.0][rng.next(2)],
                        heading,
                        new_page: rng.next(5) == 0,
                    }
                })
                .collect();
            let (placed, pages) = paginate(&flows, top, height);
            let expect: Vec<(usize, usize)> = flows
                .iter()
                .enumerate()
                .flat_map(|(i, f)| (0..f.lines.len()).map(move |l| (i, l)))
                .collect();
            let got: Vec<(usize, usize)> = placed.iter().map(|p| (p.flow, p.line)).collect();
            assert_eq!(got, expect, "case {case}: every line once, in order");
            assert!(
                placed.windows(2).all(|w| w[0].page <= w[1].page),
                "case {case}"
            );
            assert!(placed.iter().all(|p| p.page < pages), "case {case}");
            for (idx, p) in placed.iter().enumerate() {
                let h = flows[p.flow].lines[p.line];
                if p.y + h > top + height + 1e-3 {
                    // Only a first line on a page (or a heading starting
                    // one) may overflow.
                    let first = placed
                        .iter()
                        .position(|q| q.page == p.page)
                        .expect("page has a line");
                    let ok =
                        first == idx || (flows[p.flow].heading && placed[first].flow == p.flow);
                    assert!(ok, "case {case}: overflow of line {idx}");
                }
            }
            // A heading is never last on its page when its next flow
            // starts on the following page.
            for (i, f) in flows.iter().enumerate() {
                if !f.heading || f.lines.is_empty() {
                    continue;
                }
                let last = placed.iter().rev().find(|p| p.flow == i).unwrap();
                let first = placed.iter().find(|p| p.flow == i).unwrap();
                let Some(nj) = (i + 1..flows.len()).find(|&j| !flows[j].lines.is_empty()) else {
                    continue;
                };
                if flows[nj].new_page {
                    continue;
                }
                let next = placed.iter().find(|p| p.flow == nj).unwrap();
                if next.page > last.page {
                    assert!(
                        first.y == top && first.page == last.page,
                        "case {case}: heading {i} stranded"
                    );
                }
            }
        }
    }
}
