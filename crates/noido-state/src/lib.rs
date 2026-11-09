//! Per-document view state, kept in the platform's directory for application
//! state. Each document has up to three files in `docs/`: `<hash>.toml` with
//! its [`DocState`], eframe's saved window geometry in `<name>.window.ron`,
//! and (on macOS, where noido keeps the windows itself) its [`Frame`] in
//! `<name>.frame.toml`.
//! The entry names its window file: eframe keeps writing to the file it was
//! given at launch, so the window file stays put when the document's hash
//! changes (a file edited in place, or reloaded while open).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use noido_core::{
    Caret, DocPoint, Highlight, HighlightColour, Highlights, History, Pins, SIDEBAR_DEFAULT_WIDTH,
    SidebarState, SidebarTab, ViewState, ZoomMode,
};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
const STATE_SUFFIX: &str = ".toml";
const OPEN_FILE: &str = "open.toml";
const RECENT_FILE: &str = "recent.toml";
/// How many recently opened files the list keeps.
pub const RECENT_LIMIT: usize = 10;
const WINDOW_SUFFIX: &str = ".window.ron";
const FRAME_SUFFIX: &str = ".frame.toml";

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

/// Where a document's window was, in points: its outer position and its
/// inner size. A frame that has no position (see [`Frame::has_position`])
/// only carries a size.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Frame {
    /// A size and no place: where a window opens is left to the system.
    pub fn sized(width: f32, height: f32) -> Self {
        Self {
            x: f32::NAN,
            y: f32::NAN,
            width,
            height,
        }
    }

    /// Whether the frame says where the window was, not only how big.
    pub fn has_position(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Whether the numbers can be used: a size that is positive and finite.
    pub fn has_size(&self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width > 0.0 && self.height > 0.0
    }
}

/// What is remembered about a document.
#[derive(Clone, Debug, PartialEq)]
pub struct DocState {
    pub view: ViewState,
    pub sidebar: SidebarState,
    pub history: History,
    pub highlights: Vec<Highlight>,
    pub pins: Pins,
}

/// `noido` under the `XDG_STATE_HOME` value if it is an absolute path;
/// unset, empty or relative values (which the XDG spec says to ignore)
/// give whatever `fallback` does.
fn state_dir(
    xdg: Option<std::ffi::OsString>,
    fallback: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match xdg.map(PathBuf::from) {
        Some(base) if base.is_absolute() => Some(base.join("noido")),
        _ => fallback(),
    }
}

/// The state directory and the documents' files in it.
#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    docs: PathBuf,
}

