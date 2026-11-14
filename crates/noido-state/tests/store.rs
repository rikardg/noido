use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use noido_core::{
    Caret, DocPoint, Highlight, HighlightColour, History, PageTransform, PageTransforms, Pins,
    Rotation, SidebarState, SidebarTab, ViewState, ZoomMode,
};
use noido_state::{DocKey, DocState, Frame, Pagelet, Slot, Store};
use tempfile::TempDir;

fn state(page: usize) -> DocState {
    DocState {
        view: ViewState {
            zoom: ZoomMode::Custom,
            scale: 1.5,
            continuous: false,
            dual: true,
            odd_left: true,
            gapless: false,
            centre: DocPoint {
                page,
                x: 10.0,
                y: 20.5,
            },
        },
        sidebar: SidebarState::default(),
        history: History::default(),
        highlights: Vec::new(),
        pins: Pins::default(),
        pagelet: None,
        transforms: PageTransforms::default(),
    }
}

/// A highlight from `start` to `end`, each a page and a char.
fn highlight(start: (usize, usize), end: (usize, usize), colour: HighlightColour) -> Highlight {
    Highlight {
        start: Caret {
            page: start.0,
            char: start.1,
        },
        end: Caret {
            page: end.0,
            char: end.1,
        },
        colour,
    }
}

/// A history entry: just a place on a page.
fn point(page: usize) -> DocPoint {
    DocPoint {
        page,
        x: 10.0,
        y: 20.5,
    }
}

/// The state `store` has for `key`.
fn loaded(store: &Store, key: &DocKey) -> Option<DocState> {
    store.load(key.clone()).1
}

/// The slot `store` gives `key`.
fn slot(store: &Store, key: &DocKey) -> Slot {
    store.load(key.clone()).0
}

fn docs(dir: &TempDir) -> PathBuf {
    dir.path().join("docs")
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn set_age(path: &Path, seconds_ago: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
        .unwrap();
}

#[test]
fn save_then_load() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    assert_eq!(loaded(&store, &key), Some(state(3)));
}

#[test]
fn gapless_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let mut doc = state(3);
    doc.view.gapless = true;
    store.save(&slot(&store, &key), &doc).unwrap();
    assert!(loaded(&store, &key).unwrap().view.gapless);
}

#[test]
fn entries_without_gapless_open_with_gaps() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(2)).unwrap();
    let path = docs(&dir).join(format!("{}.toml", key.hex()));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("gapless = false"), "{text}");
    let text = text.replace("gapless = false\n", "");
    fs::write(&path, text).unwrap();
    assert!(!loaded(&store, &key).unwrap().view.gapless);
}

#[test]
fn missing_entry_loads_none() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    assert_eq!(loaded(&store, &key), None);
}

#[test]
fn moved_file_found_by_hash() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let old = DocKey::new(b"pdf a", Path::new("/old/a.pdf"));
    store.save(&slot(&store, &old), &state(2)).unwrap();
    let moved = DocKey::new(b"pdf a", Path::new("/new/a.pdf"));
    assert_eq!(loaded(&store, &moved), Some(state(2)));
}

#[test]
fn path_fallback_refiles_state_and_keeps_window() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let before = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let slot_before = slot(&store, &before);
    store.save(&slot_before, &state(4)).unwrap();
    fs::write(store.window_file(&slot_before), "window").unwrap();

    let edited = DocKey::new(b"pdf a, edited", Path::new("/papers/a.pdf"));
    let (slot, restored) = store.load(edited.clone());
    assert_eq!(restored, Some(state(4)));
    let mut expected = vec![
        format!("{}.toml", edited.hex()),
        format!("{}.window.ron", before.hex()),
    ];
    expected.sort();
    assert_eq!(names(&docs(&dir)), expected);
    assert_eq!(
        fs::read_to_string(store.window_file(&slot)).unwrap(),
        "window"
    );
}

