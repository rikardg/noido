//! noido's settings, in `settings.toml` in the platform's directory for
//! application config: what a new document opens with, and the paper
//! colour.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use noido_core::{Defaults, Paper, StartZoom};
use serde::Serialize;

const VERSION: u32 = 1;
const FILE_NAME: &str = "settings.toml";

/// Everything the reader can set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    /// The view a document with no saved state opens with.
    pub new_documents: Defaults,
    /// The colour pages are shown on, in every document.
    pub paper: Paper,
}

/// `noido` under the `XDG_CONFIG_HOME` value if it is an absolute path;
/// unset, empty or relative values (which the XDG spec says to ignore)
/// give whatever `fallback` does.
fn config_dir(
    xdg: Option<std::ffi::OsString>,
    fallback: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match xdg.map(PathBuf::from) {
        Some(base) if base.is_absolute() => Some(base.join("noido")),
        _ => fallback(),
    }
}

/// The settings file.
#[derive(Clone)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    /// `settings.toml` in `dir`.
    pub fn at(dir: &Path) -> Self {
        Self {
            path: dir.join(FILE_NAME),
        }
    }

    /// noido's config directory: `$XDG_CONFIG_HOME/noido` when that is set
    /// to an absolute path, on every platform; otherwise the platform's own
    /// (the roaming application data directory on Windows, Application
    /// Support on macOS).
    pub fn open_default() -> Option<Self> {
        let dir = config_dir(std::env::var_os("XDG_CONFIG_HOME"), || {
            let dirs = ProjectDirs::from("", "", "noido")?;
            Some(dirs.config_dir().to_path_buf())
        })?;
        Some(Self::at(&dir))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The settings; the defaults without a file. A missing field, or one
    /// holding a value of the wrong type, takes its default and costs only
    /// itself; a key so mended is named on stderr. A well-typed value noido
    /// doesn't know (an unknown zoom name, a percentage out of range) is
    /// mended silently. A file that isn't settings, or is another version's,
    /// is an error, and is left alone.
    pub fn load(&self) -> io::Result<Settings> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(e),
        };
        let mut mended = Vec::new();
        let settings = read(&text, &mut mended)?;
        for note in mended {
            eprintln!("noido: settings {}: {note}", self.path.display());
        }
        Ok(settings)
    }

    /// Writes the settings; the file is replaced atomically.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = File {
            version: VERSION,
            new_documents: NewDocuments::from(&settings.new_documents),
            paper: paper_name(settings.paper).to_owned(),
        };
        let text = toml::to_string(&file).map_err(io::Error::other)?;
        let temp = self
            .path
            .with_file_name(format!("{FILE_NAME}.tmp-{}", std::process::id()));
        fs::write(&temp, text)?;
        fs::rename(&temp, &self.path).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
    }
}

/// The settings `contents` holds. A key whose value can't be used takes its
/// default and is noted in `mended`; syntax and another version are errors.
fn read(contents: &str, mended: &mut Vec<String>) -> io::Result<Settings> {
    let table: toml::Table = toml::from_str(contents).map_err(io::Error::other)?;
    let version = match table.get("version") {
        None => VERSION,
        Some(toml::Value::Integer(version)) => u32::try_from(*version)
            .map_err(|_| io::Error::other(format!("unknown settings version {version}")))?,
        Some(other) => {
            return Err(io::Error::other(format!(
                "settings version must be a whole number, not {}",
                other.type_str()
            )));
        }
    };
    if version != VERSION {
        return Err(io::Error::other(format!(
            "unknown settings version {version}"
        )));
    }
    let new_documents = match table.get("new_documents") {
        None => Defaults::default(),
        Some(toml::Value::Table(fields)) => new_documents(fields, mended),
        Some(other) => {
            note(mended, "new_documents", other, "a table", "the defaults");
            Defaults::default()
        }
    };
    let paper = text(&table, mended, "paper", paper_name(Paper::default()))
        .map(paper_from_name)
        .unwrap_or_default();
    Ok(Settings {
        new_documents,
        paper,
    })
}