impl Store {
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            docs: dir.join("docs"),
        }
    }

    /// noido's state directory: `$XDG_STATE_HOME/noido` when that is set to
    /// an absolute path, on every platform; otherwise the platform's own
    /// (the local application data directory where there is no state one).
    pub fn open_default() -> Option<Self> {
        let dir = state_dir(std::env::var_os("XDG_STATE_HOME"), || {
            let dirs = ProjectDirs::from("", "", "noido")?;
            Some(
                dirs.state_dir()
                    .unwrap_or_else(|| dirs.data_local_dir())
                    .to_path_buf(),
            )
        })?;
        Some(Self::at(&dir))
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

    /// Writes the documents that are open, in order, for the next start to
    /// reopen; the file is replaced atomically. An empty list clears it.
    pub fn save_open(&self, paths: &[PathBuf]) -> io::Result<()> {
        self.save_list(OPEN_FILE, paths)
    }

    /// The documents [`save_open`](Self::save_open) wrote; none when the
    /// file is missing or unreadable.
    pub fn load_open(&self) -> Vec<PathBuf> {
        self.read_list(OPEN_FILE)
    }

    /// Puts `path` at the front of the recently opened files, out of
    /// wherever it was, and drops the oldest beyond [`RECENT_LIMIT`]; the
    /// file is replaced atomically. It is remembered by its canonical
    /// path, so a document reached two ways is one entry.
    pub fn note_recent(&self, path: &Path) -> io::Result<()> {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut paths = self.read_list(RECENT_FILE);
        paths.retain(|known| known != &path);
        paths.insert(0, path);
        paths.truncate(RECENT_LIMIT);
        self.save_list(RECENT_FILE, &paths)
    }

    /// The recently opened files, newest first, without the ones that have
    /// gone from disk; at most [`RECENT_LIMIT`], whatever the file says.
    /// The file itself catches up at the next write.
    pub fn load_recent(&self) -> Vec<PathBuf> {
        let mut recent: Vec<PathBuf> = self
            .read_list(RECENT_FILE)
            .into_iter()
            .filter(|path| path.exists())
            .collect();
        recent.truncate(RECENT_LIMIT);
        recent
    }

    /// Forgets every recently opened file.
    pub fn clear_recent(&self) -> io::Result<()> {
        self.save_list(RECENT_FILE, &[])
    }

    /// Writes `paths` to `name` in the state directory, one path a line
    /// of TOML; the file is replaced atomically. An empty list clears it.
    fn save_list(&self, name: &str, paths: &[PathBuf]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let list = PathList {
            version: VERSION,
            paths: paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        };
        let text = toml::to_string(&list).map_err(io::Error::other)?;
        let target = self.dir.join(name);
        let temp = self.dir.join(format!("{name}.tmp-{}", std::process::id()));
        fs::write(&temp, text)?;
        fs::rename(&temp, &target).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
    }

    /// The paths [`save_list`](Self::save_list) wrote to `name`; none when
    /// the file is missing, unreadable or another version's.
    fn read_list(&self, name: &str) -> Vec<PathBuf> {
        let Ok(text) = fs::read_to_string(self.dir.join(name)) else {
            return vec![];
        };
        match toml::from_str::<PathList>(&text) {
            Ok(list) if list.version == VERSION => {
                list.paths.into_iter().map(PathBuf::from).collect()
            }
            _ => vec![],
        }
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
            let window = window.as_deref().unwrap_or(&hex);
            let _ = fs::remove_file(self.window_path(window));
            let _ = fs::remove_file(self.frame_path(window));
        }
    }

    /// Writes the document's window frame; the file is replaced atomically.
    pub fn save_frame(&self, slot: &Slot, frame: Frame) -> io::Result<()> {
        fs::create_dir_all(&self.docs)?;
        let text = toml::to_string(&frame).map_err(io::Error::other)?;
        let target = self.frame_path(&slot.window);
        let temp = self
            .docs
            .join(format!("{}.frame-tmp-{}", slot.window, std::process::id()));
        fs::write(&temp, text)?;
        fs::rename(&temp, &target).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
    }

    /// The document's window frame. A document without one takes the size
    /// of the most recently written frame, with no position, so a new
    /// document opens as big as the last but not on top of it. None when
    /// there is no frame at all or it is unreadable.
    pub fn load_frame(&self, slot: &Slot) -> Option<Frame> {
        let read = |path: PathBuf| -> Option<Frame> {
            let frame: Frame = toml::from_str(&fs::read_to_string(path).ok()?).ok()?;
            frame.has_size().then_some(frame)
        };
        if let Some(frame) = read(self.frame_path(&slot.window)) {
            return Some(frame);
        }
        let mut frames = self.files_with(FRAME_SUFFIX);
        frames.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
        frames
            .into_iter()
            .find_map(|(window, _)| read(self.frame_path(&window)))
            .map(|f| Frame::sized(f.width, f.height))
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

    fn frame_path(&self, window: &str) -> PathBuf {
        self.docs.join(format!("{window}{FRAME_SUFFIX}"))
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

/// The TOML form of a list of paths: the open documents, or the recent
/// ones.
#[derive(Serialize, Deserialize)]
struct PathList {
    version: u32,
    paths: Vec<String>,
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
    /// "outline", "pins" or "thumbnails"; absent or unknown means
    /// thumbnails.
    #[serde(default)]
    sidebar_tab: String,
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
    // Absent from files written before highlights existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    highlights: Vec<StoredHighlight>,
    // Absent from files written before pins existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pins: Vec<usize>,
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
    #[serde(default)]
    gapless: bool,
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
            gapless: state.gapless,
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
            gapless: self.gapless,
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
            sidebar_tab: match doc.sidebar.last {
                SidebarTab::Outline => "outline",
                SidebarTab::Pins => "pins",
                _ => "thumbnails",
            }
            .to_owned(),
            window: Some(window.to_owned()),
            back: places(doc.history.back_entries()),
            forward: places(doc.history.forward_entries()),
            highlights: doc.highlights.iter().map(StoredHighlight::new).collect(),
            pins: doc.pins.pages().to_vec(),
        }
    }

    fn state(&self) -> DocState {
        let states = |points: &[Point]| points.iter().map(Point::state).collect();
        let tab = match self.sidebar_tab.as_str() {
            "outline" => SidebarTab::Outline,
            "pins" => SidebarTab::Pins,
            _ => SidebarTab::Thumbnails,
        };
        DocState {
            view: self.place.state(),
            sidebar: SidebarState {
                open: self.sidebar_open,
                width: self.sidebar_width,
                tab,
                last: tab,
            },
            history: History::from_parts(states(&self.back), states(&self.forward)),
            // Loading through `Highlights` drops the rows whose end is not
            // after their start, which noido never writes.
            highlights: Highlights::from_items(
                self.highlights
                    .iter()
                    .map(StoredHighlight::highlight)
                    .collect(),
            )
            .items()
            .to_vec(),
            pins: Pins::from_sorted(self.pins.iter().copied()),
        }
    }
}