#[test]
fn garbage_and_unknown_version_ignored() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let b = DocKey::new(b"pdf b", Path::new("/papers/b.pdf"));
    fs::create_dir_all(docs(&dir)).unwrap();
    fs::write(docs(&dir).join(format!("{}.toml", a.hex())), "not toml {{").unwrap();
    store.save(&slot(&store, &b), &state(1)).unwrap();
    let path = docs(&dir).join(format!("{}.toml", b.hex()));
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("version = 1", "version = 2");
    fs::write(&path, text).unwrap();
    assert_eq!(loaded(&store, &a), None);
    assert_eq!(loaded(&store, &b), None);
}

#[test]
fn save_leaves_no_temp_file() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(0)).unwrap();
    store.save(&slot(&store, &key), &state(1)).unwrap();
    assert_eq!(names(&docs(&dir)), vec![format!("{}.toml", key.hex())]);
}

#[test]
fn prune_keeps_newest() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let keys: Vec<DocKey> = (0..5u8)
        .map(|i| DocKey::new(&[i], Path::new(&format!("/papers/{i}.pdf"))))
        .collect();
    for (i, key) in keys.iter().enumerate() {
        let slot = slot(&store, key);
        store.save(&slot, &state(i)).unwrap();
        fs::write(store.window_file(&slot), "w").unwrap();
        set_age(
            &docs(&dir).join(format!("{}.toml", key.hex())),
            100 - i as u64 * 10,
        );
    }
    store.prune(2);
    let mut expected = vec![];
    for key in &keys[3..] {
        expected.push(format!("{}.toml", key.hex()));
        expected.push(format!("{}.window.ron", key.hex()));
    }
    expected.sort();
    assert_eq!(names(&docs(&dir)), expected);
}

#[test]
fn new_document_window_copies_newest() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let x = DocKey::new(b"x", Path::new("/x.pdf"));
    let y = DocKey::new(b"y", Path::new("/y.pdf"));
    let z = DocKey::new(b"z", Path::new("/z.pdf"));
    fs::create_dir_all(docs(&dir)).unwrap();
    let x_window = docs(&dir).join(format!("{}.window.ron", x.hex()));
    let y_window = docs(&dir).join(format!("{}.window.ron", y.hex()));
    fs::write(&x_window, "x window").unwrap();
    fs::write(&y_window, "y window").unwrap();
    set_age(&x_window, 100);
    set_age(&y_window, 10);

    assert_eq!(
        fs::read_to_string(store.window_file(&slot(&store, &z))).unwrap(),
        "y window"
    );
    assert_eq!(store.window_file(&slot(&store, &y)), y_window);
    assert_eq!(fs::read_to_string(&y_window).unwrap(), "y window");
}

#[test]
fn sidebar_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        sidebar: SidebarState {
            open: true,
            width: 312.5,
            ..Default::default()
        },
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn sidebar_tab_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    // Results are never saved: the tab before them is.
    let saved = DocState {
        sidebar: SidebarState {
            tab: SidebarTab::Results,
            last: SidebarTab::Outline,
            ..Default::default()
        },
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    let sidebar = loaded(&store, &key).unwrap().sidebar;
    assert_eq!(sidebar.tab, SidebarTab::Outline);
    assert_eq!(sidebar.last, SidebarTab::Outline);
}

#[test]
fn old_files_and_unknown_tabs_give_thumbnails() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    fs::create_dir_all(docs(&dir)).unwrap();
    let file = docs(&dir).join(format!("{}.toml", key.hex()));
    let head = "version = 1\npath = \"/papers/a.pdf\"\nsaved = 0\nzoom = \"custom\"\n\
         scale = 1.5\ncontinuous = false\ndual = true\npage = 3\nx = 10.0\ny = 20.5\n";
    for extra in [
        "",
        "sidebar_tab = \"bookmarks\"\n",
        "sidebar_tab = \"results\"\n",
    ] {
        fs::write(&file, format!("{head}{extra}")).unwrap();
        let sidebar = loaded(&store, &key).unwrap().sidebar;
        assert_eq!(sidebar.tab, SidebarTab::Thumbnails, "{extra:?}");
        assert_eq!(sidebar.last, SidebarTab::Thumbnails, "{extra:?}");
    }
}

