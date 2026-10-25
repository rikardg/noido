use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use noido_core::{DocPoint, ViewState, ZoomMode};
use noido_state::{DocKey, Store};
use tempfile::TempDir;

fn state(page: usize) -> ViewState {
    ViewState {
        zoom: ZoomMode::Custom,
        scale: 1.5,
        continuous: false,
        dual: true,
        centre: DocPoint {
            page,
            x: 10.0,
            y: 20.5,
        },
    }
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
    store.save(&key, &state(3)).unwrap();
    assert_eq!(store.load(&key), Some(state(3)));
}

#[test]
fn missing_entry_loads_none() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    assert_eq!(store.load(&key), None);
}

#[test]
fn moved_file_found_by_hash() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    store
        .save(&DocKey::new(b"pdf a", Path::new("/old/a.pdf")), &state(2))
        .unwrap();
    let moved = DocKey::new(b"pdf a", Path::new("/new/a.pdf"));
    assert_eq!(store.load(&moved), Some(state(2)));
}

#[test]
fn path_fallback_refiles_both_files() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let before = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&before, &state(4)).unwrap();
    fs::write(store.window_file(&before), "window").unwrap();

    let edited = DocKey::new(b"pdf a, edited", Path::new("/papers/a.pdf"));
    assert_eq!(store.load(&edited), Some(state(4)));
    assert_eq!(
        names(&docs(&dir)),
        vec![
            format!("{}.toml", edited.hex()),
            format!("{}.window.ron", edited.hex()),
        ]
    );
    assert_eq!(
        fs::read_to_string(store.window_file(&edited)).unwrap(),
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
    store.save(&b, &state(1)).unwrap();
    let path = docs(&dir).join(format!("{}.toml", b.hex()));
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("version = 1", "version = 2");
    fs::write(&path, text).unwrap();
    assert_eq!(store.load(&a), None);
    assert_eq!(store.load(&b), None);
}

#[test]
fn save_leaves_no_temp_file() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path());
    let key = DocKey::new(b"pdf a", Path::new("/papers/a.pdf"));
    store.save(&key, &state(0)).unwrap();
    store.save(&key, &state(1)).unwrap();
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
        store.save(key, &state(i)).unwrap();
        fs::write(store.window_file(key), "w").unwrap();
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
fn window_file_copies_newest() {
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
        fs::read_to_string(store.window_file(&z)).unwrap(),
        "y window"
    );
    assert_eq!(store.window_file(&y), y_window);
    assert_eq!(fs::read_to_string(&y_window).unwrap(), "y window");
}
