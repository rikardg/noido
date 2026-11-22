use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use noido_core::{
    Caret, DocPoint, Highlight, HighlightColour, History, Location, PageTransform, PageTransforms,
    Pins, Place, Quote, Rotation, SidebarState, SidebarTab, ViewAnchor, ViewState, ZoomMode,
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
        anchor: None,
        transforms: PageTransforms::default(),
    }
}

/// A highlight from `start` to `end`, each a page and a char.
fn highlight(start: (usize, usize), end: (usize, usize), colour: HighlightColour) -> Highlight {
    Highlight::new(
        Caret {
            page: start.0,
            char: start.1,
        },
        Caret {
            page: end.0,
            char: end.1,
        },
        colour,
    )
}

/// A history entry: just a place on a page.
fn point(page: usize) -> Place {
    Place::Point(DocPoint {
        page,
        x: 10.0,
        y: 20.5,
    })
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
fn load_by_path_is_edited() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let before = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &before), &state(4)).unwrap();
    assert!(!slot(&store, &before).edited());
    let edited = DocKey::new(b"pdf a, edited", Path::new("/papers/a.pdf"));
    assert!(slot(&store, &edited).edited());
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
fn prune_removes_stale_temps() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(1)).unwrap();
    // What a save that failed mid-write, or was killed, leaves behind: the
    // entry's temp, the frame's, and the two lists'.
    let temps = [
        docs(&dir).join(format!("{}.tmp-999", key.hex())),
        docs(&dir).join(format!("{}.frame-tmp-999", key.hex())),
        dir.path().join("open.toml.tmp-999"),
        dir.path().join("recent.toml.tmp-999"),
    ];
    for temp in &temps {
        fs::write(temp, "half a state").unwrap();
        set_age(temp, 2 * 60 * 60);
    }
    store.prune(10);
    assert_eq!(names(&docs(&dir)), vec![format!("{}.toml", key.hex())]);
    for temp in &temps {
        assert!(!temp.exists(), "{} left behind", temp.display());
    }
}

#[test]
fn prune_keeps_a_temp_a_save_is_using() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(1)).unwrap();
    // Another instance may be writing this one now, so it is not swept.
    let temp = docs(&dir).join(format!("{}.tmp-999", key.hex()));
    fs::write(&temp, "half a state").unwrap();
    store.prune(10);
    assert!(temp.exists());
}

#[test]
fn prune_removes_orphan_window_files() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let kept = slot(&store, &DocKey::new(b"pdf a", Path::new("/papers/a.pdf")));
    store.save(&kept, &state(1)).unwrap();
    let window = store.window_file(&kept);
    fs::write(&window, "w").unwrap();
    // A window and a frame of a document killed before its first save, so
    // no entry names them.
    let orphan = "b".repeat(64);
    let orphan_window = docs(&dir).join(format!("{orphan}.window.ron"));
    let orphan_frame = docs(&dir).join(format!("{orphan}.frame.toml"));
    for path in [&orphan_window, &orphan_frame] {
        fs::write(path, "x").unwrap();
        set_age(path, 2 * 60 * 60);
    }
    store.prune(10);
    assert!(!orphan_window.exists());
    assert!(!orphan_frame.exists());
    assert!(window.exists(), "took a window the entry names");
}

#[test]
fn prune_keeps_a_fresh_orphan_window() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(1)).unwrap();
    // A document open for less than a save's delay: its window is in use.
    let orphan = "c".repeat(64);
    let orphan_window = docs(&dir).join(format!("{orphan}.window.ron"));
    fs::write(&orphan_window, "w").unwrap();
    store.prune(10);
    assert!(orphan_window.exists());
}

#[test]
fn prune_keeps_the_window_of_a_kept_entry_that_names_none() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(1)).unwrap();
    // An entry written before entries named their window: its hash is the
    // name.
    let state_file = docs(&dir).join(format!("{}.toml", key.hex()));
    let text = fs::read_to_string(&state_file).unwrap();
    let old = text
        .lines()
        .filter(|line| !line.starts_with("window = "))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&state_file, old).unwrap();
    assert_eq!(loaded(&store, &key), Some(state(1)));
    let window = docs(&dir).join(format!("{}.window.ron", key.hex()));
    fs::write(&window, "w").unwrap();
    set_age(&window, 2 * 60 * 60);
    store.prune(10);
    assert!(window.exists(), "took the window of a kept entry");
}