/// The TOML form of a highlight.
#[derive(Serialize, Deserialize)]
struct StoredHighlight {
    start_page: usize,
    start_char: usize,
    end_page: usize,
    end_char: usize,
    colour: StoredColour,
}

impl StoredHighlight {
    fn new(highlight: &Highlight) -> Self {
        Self {
            start_page: highlight.start.page,
            start_char: highlight.start.char,
            end_page: highlight.end.page,
            end_char: highlight.end.char,
            colour: StoredColour::new(&highlight.colour),
        }
    }

    fn highlight(&self) -> Highlight {
        Highlight {
            start: Caret {
                page: self.start_page,
                char: self.start_char,
            },
            end: Caret {
                page: self.end_page,
                char: self.end_char,
            },
            colour: self.colour.colour(),
        }
    }
}

/// A [`HighlightColour`] as TOML writes it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredColour {
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl StoredColour {
    fn new(colour: &HighlightColour) -> Self {
        match colour {
            HighlightColour::Yellow => Self::Yellow,
            HighlightColour::Green => Self::Green,
            HighlightColour::Blue => Self::Blue,
            HighlightColour::Pink => Self::Pink,
            HighlightColour::Purple => Self::Purple,
        }
    }

    fn colour(&self) -> HighlightColour {
        match self {
            Self::Yellow => HighlightColour::Yellow,
            Self::Green => HighlightColour::Green,
            Self::Blue => HighlightColour::Blue,
            Self::Pink => HighlightColour::Pink,
            Self::Purple => HighlightColour::Purple,
        }
    }
}

#[cfg(test)]
mod dir_tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::state_dir;

    fn fallback() -> Option<PathBuf> {
        Some(PathBuf::from("/fallback"))
    }

    #[test]
    fn an_absolute_value_gives_noido_under_it() {
        let dir = state_dir(Some(OsString::from("/scratch/state")), fallback);
        assert_eq!(dir, Some(PathBuf::from("/scratch/state/noido")));
    }

    #[test]
    fn unset_empty_or_relative_values_use_the_fallback() {
        for value in [None, Some(""), Some("state"), Some("./state")] {
            let dir = state_dir(value.map(OsString::from), fallback);
            assert_eq!(dir, Some(PathBuf::from("/fallback")), "{value:?}");
        }
    }

    #[test]
    fn the_fallback_is_not_asked_for_when_the_value_is_used() {
        let dir = state_dir(Some(OsString::from("/s")), || panic!("asked"));
        assert_eq!(dir, Some(PathBuf::from("/s/noido")));
    }
}