/// [`Defaults`] from the `[new_documents]` table: loose where a hand edit may
/// go wrong, so an unknown zoom or a percentage out of range is mended, and a
/// key of the wrong type costs only itself.
fn new_documents(table: &toml::Table, mended: &mut Vec<String>) -> Defaults {
    let builtin = Defaults::default();
    let zoom = match text(table, mended, "zoom", zoom_name(builtin.zoom)) {
        None => builtin.zoom,
        Some("fit-width") => StartZoom::FitWidth,
        Some("fit-page") => StartZoom::FitPage,
        Some("fixed") => StartZoom::Fixed,
        Some(_) => builtin.zoom,
    };
    Defaults {
        zoom,
        zoom_percent: integer(table, mended, "zoom_percent", builtin.zoom_percent.into())
            .clamp(0, u32::MAX.into()) as u32,
        continuous: boolean(table, mended, "continuous", builtin.continuous),
        dual: boolean(table, mended, "dual", builtin.dual),
        odd_left: boolean(table, mended, "odd_left", builtin.odd_left),
        gapless: boolean(table, mended, "gapless", builtin.gapless),
        sidebar: boolean(table, mended, "sidebar", builtin.sidebar),
    }
    .clamped()
}

/// What `table` holds under `key` as text, or `None` when it is missing or
/// something else (noted in `mended`, with `default` as the word for it).
fn text<'a>(
    table: &'a toml::Table,
    mended: &mut Vec<String>,
    key: &str,
    default: &str,
) -> Option<&'a str> {
    match table.get(key) {
        None => None,
        Some(toml::Value::String(text)) => Some(text),
        Some(other) => {
            note(mended, key, other, "text", default);
            None
        }
    }
}

/// What `table` holds under `key` as a boolean, or `default` when it is
/// missing or something else (noted in `mended`).
fn boolean(table: &toml::Table, mended: &mut Vec<String>, key: &str, default: bool) -> bool {
    match table.get(key) {
        None => default,
        Some(toml::Value::Boolean(value)) => *value,
        Some(other) => {
            note(mended, key, other, "a boolean", on_off(default));
            default
        }
    }
}

/// What `table` holds under `key` as a whole number, or `default` when it is
/// missing or something else (noted in `mended`).
fn integer(table: &toml::Table, mended: &mut Vec<String>, key: &str, default: i64) -> i64 {
    match table.get(key) {
        None => default,
        Some(toml::Value::Integer(value)) => *value,
        Some(other) => {
            note(mended, key, other, "a whole number", default);
            default
        }
    }
}

/// Notes that `key` holds `found` where it should hold `expected`, and that
/// `used` is taken instead.
fn note(
    mended: &mut Vec<String>,
    key: &str,
    found: &toml::Value,
    expected: &str,
    used: impl std::fmt::Display,
) {
    mended.push(format!(
        "{key} must be {expected}, not {}; using {used}",
        found.type_str()
    ));
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// The TOML form the settings are written in.
#[derive(Serialize)]
struct File {
    version: u32,
    new_documents: NewDocuments,
    paper: String,
}

fn paper_name(paper: Paper) -> &'static str {
    match paper {
        Paper::White => "white",
        Paper::Sepia => "sepia",
        Paper::Bisque => "bisque",
        Paper::Sand => "sand",
        Paper::Grey => "grey",
        Paper::DarkGrey => "dark-grey",
        Paper::DarkerGrey => "darker-grey",
    }
}

fn zoom_name(zoom: StartZoom) -> &'static str {
    match zoom {
        StartZoom::FitWidth => "fit-width",
        StartZoom::FitPage => "fit-page",
        StartZoom::Fixed => "fixed",
    }
}

/// Unknown or missing is white.
fn paper_from_name(name: &str) -> Paper {
    Paper::ALL
        .into_iter()
        .find(|&paper| paper_name(paper) == name)
        .unwrap_or_default()
}

/// The TOML form [`Defaults`] is written in.
#[derive(Serialize)]
struct NewDocuments {
    zoom: String,
    zoom_percent: i64,
    continuous: bool,
    dual: bool,
    odd_left: bool,
    gapless: bool,
    sidebar: bool,
}

