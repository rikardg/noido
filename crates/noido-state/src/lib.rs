//! Per-document view state, kept in the platform's directory for application
//! state. Each document has up to two files in `docs/`: `<hash>.toml` with its
//! [`DocState`], and eframe's saved window geometry in `<name>.window.ron`.
//! The entry names its window file: eframe keeps writing to the file it was
//! given at launch, so the window file stays put when the document's hash
//! changes (a file edited in place, or reloaded while open).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use noido_core::{DocPoint, History, SIDEBAR_DEFAULT_WIDTH, SidebarState, ViewState, ZoomMode};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
const STATE_SUFFIX: &str = ".toml";
const WINDOW_SUFFIX: &str = ".window.ron";

/// Identifies a document: the hash of its content, and its canonical path for
/// finding the state of a file that was edited in place.
#[derive(Clone, Debug)]
pub struct DocKey {
    hash: blake3::Hash,
    path: PathBuf,
}

impl DocKey {
    pub fn new(bytes: &[u8], path: &Path) -> Self {
        Self {
            hash: blake3::hash(bytes),
            path: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        }
    }

    /// The hash as hex, which names the document's files.
    pub fn hex(&self) -> String {
        self.hash.to_hex().to_string()
    }
}

/// Where a document's state lives: its key, and the window file its entry
/// names (the hash the document had when the window file was made).
#[derive(Clone, Debug)]
pub struct Slot {
    key: DocKey,
    window: String,
}

impl Slot {
    pub fn key(&self) -> &DocKey {
        &self.key
    }
}

/// What is remembered about a document.
#[derive(Clone, Debug, PartialEq)]
pub struct DocState {
    pub view: ViewState,
    pub sidebar: SidebarState,
    pub history: History,
}

/// The state directory and the documents' files in it.
pub struct Store {
    docs: PathBuf,
}

impl Store {
    pub fn at(dir: &Path) -> Self {
        Self {
            docs: dir.join("docs"),
        }
    }

    /// The platform's state directory for noido: `$XDG_STATE_HOME/noido` on
    /// Linux, the local application data directory elsewhere.
    pub fn open_default() -> Option<Self> {
        let dirs = ProjectDirs::from("", "", "noido")?;
        let dir = dirs.state_dir().unwrap_or_else(|| dirs.data_local_dir());
        Some(Self::at(dir))
    }

    /// The document's slot and saved state: by hash, else by path (a file
    /// edited in place), in which case its state file is re-filed under the
    /// new hash. Missing, unreadable or unknown entries give no state.
    pub fn load(&self, key: DocKey) -> (Slot, Option<DocState>) {
        let found = match read_entry(&self.state_file(&key)) {
            Some(entry) => Some((key.hex(), entry)),
            None => self.find_by_path(&key.path).inspect(|(old_hex, _)| {
                // Best effort: the state is returned even if re-filing fails.
                let _ = fs::rename(
                    self.docs.join(format!("{old_hex}{STATE_SUFFIX}")),
                    self.state_file(&key),
                );
            }),
        };
        match found {
            Some((hex, entry)) => {
                let window = entry.window.clone().unwrap_or(hex);
                (Slot { key, window }, Some(entry.state()))
            }
            None => {
                let window = key.hex();
                (Slot { key, window }, None)
            }
        }
    }

    /// Writes the document's state; the file is replaced atomically.
    pub fn save(&self, slot: &Slot, state: &DocState) -> io::Result<()> {
        fs::create_dir_all(&self.docs)?;
        let key = &slot.key;
        let entry = Entry::new(&key.path, &slot.window, state);
        let text = toml::to_string(&entry).map_err(io::Error::other)?;
        let target = self.state_file(key);
        let temp = self
            .docs
            .join(format!("{}.tmp-{}", key.hex(), std::process::id()));
        fs::write(&temp, text)?;
        fs::rename(&temp, &target).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
    }

    /// Moves the slot's state to `key`, the document's new hash; the window
    /// file stays where it is. Best effort: a state file that can't be moved
    /// is written anew under `key` at the next save.
    pub fn rekey(&self, slot: &mut Slot, key: DocKey) {
        let _ = fs::rename(self.state_file(&slot.key), self.state_file(&key));
        slot.key = key;
    }

    /// Deletes all but the `keep` most recently written documents' files.
    pub fn prune(&self, keep: usize) {
        let mut entries = self.files_with(STATE_SUFFIX);
        entries.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
        for (hex, _) in entries.into_iter().skip(keep) {
            let state = self.docs.join(format!("{hex}{STATE_SUFFIX}"));
            let window = read_entry(&state).and_then(|entry| entry.window);
            let _ = fs::remove_file(&state);
            let _ = fs::remove_file(self.window_path(window.as_deref().unwrap_or(&hex)));
        }
    }

    /// The window file for eframe that the slot names. A document without
    /// one starts from a copy of the most recently written one, so a new
    /// document opens where noido was last.
    pub fn window_file(&self, slot: &Slot) -> PathBuf {
        let path = self.window_path(&slot.window);
        if !path.exists() {
            let _ = fs::create_dir_all(&self.docs);
            let newest = self
                .files_with(WINDOW_SUFFIX)
                .into_iter()
                .max_by_key(|(_, modified)| *modified);
            if let Some((newest, _)) = newest {
                let _ = fs::copy(self.window_path(&newest), &path);
            }
        }
        path
    }

