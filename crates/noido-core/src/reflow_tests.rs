//! The view across a reflow, on a fake reflowable document.

use noido_engine::{Location, LocationMap, Run, Size};

use crate::{Defaults, DocPoint, Place, StartZoom, View};

const MARGIN_Y: f32 = 36.0;
const LINE: f32 = 20.0;
const TEXT: usize = 20_000;

/// `chars` characters of part 0 set on pages of `page` size, `per_line`
/// characters a line, lines `line` high between top and bottom margins of
/// 36: what a reflowable engine hands over.
fn flow(chars: usize, page: Size, line: f32, per_line: usize) -> (Vec<Size>, LocationMap) {
    let per_page = ((page.height - 2.0 * MARGIN_Y) / line).floor() as usize;
    let lines: Vec<Run> = (0..chars.div_ceil(per_line))
        .map(|i| Run {
            start: at(i * per_line),
            chars: per_line.min(chars - i * per_line),
            top: MARGIN_Y + (i % per_page) as f32 * line,
            bottom: MARGIN_Y + (i % per_page + 1) as f32 * line,
        })
        .collect();
    let pages: Vec<Vec<Run>> = lines.chunks(per_page).map(<[Run]>::to_vec).collect();
    (vec![page; pages.len()], LocationMap::from_lines(pages))
}

fn at(char: usize) -> Location {
    Location { part: 0, char }
}

/// 26 lines of 50 a page: 1300 characters, 16 pages.
fn wide() -> (Vec<Size>, LocationMap) {
    flow(
        TEXT,
        Size {
            width: 400.0,
            height: 600.0,
        },
        LINE,
        50,
    )
}

/// 26 lines of 37 a page: 962 characters, 21 pages.
fn narrow() -> (Vec<Size>, LocationMap) {
    flow(
        TEXT,
        Size {
            width: 300.0,
            height: 600.0,
        },
        LINE,
        37,
    )
}

fn view_with(sizes: Vec<Size>, defaults: Defaults) -> View {
    let mut view = View::with_defaults(sizes, &defaults);
    view.set_viewport_size(800.0, 600.0);
    view
}

fn view(sizes: Vec<Size>) -> View {
    view_with(sizes, Defaults::default())
}

/// Scrolls so `y` on `page` is `h` below the viewport's top.
fn scroll_to(view: &mut View, page: usize, y: f32, h: f32) {
    let (_, cy) = view
        .layout()
        .content_point(DocPoint { page, x: 0.0, y })
        .unwrap();
    view.scrolled_to(view.offset().0, cy - h);
}

/// How far below the viewport's top the line holding `at` is.
fn height_of(view: &View, map: &LocationMap, at: Location) -> f32 {
    let (page, run) = map.line(at).unwrap();
    let (_, y) = view
        .layout()
        .content_point(DocPoint {
            page,
            x: 0.0,
            y: run.top,
        })
        .unwrap();
    y - view.offset().1
}

/// The line `at` is on in `map`: its start.
fn line_start(map: &LocationMap, at: Location) -> Location {
    map.line(at).unwrap().1.start
}

#[test]
fn the_fake_keeps_locations_across_a_page_size_change() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    assert_eq!((wide_sizes.len(), narrow_sizes.len()), (16, 21));
    for char in (0..=TEXT).step_by(97).chain([TEXT]) {
        assert!(wide.page_char(at(char)).is_some(), "{char}");
        assert!(narrow.page_char(at(char)).is_some(), "{char}");
    }
}

#[test]
fn first_in_view_is_the_first_line_below_the_viewport_top() {
    let (sizes, map) = wide();
    let mut v = view(sizes);
    let scale = v.scale();
    // Line 3 of page 2 (36 + 40 .. 36 + 60) cut 5 points down.
    scroll_to(&mut v, 2, MARGIN_Y + 2.0 * LINE + 5.0, 0.0);
    let (first, h) = v.first_in_view(&map).unwrap();
    assert_eq!(first, at(2 * 1300 + 2 * 50));
    assert!((h + 5.0 * scale).abs() < 0.5, "{h}");
    // At line 3's bottom: line 4.
    scroll_to(&mut v, 2, MARGIN_Y + 3.0 * LINE, 0.0);
    assert_eq!(v.first_in_view(&map).unwrap().0, at(2 * 1300 + 3 * 50));
}

#[test]
fn reflow_keeps_the_line_at_its_height() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let mut v = view(wide_sizes);
    scroll_to(&mut v, 7, MARGIN_Y + 10.0 * LINE, 100.0);
    let (first, h) = v.first_in_view(&wide).unwrap();
    v.reflow(narrow_sizes, &wide, &narrow);
    assert_eq!(v.layout().page_count(), 21);
    let (now, h_now) = v.first_in_view(&narrow).unwrap();
    assert_eq!(now, line_start(&narrow, first));
    assert!((h_now - h).abs() < 0.5, "{h} → {h_now}");
}

#[test]
fn reflow_to_fewer_pages_does_not_panic() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let mut v = view(narrow_sizes);
    scroll_to(&mut v, 19, MARGIN_Y, 50.0);
    let (first, h) = v.first_in_view(&narrow).unwrap();
    assert!((h - 50.0).abs() < 0.5, "{h}");
    v.reflow(wide_sizes, &narrow, &wide);
    assert_eq!(v.layout().page_count(), 16);
    // Its line is 50 down; the line before it now shows above.
    let h_now = height_of(&v, &wide, first);
    assert!((h_now - 50.0).abs() < 0.5, "{h_now}");
}