#[test]
fn prune_keeps_the_old_window_and_frame_of_a_kept_document() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let slot = slot(&store, &key);
    store.save(&slot, &state(1)).unwrap();
    store
        .save_frame(
            &slot,
            Frame {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            },
        )
        .unwrap();
    // A document not opened for hours still keeps where its window was.
    let window = docs(&dir).join(format!("{}.window.ron", key.hex()));
    let frame = docs(&dir).join(format!("{}.frame.toml", key.hex()));
    fs::write(&window, "w").unwrap();
    for path in [&window, &frame] {
        set_age(path, 2 * 60 * 60);
    }
    store.prune(10);
    assert!(window.exists(), "took the window of a kept document");
    assert!(frame.exists(), "took the frame of a kept document");
}

#[test]
fn prune_keeps_the_window_of_a_rekeyed_document() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let before = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let mut slot = slot(&store, &before);
    store.save(&slot, &state(1)).unwrap();
    store.window_file(&slot);
    let frame = Frame {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    };
    store.save_frame(&slot, frame).unwrap();
    // The file was edited in place: the state moves to the new hash and
    // names the old one's window, which stays where it is.
    let after = DocKey::new(b"pdf a, recompiled", Path::new("/papers/a.pdf"));
    store.rekey(&mut slot, after.clone());
    store.save(&slot, &state(1)).unwrap();
    // A newer document, so the entry that names the window is not the first
    // one read.
    let newer = DocKey::new(b"pdf b", Path::new("/papers/b.pdf"));
    let newer_slot = store.load(newer).0;
    store.save(&newer_slot, &state(2)).unwrap();
    set_age(&docs(&dir).join(format!("{}.toml", after.hex())), 60);
    let window = docs(&dir).join(format!("{}.window.ron", before.hex()));
    let frame = docs(&dir).join(format!("{}.frame.toml", before.hex()));
    for path in [&window, &frame] {
        fs::write(path, "x").unwrap();
        set_age(path, 2 * 60 * 60);
    }
    store.prune(10);
    assert!(window.exists(), "took the window the entry names");
    assert!(frame.exists(), "took the frame the entry names");
}

#[test]
fn a_failed_rename_takes_its_temp_file_with_it() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let slot = slot(&store, &key);
    // A directory where the entry belongs: the rename cannot land.
    fs::create_dir_all(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(store.save(&slot, &state(1)).is_err());
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
    assert_eq!(
        loaded(&store, &key).unwrap().pins,
        Pins::from_sorted([2, 5, 9])
    );
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
    assert_eq!(
        loaded(&store, &key).unwrap().pins,
        Pins::from_sorted([2, 7])
    );
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

/// Adds `rows` where a hand edit appends to the pages table: at the end of
/// the entry, which is where TOML puts an array of tables.
fn append(dir: &TempDir, key: &DocKey, rows: &str) {
    let file = docs(dir).join(format!("{}.toml", key.hex()));
    let mut text = fs::read_to_string(&file).unwrap();
    text.push('\n');
    text.push_str(rows);
    fs::write(&file, text).unwrap();
}

#[test]
fn an_angle_in_another_type_is_read() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    append(
        &dir,
        &key,
        "[[pages]]\npage = 1\nrotation = 90.0\n\n[[pages]]\npage = 2\nrotation = \"180\"\n\n\n[[pages]]\npage = 3\nrotation = 270\n",
    );
    assert_eq!(
        loaded(&store, &key).unwrap().transforms,
        PageTransforms::from_records([
            (1, turned(Rotation::R90)),
            (2, turned(Rotation::R180)),
            (3, turned(Rotation::R270)),
        ])
    );
}

