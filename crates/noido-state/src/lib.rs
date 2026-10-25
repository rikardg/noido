//! Per-document view state, kept in the platform's directory for application
//! state. Each document has up to two files in `docs/`: `<hash>.toml` with its
//! [`ViewState`] and `<hash>.window.ron`, eframe's saved window geometry.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use noido_core::{DocPoint, ViewState, ZoomMode};
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

    /// The document's saved state: by hash, else by path (a file edited in
    /// place), in which case its files are re-filed under the new hash.
    /// Missing, unreadable or unknown entries give `None`.
    pub fn load(&self, key: &DocKey) -> Option<ViewState> {
        if let Some(entry) = read_entry(&self.state_file(key)) {
            return Some(entry.state());
        }
        let (old_hex, entry) = self.find_by_path(&key.path)?;
        let new_hex = key.hex();
        // Best effort: the state is returned even if re-filing fails.
        let _ = fs::rename(
            self.docs.join(format!("{old_hex}{STATE_SUFFIX}")),
            self.docs.join(format!("{new_hex}{STATE_SUFFIX}")),
        );
        let old_window = self.docs.join(format!("{old_hex}{WINDOW_SUFFIX}"));
        if old_window.exists() {
            let _ = fs::rename(old_window, self.window_path(&new_hex));
        }
        Some(entry.state())
    }

    /// Writes the document's state; the file is replaced atomically.
    pub fn save(&self, key: &DocKey, state: &ViewState) -> io::Result<()> {
        fs::create_dir_all(&self.docs)?;
        let entry = Entry::new(&key.path, state);
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

    /// Deletes all but the `keep` most recently written documents' files.
    pub fn prune(&self, keep: usize) {
        let mut entries = self.files_with(STATE_SUFFIX);
        entries.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
        for (hex, _) in entries.into_iter().skip(keep) {
            let _ = fs::remove_file(self.docs.join(format!("{hex}{STATE_SUFFIX}")));
            let _ = fs::remove_file(self.window_path(&hex));
        }
    }

    /// The document's window file for eframe. A document without one starts
    /// from a copy of the most recently written one, so a new document opens
    /// where noido was last.
    pub fn window_file(&self, key: &DocKey) -> PathBuf {
        let hex = key.hex();
        let path = self.window_path(&hex);
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
    zoom: Zoom,
    scale: f32,
    continuous: bool,
    dual: bool,
    page: usize,
    x: f32,
    y: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Zoom {
    FitWidth,
    FitPage,
    Custom,
}

impl Entry {
    fn new(path: &Path, state: &ViewState) -> Self {
        Self {
            version: VERSION,
            path: path.to_string_lossy().into_owned(),
            saved: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            zoom: match state.zoom {
                ZoomMode::FitWidth => Zoom::FitWidth,
                ZoomMode::FitPage => Zoom::FitPage,
                ZoomMode::Custom => Zoom::Custom,
            },
            scale: state.scale,
            continuous: state.continuous,
            dual: state.dual,
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
            centre: DocPoint {
                page: self.page,
                x: self.x,
                y: self.y,
            },
        }
    }
}