#[test]
fn reflow_at_the_end_of_the_text_keeps_it_in_view() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let mut v = view(wide_sizes);
    v.scrolled_to(0.0, f32::MAX);
    assert_eq!(v.first_in_view(&wide), None, "only the page's foot shows");
    let first = v.here(&wide).unwrap();
    assert_eq!(first, Place::Text(at(TEXT - 50)));
    v.reflow(narrow_sizes, &wide, &narrow);
    let h = height_of(&v, &narrow, at(TEXT - 50));
    assert!((0.0..600.0).contains(&h), "{h}");
}

#[test]
fn reflow_in_fit_page_shows_the_page_holding_it() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let defaults = Defaults {
        zoom: StartZoom::FitPage,
        continuous: false,
        ..Defaults::default()
    };
    let mut v = view_with(wide_sizes, defaults);
    v.go_to_page(5);
    assert_eq!(v.first_in_view(&wide).unwrap().0, at(5 * 1300));
    v.reflow(narrow_sizes, &wide, &narrow);
    // Character 6500 is on page 6 of 962 characters a page.
    assert_eq!(v.current_spread(), Some(6));
    let (now, _) = v.first_in_view(&narrow).unwrap();
    assert_eq!(narrow.page_char(now).unwrap().0, 6);
}

#[test]
fn reflow_in_dual_reads_left_before_right() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let defaults = Defaults {
        dual: true,
        ..Defaults::default()
    };
    let mut v = view_with(wide_sizes, defaults);
    let spread = v.layout().spread_pages(v.layout().spread_of(4));
    scroll_to(&mut v, spread.start, MARGIN_Y + 4.0 * LINE, 30.0);
    let (first, h) = v.first_in_view(&wide).unwrap();
    assert_eq!(wide.page_char(first).unwrap().0, spread.start);
    v.reflow(narrow_sizes, &wide, &narrow);
    // Its line is at the same height, on whichever page of the spread.
    let h_now = height_of(&v, &narrow, first);
    assert!((h_now - h).abs() < 0.5, "{h} → {h_now}");
}

#[test]
fn reflow_without_a_viewport_waits_for_one() {
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let mut v = View::new(wide_sizes);
    v.restore_place(&DocPoint {
        page: 5,
        x: 0.0,
        y: MARGIN_Y,
    });
    assert_eq!(v.first_in_view(&wide), None);
    v.reflow(narrow_sizes, &wide, &narrow);
    v.set_viewport_size(800.0, 600.0);
    let (now, h) = v.first_in_view(&narrow).unwrap();
    assert_eq!(now, line_start(&narrow, at(5 * 1300)));
    assert!(h.abs() < 0.5, "{h}");
}

#[test]
fn here_on_an_identity_map_is_the_centre_point() {
    let mut v = view(vec![
        Size {
            width: 612.0,
            height: 792.0
        };
        4
    ]);
    v.go_to_page(2);
    let identity = LocationMap::identity();
    assert_eq!(v.here(&identity), Some(Place::Point(v.place().unwrap())));
    assert_eq!(v.first_in_view(&identity), None);
}

#[test]
fn here_in_text_is_the_first_character_in_view() {
    let (sizes, map) = wide();
    let mut v = view(sizes);
    scroll_to(&mut v, 3, MARGIN_Y + 5.0 * LINE, 0.0);
    assert_eq!(v.here(&map), Some(Place::Text(at(3 * 1300 + 5 * 50))));
}

#[test]
fn going_to_text_puts_its_line_at_the_top() {
    let (sizes, map) = wide();
    let mut v = view(sizes);
    v.go_to_place(&Place::Text(at(4 * 1300 + 7 * 50 + 3)), &map);
    let (now, h) = v.first_in_view(&map).unwrap();
    assert_eq!(now, at(4 * 1300 + 7 * 50));
    assert!(h.abs() < 0.5, "{h}");
}

#[test]
fn here_on_a_page_without_text_is_its_point() {
    // A cover: a page with no text before the text.
    let (mut sizes, map) = wide();
    sizes.insert(0, sizes[0]);
    let mut pages: Vec<Vec<Run>> = vec![vec![]];
    pages.extend((0..sizes.len() - 1).map(|page| {
        (0..26)
            .filter_map(|line| {
                let start = at(page * 1300 + line * 50);
                map.line(start)
                    .map(|(_, run)| run)
                    .filter(|run| run.start == start)
            })
            .collect()
    }));
    let map = LocationMap::from_lines(pages);
    let v = view(sizes);
    assert_eq!(v.first_in_view(&map), None);
    assert_eq!(v.here(&map), Some(Place::Point(v.place().unwrap())));
}

#[test]
fn reflow_at_the_top_stays_at_the_top() {
    // Narrow pages fill the width at a larger scale: on wider ones the
    // first line sits higher than before, so its old height can't be kept.
    let ((wide_sizes, wide), (narrow_sizes, narrow)) = (wide(), narrow());
    let mut v = view(narrow_sizes);
    let (first, h) = v.first_in_view(&narrow).unwrap();
    assert_eq!(first, at(0));
    v.reflow(wide_sizes, &narrow, &wide);
    assert_eq!(v.offset().1, 0.0, "the page's head stays in view");
    let h_now = height_of(&v, &wide, first);
    assert!(h_now < h && h_now > 0.0, "{h} → {h_now}");
}