#[test]
fn pins_round_trip_and_stay_sorted() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        pins: Pins::from_sorted([9, 2, 5]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key).unwrap().pins.pages(), [2, 5, 9]);
}

#[test]
fn no_pins_write_no_key() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(!text.contains("pins"), "{text}");
}

#[test]
fn duplicate_pins_are_dropped_on_load() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let state = state(3);
    store.save(&slot(&store, &key), &state).unwrap();
    let file = docs(&dir).join(format!("{}.toml", key.hex()));
    fs::write(
        &file,
        format!("{}\npins = [7, 2, 7]\n", fs::read_to_string(&file).unwrap()),
    )
    .unwrap();
    assert_eq!(loaded(&store, &key).unwrap().pins.pages(), [2, 7]);
}

fn turned(rotation: Rotation) -> PageTransform {
    PageTransform {
        rotation,
        ..PageTransform::default()
    }
}

#[test]
fn rotations_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        transforms: PageTransforms::from_records([
            (3, turned(Rotation::R90)),
            (0, turned(Rotation::R270)),
            (8, turned(Rotation::R180)),
        ]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key).unwrap().transforms, saved.transforms);
}

#[test]
fn rotations_are_written_as_a_pages_table() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        transforms: PageTransforms::from_records([(3, turned(Rotation::R90))]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(text.contains("[[pages]]"), "{text}");
    assert!(text.contains("page = 3"), "{text}");
    assert!(text.contains("rotation = 90"), "{text}");
}

#[test]
fn no_rotations_write_no_pages_table() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(!text.contains("[[pages]]"), "{text}");
}

fn scaling_on() -> PageTransforms {
    let mut transforms = PageTransforms::from_records([(2, turned(Rotation::R90))]);
    transforms.set_scale_to_fit(true);
    transforms
}

fn written(dir: &TempDir, key: &DocKey) -> String {
    fs::read_to_string(docs(dir).join(format!("{}.toml", key.hex()))).unwrap()
}

#[test]
fn scale_to_fit_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        transforms: scaling_on(),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert!(written(&dir, &key).contains("scale_to_fit = true"));
    let transforms = loaded(&store, &key).unwrap().transforms;
    assert!(transforms.scale_to_fit());
    assert_eq!(transforms, saved.transforms);
}

#[test]
fn scale_to_fit_without_rotations_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let mut transforms = PageTransforms::default();
    transforms.set_scale_to_fit(true);
    let saved = DocState {
        transforms,
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    let text = written(&dir, &key);
    assert!(text.contains("scale_to_fit = true"), "{text}");
    assert!(!text.contains("[[pages]]"), "{text}");
    let transforms = loaded(&store, &key).unwrap().transforms;
    assert!(transforms.scale_to_fit());
    assert_eq!(transforms, saved.transforms);
}

#[test]
fn files_without_the_key_load_with_scale_to_fit_off() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    write_old_entry(&dir, &key);
    assert!(!loaded(&store, &key).unwrap().transforms.scale_to_fit());
}

#[test]
fn scale_to_fit_off_writes_no_key() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    assert!(!written(&dir, &key).contains("scale_to_fit"));
}