impl From<&Defaults> for NewDocuments {
    fn from(defaults: &Defaults) -> Self {
        Self {
            zoom: zoom_name(defaults.zoom).to_owned(),
            zoom_percent: defaults.zoom_percent.into(),
            continuous: defaults.continuous,
            dual: defaults.dual,
            odd_left: defaults.odd_left,
            gapless: defaults.gapless,
            sidebar: defaults.sidebar,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use noido_core::{Defaults, Paper, StartZoom};

    use super::*;

    fn file(dir: &tempfile::TempDir) -> SettingsFile {
        SettingsFile::at(dir.path())
    }

    fn write(dir: &tempfile::TempDir, text: &str) {
        fs::write(dir.path().join("settings.toml"), text).unwrap();
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(file(&dir).load().unwrap(), Settings::default());
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            new_documents: Defaults {
                zoom: StartZoom::Fixed,
                zoom_percent: 150,
                continuous: false,
                dual: true,
                odd_left: false,
                gapless: true,
                sidebar: true,
            },
            paper: Paper::Sepia,
        };
        file(&dir).save(&settings).unwrap();
        assert_eq!(file(&dir).load().unwrap(), settings);
    }

    #[test]
    fn missing_gapless_is_off() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "version = 1\n[new_documents]\ndual = true\n");
        assert!(!file(&dir).load().unwrap().new_documents.gapless);
    }

    #[test]
    fn gapless_is_written_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            new_documents: Defaults {
                gapless: true,
                ..Defaults::default()
            },
            ..Settings::default()
        };
        file(&dir).save(&settings).unwrap();
        let text = fs::read_to_string(file(&dir).path()).unwrap();
        assert!(text.contains("gapless = true"), "{text}");
    }

    #[test]
    fn missing_fields_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "version = 1\n[new_documents]\ndual = true\n");
        let expected = Defaults {
            dual: true,
            ..Defaults::default()
        };
        assert_eq!(file(&dir).load().unwrap().new_documents, expected);
    }

    #[test]
    fn out_of_range_percentage_is_clamped() {
        let dir = tempfile::tempdir().unwrap();
        for (written, read) in [("0", 10), ("-5", 10), ("9999", 400)] {
            write(
                &dir,
                &format!("[new_documents]\nzoom_percent = {written}\n"),
            );
            let loaded = file(&dir).load().unwrap().new_documents;
            assert_eq!(loaded.zoom_percent, read, "{written}");
        }
    }

    #[test]
    fn unknown_zoom_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "[new_documents]\nzoom = \"huge\"\ndual = true\n");
        let loaded = file(&dir).load().unwrap().new_documents;
        assert_eq!(loaded.zoom, StartZoom::FitWidth);
        assert!(loaded.dual);
    }

    #[test]
    fn unreadable_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "not toml [");
        assert!(file(&dir).load().is_err());
        let text = fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert_eq!(text, "not toml [");
    }

    #[test]
    fn a_wrong_typed_percentage_costs_only_its_own_field() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "version = 1\n[new_documents]\nzoom = \"fixed\"\nzoom_percent = 150.0\ndual = true\n",
        );
        let loaded = file(&dir).load().unwrap().new_documents;
        assert_eq!(loaded.zoom, StartZoom::Fixed);
        assert_eq!(loaded.zoom_percent, Defaults::default().zoom_percent);
        assert!(loaded.dual);
    }

    #[test]
    fn a_wrong_typed_flag_costs_only_its_own_field() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "version = 1\npaper = \"sepia\"\n[new_documents]\ncontinuous = \"off\"\ngapless = true\n",
        );
        let loaded = file(&dir).load().unwrap();
        // The built-in default, not `bool::default()`.
        assert!(loaded.new_documents.continuous);
        assert!(loaded.new_documents.gapless);
        assert_eq!(loaded.paper, Paper::Sepia);
    }

    #[test]
    fn a_mistyped_field_is_rewritten_when_saved() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "version = 1\n[new_documents]\nzoom = \"fixed\"\nzoom_percent = 150\ngapless = true\nodd_left = \"yes\"\n",
        );
        let loaded = file(&dir).load().unwrap();
        // The bad key costs only itself: the rest of the file is kept, and
        // the fallback is the built-in default (`odd_left` is on), not zero.
        assert_eq!(loaded.new_documents.zoom, StartZoom::Fixed);
        assert_eq!(loaded.new_documents.zoom_percent, 150);
        assert!(loaded.new_documents.gapless);
        assert!(loaded.new_documents.odd_left);

        file(&dir).save(&loaded).unwrap();
        let text = fs::read_to_string(file(&dir).path()).unwrap();
        assert!(text.contains("zoom = \"fixed\""), "{text}");
        assert!(text.contains("zoom_percent = 150"), "{text}");
        assert!(text.contains("gapless = true"), "{text}");
        assert!(text.contains("odd_left = true"), "{text}");
        assert!(!text.contains("yes"), "{text}");
        assert_eq!(file(&dir).load().unwrap(), loaded);
    }

    #[test]
    fn new_documents_that_is_not_a_table_costs_only_the_table() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "version = 1\nnew_documents = \"x\"\npaper = \"sepia\"\n",
        );
        let loaded = file(&dir).load().unwrap();
        assert_eq!(loaded.new_documents, Defaults::default());
        assert_eq!(loaded.paper, Paper::Sepia);
    }

    #[test]
    fn a_wrong_typed_paper_is_white() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "version = 1\npaper = 3\n");
        assert_eq!(file(&dir).load().unwrap().paper, Paper::White);
    }

    #[test]
    fn a_wrong_typed_version_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "version = \"1\"\n");
        assert!(file(&dir).load().is_err());
    }

    #[test]
    fn another_version_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir, "version = 2\n");
        let error = file(&dir).load().unwrap_err();
        assert!(error.to_string().contains("version"), "{error}");
    }

    #[test]
    fn an_ignored_key_is_noted_with_the_value_used() {
        let mut mended = Vec::new();
        let settings = read(
            "version = 1\n[new_documents]\nzoom = 3\nzoom_percent = 150.0\n",
            &mut mended,
        )
        .unwrap();
        assert_eq!(mended.len(), 2, "{mended:?}");
        assert!(
            mended
                .iter()
                .any(|note| note.contains("zoom must be") && note.contains("fit-width")),
            "{mended:?}"
        );
        assert!(
            mended
                .iter()
                .any(|note| note.contains("zoom_percent must be") && note.contains("100")),
            "{mended:?}"
        );
        assert_eq!(settings.new_documents.zoom, StartZoom::FitWidth);
        assert_eq!(settings.new_documents.zoom_percent, 100);
    }

    #[test]
    fn every_ignored_key_is_noted_once() {
        let mut mended = Vec::new();
        read(
            "version = 1\npaper = 3\n[new_documents]\nzoom_percent = 150.0\ndual = \"yes\"\n",
            &mut mended,
        )
        .unwrap();
        assert_eq!(mended.len(), 3, "{mended:?}");
    }

    #[test]
    fn save_creates_directory_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("noido");
        SettingsFile::at(&nested)
            .save(&Settings::default())
            .unwrap();
        let names: Vec<_> = fs::read_dir(&nested)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["settings.toml"]);
    }

    #[test]
    fn file_names_its_version() {
        let dir = tempfile::tempdir().unwrap();
        file(&dir).save(&Settings::default()).unwrap();
        let text = fs::read_to_string(file(&dir).path()).unwrap();
        assert!(text.contains("version = 1"), "{text}");
        assert!(text.contains("zoom = \"fit-width\""), "{text}");
    }

    #[test]
    fn every_paper_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        for paper in Paper::ALL {
            let settings = Settings {
                paper,
                ..Settings::default()
            };
            file(&dir).save(&settings).unwrap();
            assert_eq!(file(&dir).load().unwrap().paper, paper);
        }
    }

    #[test]
    fn each_paper_is_written_by_its_stable_name() {
        let dir = tempfile::tempdir().unwrap();
        // These names are in users' files: renaming one resets that user's
        // paper to white, so each is pinned here, not derived from the enum.
        let names = [
            (Paper::White, "white"),
            (Paper::Sepia, "sepia"),
            (Paper::Bisque, "bisque"),
            (Paper::Sand, "sand"),
            (Paper::Grey, "grey"),
            (Paper::DarkGrey, "dark-grey"),
            (Paper::DarkerGrey, "darker-grey"),
        ];
        for (paper, name) in names {
            let settings = Settings {
                paper,
                ..Settings::default()
            };
            file(&dir).save(&settings).unwrap();
            let text = fs::read_to_string(file(&dir).path()).unwrap();
            assert!(
                text.contains(&format!("paper = \"{name}\"")),
                "{paper:?}: {text}"
            );
        }
    }

    #[test]
    fn missing_or_unknown_paper_is_white() {
        let dir = tempfile::tempdir().unwrap();
        for text in ["version = 1\n", "paper = \"purple\"\n"] {
            write(&dir, text);
            assert_eq!(file(&dir).load().unwrap().paper, Paper::White, "{text}");
        }
    }
}

#[cfg(test)]
mod dir_tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::config_dir;

    fn fallback() -> Option<PathBuf> {
        Some(PathBuf::from("/fallback"))
    }

    #[test]
    fn an_absolute_value_gives_noido_under_it() {
        let dir = config_dir(Some(OsString::from("/scratch/config")), fallback);
        assert_eq!(dir, Some(PathBuf::from("/scratch/config/noido")));
    }

    #[test]
    fn unset_empty_or_relative_values_use_the_fallback() {
        for value in [None, Some(""), Some("config"), Some("./config")] {
            let dir = config_dir(value.map(OsString::from), fallback);
            assert_eq!(dir, Some(PathBuf::from("/fallback")), "{value:?}");
        }
    }

    #[test]
    fn the_fallback_is_not_asked_for_when_the_value_is_used() {
        let dir = config_dir(Some(OsString::from("/c")), || panic!("asked"));
        assert_eq!(dir, Some(PathBuf::from("/c/noido")));
    }
}