#[test]
fn an_unreadable_angle_keeps_the_rest_of_the_entry() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((1, 3), (1, 9), HighlightColour::Yellow)],
        pins: Pins::from_sorted([2, 7]),
        history: History::from_parts(vec![point(1), point(7)], vec![point(9)]),
        pagelet: Some(Pagelet {
            x: 12.0,
            y: 34.0,
            width: 400.0,
            page: Some(point_on(4)),
        }),
        transforms: PageTransforms::from_records([(4, turned(Rotation::R270))]),
        ..state(5)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    append(
        &dir,
        &key,
        "[[pages]]\npage = 1\nrotation = \"east\"\n\n[[pages]]\npage = 2\nrotation = true\n\n[[pages]]\npage = 3\n\n[[pages]]\npage = 5\nrotation = 90.5\n",
    );
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn an_entry_broken_outside_the_pages_table_is_still_refused() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();
    // A page row is bad, and so is a top-level field: only page rows are
    // repairable, and a field that is still wrong ends the entry.
    let file = docs(&dir).join(format!("{}.toml", key.hex()));
    let text = fs::read_to_string(&file)
        .unwrap()
        .replace("scale = 1.5", "scale = \"big\"");
    fs::write(
        &file,
        format!("{text}\n[[pages]]\npage = \"x\"\nrotation = 90\n"),
    )
    .unwrap();
    assert_eq!(loaded(&store, &key), None);
}

#[test]
fn a_row_whose_page_is_unreadable_drops_only_itself() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((1, 3), (1, 9), HighlightColour::Yellow)],
        pins: Pins::from_sorted([2, 7]),
        transforms: PageTransforms::from_records([(6, turned(Rotation::R90))]),
        ..state(4)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    append(
        &dir,
        &key,
        "[[pages]]\npage = \"two\"\nrotation = 90\n\n[[pages]]\npage = -1\nrotation = 180\n",
    );
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn an_entry_whose_page_rows_all_fail_still_loads() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((2, 4), (2, 8), HighlightColour::Pink)],
        transforms: PageTransforms::from_records([(4, turned(Rotation::R180))]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    append(
        &dir,
        &key,
        "[[pages]]\npage = \"x\"\nrotation = 90\n\n[[pages]]\npage = 1\nrotation = \"east\"\n",
    );
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn a_negative_or_full_angle_is_dropped() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((1, 3), (1, 9), HighlightColour::Blue)],
        transforms: PageTransforms::from_records([(3, turned(Rotation::R90))]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    append(
        &dir,
        &key,
        "[[pages]]\npage = 1\nrotation = -90\n\n[[pages]]\npage = 2\nrotation = 360\n",
    );
    assert_eq!(loaded(&store, &key), Some(saved));
}

#[test]
fn an_angle_of_another_type_drops_only_its_row() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((1, 3), (1, 9), HighlightColour::Green)],
        transforms: PageTransforms::from_records([
            (7, turned(Rotation::R90)),
            (8, turned(Rotation::R270)),
            (10, turned(Rotation::R90)),
            (11, turned(Rotation::R270)),
        ]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    append(
        &dir,
        &key,
        "[[pages]]\npage = 1\nrotation = nan\n\n\
         [[pages]]\npage = 2\nrotation = inf\n\n\
         [[pages]]\npage = 3\nrotation = 65535\n\n\
         [[pages]]\npage = 4\nrotation = 1979-05-27T07:32:00Z\n\n\
         [[pages]]\npage = 5\nrotation = [90]\n\n\
         [[pages]]\npage = 6\nrotation = { degrees = 90 }\n\n\
         [[pages]]\npage = 9\nrotation = 99999999999999999999\n\n\
         [[pages]]\npage = 10\nrotation = \"+90\"\n\n\
         [[pages]]\npage = 11\nrotation = \" 270 \"\n",
    );
    assert_eq!(loaded(&store, &key), Some(saved));
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
            page: Some(point_on(4)),
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
            page: Some(point_on(4)),
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
            vec![Place::Point(DocPoint {
                page: 50,
                x: 288.0,
                y: 411.3435,
            })],
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
fn highlights_from_before_parts_still_load() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &state(3)).unwrap();

    let file = docs(&dir).join(format!("{}.toml", key.hex()));
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str(
        "[[highlights]]\nstart_page = 3\nstart_char = 120\nend_page = 3\nend_char = 164\n\
         colour = \"yellow\"\n",
    );
    fs::write(&file, text).unwrap();

    let restored = loaded(&store, &key).unwrap();
    assert_eq!(restored.highlights.len(), 1);
    let h = &restored.highlights[0];
    assert_eq!(h.start, Location { part: 3, char: 120 });
    assert_eq!(h.end, Location { part: 3, char: 164 });
    assert_eq!(h.quote, None);
    assert!(!h.orphaned);
}