#[test]
fn scales_are_not_written() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let scaled = |rotation| PageTransform {
        rotation,
        scale: 0.5,
    };
    let saved = DocState {
        transforms: PageTransforms::from_records([
            (1, scaled(Rotation::R0)),
            (4, scaled(Rotation::R90)),
        ]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    let text = written(&dir, &key);
    assert_eq!(text.matches("[[pages]]").count(), 1, "{text}");
    assert!(text.contains("page = 4"), "{text}");
    assert!(!text.contains("scale = 0.5"), "{text}");
    assert_eq!(
        loaded(&store, &key).unwrap().transforms,
        PageTransforms::from_records([(4, turned(Rotation::R90))])
    );
}

#[test]
fn odd_angles_are_dropped_on_load() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    let file = docs(&dir).join(format!("{}.toml", key.hex()));
    fs::write(
        &file,
        format!(
            "{}\n[[pages]]\npage = 1\nrotation = 45\n\n[[pages]]\npage = 2\nrotation = 0\n\n[[pages]]\npage = 4\nrotation = 270\n",
            fs::read_to_string(&file).unwrap()
        ),
    )
    .unwrap();
    assert_eq!(
        loaded(&store, &key).unwrap().transforms,
        PageTransforms::from_records([(4, turned(Rotation::R270))])
    );
}

#[test]
fn files_without_pages_load_without_transforms() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    write_old_entry(&dir, &key);
    assert_eq!(
        loaded(&store, &key).unwrap().transforms,
        PageTransforms::default()
    );
}

#[test]
fn the_pagelets_place_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        pagelet: Some(Pagelet {
            x: 12.0,
            y: 34.0,
            width: 400.0,
            page: Some(4),
        }),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(
        loaded(&store, &key).unwrap().pagelet,
        Some(Pagelet {
            x: 12.0,
            y: 34.0,
            width: 400.0,
            page: Some(4),
        })
    );
}

#[test]
fn a_closed_pagelets_place_has_no_page() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        pagelet: Some(Pagelet {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            page: None,
        }),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    let pagelet = text.split("[pagelet]").nth(1).expect("a [pagelet] table");
    assert!(!pagelet.contains("page ="), "{pagelet}");
    assert_eq!(loaded(&store, &key).unwrap().pagelet.unwrap().page, None);
}

#[test]
fn no_pagelet_writes_no_key() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(!text.contains("pagelet"), "{text}");
}

#[test]
fn the_pins_tab_is_saved_and_restored() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let mut state = state(3);
    state.sidebar.choose(SidebarTab::Pins);
    store.save(&slot(&store, &key), &state).unwrap();
    let sidebar = loaded(&store, &key).unwrap().sidebar;
    assert_eq!(
        (sidebar.tab, sidebar.last),
        (SidebarTab::Pins, SidebarTab::Pins)
    );
}

#[test]
fn odd_pages_right_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let mut saved = state(3);
    saved.view.odd_left = false;
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn v1_file_without_sidebar_loads_defaults() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    fs::create_dir_all(docs(&dir)).unwrap();
    fs::write(
        docs(&dir).join(format!("{}.toml", key.hex())),
        "version = 1\npath = \"/papers/a.pdf\"\nsaved = 0\nzoom = \"custom\"\n\
         scale = 1.5\ncontinuous = false\ndual = true\npage = 3\nx = 10.0\ny = 20.5\n",
    )
    .unwrap();
    assert_eq!(loaded(&store, &key), Some(state(3)));
}

/// Writes a version 1 entry for `key` by hand, as noido wrote before
/// entries named their window file.
fn write_old_entry(dir: &TempDir, key: &DocKey) {
    fs::create_dir_all(docs(dir)).unwrap();
    fs::write(
        docs(dir).join(format!("{}.toml", key.hex())),
        "version = 1\npath = \"/papers/a.pdf\"\nsaved = 0\nzoom = \"custom\"\n\
         scale = 1.5\ncontinuous = false\ndual = true\npage = 3\nx = 10.0\ny = 20.5\n",
    )
    .unwrap();
}

#[test]
fn entry_without_window_uses_its_own_hash() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    write_old_entry(&dir, &key);
    let window = docs(&dir).join(format!("{}.window.ron", key.hex()));
    fs::write(&window, "window").unwrap();
    assert_eq!(store.window_file(&slot(&store, &key)), window);
}

