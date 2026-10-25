use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use noido_core::{DocPoint, SidebarState, ViewState, ZoomMode};
use noido_state::{DocKey, DocState, Slot, Store};
use tempfile::TempDir;

fn state(page: usize) -> DocState {
    DocState {
        view: ViewState {
            zoom: ZoomMode::Custom,
            scale: 1.5,
            continuous: false,
            dual: true,
            odd_left: true,
            centre: DocPoint {
                page,
                x: 10.0,
                y: 20.5,
            },
        },
        sidebar: SidebarState::default(),
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
        },
        ..state(3)
    };
    store.save(&slot(&store, &key), &saved).unwrap();
    assert_eq!(loaded(&store, &key), Some(saved));
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