#[test]
fn highlights_save_with_part_names() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let saved = DocState {
        highlights: vec![highlight((3, 120), (3, 164), HighlightColour::Yellow)],
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();

    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(text.contains("start_part = 3"), "{text}");
    assert!(text.contains("end_part = 3"), "{text}");
    for absent in [
        "start_page",
        "end_page",
        "quote",
        "prefix",
        "suffix",
        "orphaned",
    ] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
}

#[test]
fn quote_and_orphan_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    let mut h = highlight((1, 4), (1, 14), HighlightColour::Green);
    h.quote = Some(Quote {
        exact: "the nature".into(),
        prefix: "Liberty: ".into(),
        suffix: " of power".into(),
    });
    h.orphaned = true;
    let saved = DocState {
        highlights: vec![h],
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();

    let text = fs::read_to_string(docs(&dir).join(format!("{}.toml", key.hex()))).unwrap();
    assert!(text.contains("quote = \"the nature\""), "{text}");
    assert!(text.contains("orphaned = true"), "{text}");
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

/// A PDF's state with history, pins and an open pagelet, as saved.
fn pdf_state() -> DocState {
    DocState {
        history: History::from_parts(vec![point(1), point(7)], vec![point(9)]),
        pins: Pins::from_sorted([2, 7]),
        pagelet: Some(Pagelet {
            x: 300.0,
            y: 120.0,
            width: 360.0,
            page: Some(point_on(2)),
        }),
        ..state(3)
    }
}

/// A pin or the pagelet's page: a page's top left.
fn point_on(page: usize) -> Place {
    Place::Point(DocPoint {
        page,
        x: 0.0,
        y: 0.0,
    })
}

fn text(char: usize) -> Place {
    Place::Text(Location { part: 0, char })
}

/// The saved file's text without its `saved = …` time.
fn saved_text(store: &Store, key: &DocKey) -> String {
    fs::read_to_string(store.state_path(&slot(store, key)))
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with("saved = "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_pdf_state_file_is_written_as_before() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&slot(&store, &key), &pdf_state()).unwrap();
    let window = slot(&store, &key).key().hex();
    let expected = format!(
        "version = 1
path = \"/papers/a.pdf\"
zoom = \"custom\"
scale = 1.5
continuous = false
dual = true
odd_left = true
gapless = false
page = 3
x = 10.0
y = 20.5
sidebar_open = false
sidebar_width = 260.0
sidebar_tab = \"thumbnails\"
window = \"{window}\"
pins = [2, 7]
[[back]]
page = 1
x = 10.0
y = 20.5
[[back]]
page = 7
x = 10.0
y = 20.5
[[forward]]
page = 9
x = 10.0
y = 20.5
[pagelet]
x = 300.0
y = 120.0
width = 360.0
page = 2"
    );
    assert_eq!(saved_text(&store, &key), expected);
    assert_eq!(loaded(&store, &key), Some(pdf_state()));
}

#[test]
fn text_places_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"epub a", Path::new("/books/a.epub"));
    let saved = DocState {
        history: History::from_parts(vec![text(40), point(1)], vec![text(900)]),
        pins: Pins::from_places([text(1300), text(2600)]),
        pagelet: Some(Pagelet {
            x: 300.0,
            y: 120.0,
            width: 360.0,
            page: Some(text(1300)),
        }),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
    let file = saved_text(&store, &key);
    assert!(file.contains("part = 0\nchar = 40"), "{file}");
}

#[test]
fn a_view_anchor_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"epub a", Path::new("/books/a.epub"));
    let saved = DocState {
        anchor: Some(ViewAnchor {
            at: Location { part: 2, char: 640 },
            layout: Some("paperback serif 11".into()),
        }),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
    let file = saved_text(&store, &key);
    assert!(
        file.contains("anchor_part = 2\nanchor_char = 640\nlayout = \"paperback serif 11\""),
        "{file}"
    );
}

#[test]
fn a_pin_list_of_pages_and_text_round_trips() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"md a", Path::new("/notes/a.md"));
    // A page without text (an image) pinned as a page, beside text pins.
    let saved = DocState {
        pins: Pins::from_places([point_on(0), text(0), text(1300)]),
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
}