#[test]
fn rekey_moves_state_keeps_window() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let b = DocKey::new(b"pdf a, recompiled", Path::new("/papers/a.pdf"));
    let mut slot = slot(&store, &a);
    store.save(&slot, &state(2)).unwrap();
    let window = store.window_file(&slot);
    fs::write(&window, "window").unwrap();

    store.rekey(&mut slot, b.clone());
    assert_eq!(slot.key().hex(), b.hex());
    assert_eq!(store.window_file(&slot), window);
    let (by_b, restored) = store.load(b);
    assert_eq!(restored, Some(state(2)));
    assert_eq!(store.window_file(&by_b), window);
    assert_eq!(fs::read_to_string(&window).unwrap(), "window");
    assert!(!docs(&dir).join(format!("{}.toml", a.hex())).exists());
}

#[test]
fn save_after_rekey_records_window() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let b = DocKey::new(b"pdf a, recompiled", Path::new("/papers/a.pdf"));
    let mut slot = slot(&store, &a);
    let window = store.window_file(&slot);
    store.rekey(&mut slot, b.clone());
    store.save(&slot, &state(5)).unwrap();
    let (by_b, restored) = store.load(b);
    assert_eq!(restored, Some(state(5)));
    assert_eq!(store.window_file(&by_b), window);
}

#[test]
fn rekey_without_state_file_is_fine() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let b = DocKey::new(b"pdf b", Path::new("/papers/a.pdf"));
    let mut slot = slot(&store, &a);
    store.rekey(&mut slot, b.clone());
    store.save(&slot, &state(1)).unwrap();
    assert_eq!(loaded(&store, &b), Some(state(1)));
}

#[test]
fn prune_removes_named_window_file() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let b = DocKey::new(b"pdf a, recompiled", Path::new("/papers/a.pdf"));
    let mut slot = slot(&store, &a);
    fs::write(store.window_file(&slot), "window").unwrap();
    store.rekey(&mut slot, b);
    store.save(&slot, &state(1)).unwrap();
    store.prune(0);
    assert_eq!(names(&docs(&dir)), Vec::<String>::new());
}

#[test]
fn history_is_saved_and_loaded() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        history: History::from_parts(vec![point(1), point(7)], vec![point(9)]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn an_old_history_entry_loads_as_its_point() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    fs::create_dir_all(docs(&dir)).unwrap();
    // A `[[back]]` written before history entries were just a point: the
    // extra view fields are ignored and only page/x/y are kept.
    fs::write(
        docs(&dir).join(format!("{}.toml", key.hex())),
        "version = 1\npath = \"/papers/a.pdf\"\nsaved = 0\nzoom = \"custom\"\n\
         scale = 1.5\ncontinuous = false\ndual = true\nodd_left = true\npage = 3\n\
         x = 10.0\ny = 20.5\nsidebar_open = true\nsidebar_width = 300.0\n\
         window = \"abc\"\n\
         [[back]]\nzoom = \"fit-page\"\nscale = 1.7843966\ncontinuous = true\n\
         dual = false\nodd_left = false\npage = 50\nx = 288.0\ny = 411.3435\n",
    )
    .unwrap();
    let expected = DocState {
        sidebar: SidebarState {
            open: true,
            width: 300.0,
            ..Default::default()
        },
        history: History::from_parts(
            vec![DocPoint {
                page: 50,
                x: 288.0,
                y: 411.3435,
            }],
            vec![],
        ),
        ..state(3)
    };
    assert_eq!(loaded(&store, &key), Some(expected));
}

#[test]
fn a_file_without_history_loads() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    fs::create_dir_all(docs(&dir)).unwrap();
    fs::write(
        docs(&dir).join(format!("{}.toml", key.hex())),
        "version = 1\npath = \"/papers/a.pdf\"\nsaved = 0\nzoom = \"custom\"\n\
         scale = 1.5\ncontinuous = false\ndual = true\nodd_left = true\npage = 3\n\
         x = 10.0\ny = 20.5\nsidebar_open = true\nsidebar_width = 300.0\n\
         window = \"abc\"\n",
    )
    .unwrap();
    let expected = DocState {
        sidebar: SidebarState {
            open: true,
            width: 300.0,
            ..Default::default()
        },
        ..state(3)
    };
    assert_eq!(loaded(&store, &key), Some(expected));
}

