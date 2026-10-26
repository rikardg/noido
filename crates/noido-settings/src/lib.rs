//! noido's settings, in `settings.toml` in the platform's directory for
//! application config. For now: what a new document opens with.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use noido_core::{Defaults, StartZoom};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
const FILE_NAME: &str = "settings.toml";

/// Everything the reader can set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    /// The view a document with no saved state opens with.
    pub new_documents: Defaults,
}

/// The settings file.
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

    /// The platform's config directory for noido: `$XDG_CONFIG_HOME/noido`
    /// on Linux, the roaming application data directory on Windows,
    /// Application Support on macOS.
    pub fn open_default() -> Option<Self> {
        let dirs = ProjectDirs::from("", "", "noido")?;
        Some(Self::at(dirs.config_dir()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The settings; the defaults without a file. Missing fields take their
    /// defaults and odd values are mended; a file that isn't settings is an
    /// error, and is left alone.
    pub fn load(&self) -> io::Result<Settings> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(e),
        };
        let file: File = toml::from_str(&text).map_err(io::Error::other)?;
        if file.version != VERSION {
            return Err(io::Error::other(format!(
                "unknown settings version {}",
                file.version
            )));
        }
        Ok(Settings {
            new_documents: file.new_documents.defaults(),
        })
    }

    /// Writes the settings; the file is replaced atomically.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = File {
            version: VERSION,
            new_documents: NewDocuments::from(&settings.new_documents),
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

/// The TOML form of the settings.
#[derive(Serialize, Deserialize)]
struct File {
    #[serde(default = "version")]
    version: u32,
    #[serde(default)]
    new_documents: NewDocuments,
}

fn version() -> u32 {
    VERSION
}

/// The TOML form of [`Defaults`]. Loose where a hand edit may go wrong: an
/// unknown zoom or a percentage out of range is mended, not refused.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct NewDocuments {
    zoom: String,
    zoom_percent: i64,
    continuous: bool,
    dual: bool,
    odd_left: bool,
    sidebar: bool,
}

impl Default for NewDocuments {
    fn default() -> Self {
        Self::from(&Defaults::default())
    }
}

impl From<&Defaults> for NewDocuments {
    fn from(defaults: &Defaults) -> Self {
        let zoom = match defaults.zoom {
            StartZoom::FitWidth => "fit-width",
            StartZoom::FitPage => "fit-page",
            StartZoom::Fixed => "fixed",
        };
        Self {
            zoom: zoom.to_owned(),
            zoom_percent: defaults.zoom_percent.into(),
            continuous: defaults.continuous,
            dual: defaults.dual,
            odd_left: defaults.odd_left,
            sidebar: defaults.sidebar,
        }
    }
}

impl NewDocuments {
    fn defaults(&self) -> Defaults {
        let zoom = match self.zoom.as_str() {
            "fit-page" => StartZoom::FitPage,
            "fixed" => StartZoom::Fixed,
            _ => StartZoom::FitWidth,
        };
        Defaults {
            zoom,
            zoom_percent: self.zoom_percent.clamp(0, u32::MAX.into()) as u32,
            continuous: self.continuous,
            dual: self.dual,
            odd_left: self.odd_left,
            sidebar: self.sidebar,
        }
        .clamped()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use noido_core::{Defaults, StartZoom};

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
                sidebar: true,
            },
        };
        file(&dir).save(&settings).unwrap();
        assert_eq!(file(&dir).load().unwrap(), settings);
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
}