    fn state_file(&self, key: &DocKey) -> PathBuf {
        self.docs.join(format!("{}{STATE_SUFFIX}", key.hex()))
    }

    fn window_path(&self, hex: &str) -> PathBuf {
        self.docs.join(format!("{hex}{WINDOW_SUFFIX}"))
    }

    /// The newest readable entry for `path`, with its hash.
    fn find_by_path(&self, path: &Path) -> Option<(String, Entry)> {
        let path = path.to_string_lossy();
        self.files_with(STATE_SUFFIX)
            .into_iter()
            .filter_map(|(hex, _)| {
                let entry = read_entry(&self.docs.join(format!("{hex}{STATE_SUFFIX}")))?;
                (entry.path == path).then_some((hex, entry))
            })
            .max_by_key(|(_, entry)| entry.saved)
    }

    /// Hashes and modification times of the files in `docs/` ending in `suffix`.
    fn files_with(&self, suffix: &str) -> Vec<(String, SystemTime)> {
        let Ok(dir) = fs::read_dir(&self.docs) else {
            return vec![];
        };
        dir.filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let hex = name.strip_suffix(suffix)?;
            // Skips stray files.
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((hex.to_owned(), modified))
        })
        .collect()
    }
}

fn read_entry(path: &Path) -> Option<Entry> {
    let text = fs::read_to_string(path).ok()?;
    let entry: Entry = toml::from_str(&text).ok()?;
    (entry.version == VERSION).then_some(entry)
}

/// The TOML form of a document's state.
#[derive(Serialize, Deserialize)]
struct Entry {
    version: u32,
    path: String,
    /// Seconds since the Unix epoch.
    saved: u64,
    #[serde(flatten)]
    place: Place,
    // Absent from files written before the sidebar existed.
    #[serde(default)]
    sidebar_open: bool,
    #[serde(default = "default_sidebar_width")]
    sidebar_width: f32,
    /// The name of the document's window file; absent from files written
    /// before entries named it, which use their own hash.
    #[serde(default)]
    window: Option<String>,
    // Absent from files written before history existed. Old entries carry
    // the view fields too; serde ignores them.
    #[serde(default)]
    back: Vec<Point>,
    #[serde(default)]
    forward: Vec<Point>,
}

/// The TOML form of the document's own [`ViewState`].
#[derive(Serialize, Deserialize)]
struct Place {
    zoom: Zoom,
    scale: f32,
    continuous: bool,
    dual: bool,
    // Absent from files written before the setting existed.
    #[serde(default = "yes")]
    odd_left: bool,
    page: usize,
    x: f32,
    y: f32,
}

fn yes() -> bool {
    true
}

fn default_sidebar_width() -> f32 {
    SIDEBAR_DEFAULT_WIDTH
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Zoom {
    FitWidth,
    FitPage,
    Custom,
}

impl Place {
    fn new(state: &ViewState) -> Self {
        Self {
            zoom: match state.zoom {
                ZoomMode::FitWidth => Zoom::FitWidth,
                ZoomMode::FitPage => Zoom::FitPage,
                ZoomMode::Custom => Zoom::Custom,
            },
            scale: state.scale,
            continuous: state.continuous,
            dual: state.dual,
            odd_left: state.odd_left,
            page: state.centre.page,
            x: state.centre.x,
            y: state.centre.y,
        }
    }

    fn state(&self) -> ViewState {
        ViewState {
            zoom: match self.zoom {
                Zoom::FitWidth => ZoomMode::FitWidth,
                Zoom::FitPage => ZoomMode::FitPage,
                Zoom::Custom => ZoomMode::Custom,
            },
            scale: self.scale,
            continuous: self.continuous,
            dual: self.dual,
            odd_left: self.odd_left,
            centre: DocPoint {
                page: self.page,
                x: self.x,
                y: self.y,
            },
        }
    }
}

/// The TOML form of a history entry: the place, and nothing else.
#[derive(Serialize, Deserialize)]
struct Point {
    page: usize,
    x: f32,
    y: f32,
}

impl Point {
    fn new(point: &DocPoint) -> Self {
        Self {
            page: point.page,
            x: point.x,
            y: point.y,
        }
    }

    fn state(&self) -> DocPoint {
        DocPoint {
            page: self.page,
            x: self.x,
            y: self.y,
        }
    }
}

impl Entry {
    fn new(path: &Path, window: &str, doc: &DocState) -> Self {
        let places = |points: &[DocPoint]| points.iter().map(Point::new).collect();
        Self {
            version: VERSION,
            path: path.to_string_lossy().into_owned(),
            saved: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            place: Place::new(&doc.view),
            sidebar_open: doc.sidebar.open,
            sidebar_width: doc.sidebar.width,
            window: Some(window.to_owned()),
            back: places(doc.history.back_entries()),
            forward: places(doc.history.forward_entries()),
        }
    }

    fn state(&self) -> DocState {
        let states = |points: &[Point]| points.iter().map(Point::state).collect();
        DocState {
            view: self.place.state(),
            sidebar: SidebarState {
                open: self.sidebar_open,
                width: self.sidebar_width,
            },
            history: History::from_parts(states(&self.back), states(&self.forward)),
        }
    }
}