#[test]
fn highlights_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![
            highlight((0, 3), (0, 9), HighlightColour::Yellow),
            highlight((1, 0), (3, 5), HighlightColour::Green),
            highlight((2, 10), (2, 12), HighlightColour::Blue),
            highlight((4, 1), (4, 2), HighlightColour::Pink),
            highlight((5, 7), (6, 0), HighlightColour::Purple),
        ],
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));

    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    for colour in ["yellow", "green", "blue", "pink", "purple"] {
        assert!(text.contains(&format!("colour = \"{colour}\"")), "{text}");
    }
}

#[test]
fn entries_without_highlights_load_empty() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();

    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(!text.contains("[[highlights]]"), "{text}");
    let restored = loaded(&store, &key).unwrap();
    assert!(restored.highlights.is_empty());
}

#[test]
fn malformed_highlight_ranges_are_dropped_on_load() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((1, 0), (2, 4), HighlightColour::Blue)],
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();

    let file = docs(&dir).join(format!("{}.toml", key.hex()));
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str(
        "[[highlights]]\nstart_page = 2\nstart_char = 5\nend_page = 1\nend_char = 0\n\
         colour = \"yellow\"\n",
    );
    fs::write(&file, text).unwrap();

    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn open_documents_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(&dir.path().join("fresh"));
    let paths = vec![PathBuf::from("/b.pdf"), PathBuf::from("/a b/ä.pdf")];
    store.save_open(&paths).unwrap();
    assert_eq!(store.load_open(), paths);
    store.save_open(&[]).unwrap();
    assert_eq!(store.load_open(), Vec::<PathBuf>::new());
}

#[test]
fn no_open_list_is_empty() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    assert!(store.load_open().is_empty());
    fs::write(dir.path().join("open.toml"), "not [ toml").unwrap();
    assert!(store.load_open().is_empty());
}

#[test]
fn frame_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let slot = slot(&store, &key);
    assert_eq!(store.load_frame(&slot), None);
    let frame = Frame {
        x: 120.5,
        y: -30.0,
        width: 900.0,
        height: 1100.5,
    };
    store.save_frame(&slot, frame).unwrap();
    assert_eq!(store.load_frame(&slot), Some(frame));
    // Only the frame file is left behind: no temporary.
    assert_eq!(
        names(&docs(&dir)),
        vec![format!("{}.frame.toml", key.hex())]
    );
}

#[test]
fn a_new_document_takes_the_newest_size_only() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = slot(&store, &DocKey::new(b"a", Path::new("/a.pdf")));
    let b = slot(&store, &DocKey::new(b"b", Path::new("/b.pdf")));
    let c = slot(&store, &DocKey::new(b"c", Path::new("/c.pdf")));
    let frame = |x: f32, width: f32| Frame {
        x,
        y: x,
        width,
        height: width + 1.0,
    };
    store.save_frame(&a, frame(10.0, 500.0)).unwrap();
    store.save_frame(&b, frame(20.0, 600.0)).unwrap();
    for (slot, age) in [(&a, 100), (&b, 10)] {
        set_age(
            &docs(&dir).join(format!("{}.frame.toml", slot.key().hex())),
            age,
        );
    }
    let loaded = store.load_frame(&c).unwrap();
    assert_eq!((loaded.width, loaded.height), (600.0, 601.0));
    assert!(!loaded.has_position());
    // Looking does not give the document a frame of its own.
    assert!(
        !docs(&dir)
            .join(format!("{}.frame.toml", c.key().hex()))
            .exists()
    );
    assert!(store.load_frame(&b).unwrap().has_position());
}

#[test]
fn prune_removes_frames() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let b = DocKey::new(b"pdf a, recompiled", Path::new("/papers/a.pdf"));
    let mut slot = slot(&store, &a);
    let frame = Frame {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    };
    store.save_frame(&slot, frame).unwrap();
    // The frame stays with the document through a rekey.
    store.rekey(&mut slot, b);
    store.save(&slot, &state(1)).unwrap();
    assert_eq!(store.load_frame(&slot), Some(frame));
    store.prune(0);
    assert_eq!(names(&docs(&dir)), Vec::<String>::new());
}

/// A file on disk named `name`, by its canonical path, which is how the
/// recent list knows it.
fn file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, name).unwrap();
    fs::canonicalize(path).unwrap()
}

#[test]
fn recent_files_are_newest_first() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(&dir.path().join("fresh"));
    let a = file(&dir, "a.pdf");
    let b = file(&dir, "b.pdf");
    store.note_recent(&a).unwrap();
    store.note_recent(&b).unwrap();
    assert_eq!(store.load_recent(), vec![b, a]);
}

#[test]
fn noting_a_recent_file_again_moves_it_to_the_front() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = file(&dir, "a.pdf");
    let b = file(&dir, "b.pdf");
    let c = file(&dir, "c.pdf");
    for path in [&a, &b, &c, &a] {
        store.note_recent(path).unwrap();
    }
    assert_eq!(store.load_recent(), vec![a, c, b]);
}

#[test]
fn a_recent_file_is_known_by_its_canonical_path() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = file(&dir, "a.pdf");
    store.note_recent(&a).unwrap();
    store
        .note_recent(&dir.path().join(".").join("a.pdf"))
        .unwrap();
    assert_eq!(store.load_recent(), vec![a]);
}

#[test]
fn recent_files_stop_at_ten() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let files: Vec<PathBuf> = (0..12).map(|i| file(&dir, &format!("{i}.pdf"))).collect();
    for path in &files {
        store.note_recent(path).unwrap();
    }
    let recent = store.load_recent();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent.first(), files.last());
    assert!(!recent.contains(&files[0]));
}

#[test]
fn a_recent_file_that_is_gone_is_dropped() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = file(&dir, "a.pdf");
    let b = file(&dir, "b.pdf");
    store.note_recent(&a).unwrap();
    store.note_recent(&b).unwrap();
    fs::remove_file(&a).unwrap();
    assert_eq!(store.load_recent(), vec![b]);
}

#[test]
fn clearing_recent_files_empties_the_list() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let a = file(&dir, "a.pdf");
    store.note_recent(&a).unwrap();
    store.clear_recent().unwrap();
    assert!(store.load_recent().is_empty());
    // Noting one again starts a new list.
    store.note_recent(&a).unwrap();
    assert_eq!(store.load_recent(), vec![a]);
}

#[test]
fn no_recent_list_is_empty() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    assert!(store.load_recent().is_empty());
    fs::write(dir.path().join("recent.toml"), "not [ toml").unwrap();
    assert!(store.load_recent().is_empty());
}

#[test]
fn a_recent_list_is_written_atomically_and_names_its_version() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    store.note_recent(&file(&dir, "a.pdf")).unwrap();
    let text = fs::read_to_string(dir.path().join("recent.toml")).unwrap();
    assert!(text.contains("version = 1"), "{text}");
    assert_eq!(names(dir.path()), vec!["a.pdf", "recent.toml"]);
}

#[test]
fn a_hand_written_recent_list_stops_at_ten() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let mut list = String::from("version = 1\npaths = [\n");
    let files: Vec<PathBuf> = (0..12).map(|i| file(&dir, &format!("{i}.pdf"))).collect();
    for path in &files {
        list.push_str(&format!("  \"{}\",\n", path.display()));
    }
    list.push_str("]\n");
    fs::write(dir.path().join("recent.toml"), list).unwrap();
    let recent = store.load_recent();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent, files[..10].to_vec());
}
