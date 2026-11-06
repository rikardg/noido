//! The application menu, as data: frontends draw it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Action;

/// Which state a menu entry's checkmark shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Continuous,
    Dual,
    OddLeft,
    Gapless,
    Sidebar,
    SelectTool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: Action,
    pub label: &'static str,
    pub check: Option<Check>,
}

/// A recently opened file, as a frontend offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub label: String,
}

/// The menu's entries for the recently opened `paths`, newest first. Each
/// is named by its file name; files that share a name also take their
/// folder's, and those whose folders share a name take their whole path.
pub fn recent(paths: &[PathBuf]) -> Vec<RecentEntry> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for path in paths {
        *counts.entry(file_name(path)).or_default() += 1;
    }
    let mut entries: Vec<RecentEntry> = paths
        .iter()
        .map(|path| {
            let name = file_name(path);
            let label = if counts[&name] > 1 {
                match path.parent().and_then(Path::file_name) {
                    Some(folder) => format!("{name} — {}", folder.to_string_lossy()),
                    None => path.display().to_string(),
                }
            } else {
                name
            };
            RecentEntry {
                path: path.clone(),
                label,
            }
        })
        .collect();
    // Two can still be labelled alike, one folder's name being another
    // file's whole path: those get their own path.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for entry in &entries {
        *seen.entry(entry.label.clone()).or_default() += 1;
    }
    for entry in &mut entries {
        if seen[&entry.label] > 1 {
            entry.label = entry.path.display().to_string();
        }
    }
    entries
}

/// The file's own name; the whole path for one that has no name of its
/// own.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The menu's sections, in order; frontends separate them.
pub fn menu() -> Vec<Vec<MenuEntry>> {
    let entry = |action, label| MenuEntry {
        action,
        label,
        check: None,
    };
    let check = |action, label, check| MenuEntry {
        action,
        label,
        check: Some(check),
    };
    vec![
        vec![entry(Action::OpenDocument, "Open…")],
        vec![
            entry(Action::FitWidth, "Fit width"),
            entry(Action::FitPage, "Fit page"),
            entry(Action::ZoomIn, "Zoom in"),
            entry(Action::ZoomOut, "Zoom out"),
        ],
        vec![
            check(Action::ToggleContinuous, "Continuous", Check::Continuous),
            check(Action::ToggleDual, "Dual page", Check::Dual),
            check(Action::ToggleOddLeft, "Odd pages left", Check::OddLeft),
            check(Action::ToggleGapless, "Gapless mode", Check::Gapless),
        ],
        vec![check(Action::ToggleSidebar, "Show sidebar", Check::Sidebar)],
        vec![check(
            Action::ToggleSelectTool,
            "Select text",
            Check::SelectTool,
        )],
        vec![
            entry(Action::Back, "Back"),
            entry(Action::Forward, "Forward"),
        ],
        vec![
            entry(Action::CopyLink, "Copy link to page"),
            entry(Action::CopyMarkdownLink, "Copy Markdown link to page"),
        ],
        vec![
            entry(Action::NextHit, "Next match"),
            entry(Action::PreviousHit, "Previous match"),
        ],
        vec![
            entry(Action::ShowDocumentInfo, "Document info…"),
            entry(Action::OpenSettings, "Settings…"),
        ],
        vec![
            entry(Action::CloseWindow, "Close window"),
            entry(Action::Quit, "Quit"),
        ],
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Keymap;

    #[test]
    fn menu_sections() {
        let menu = menu();
        let labels: Vec<Vec<&str>> = menu
            .iter()
            .map(|section| section.iter().map(|e| e.label).collect())
            .collect();
        assert_eq!(
            labels,
            vec![
                vec!["Open…"],
                vec!["Fit width", "Fit page", "Zoom in", "Zoom out"],
                vec!["Continuous", "Dual page", "Odd pages left", "Gapless mode"],
                vec!["Show sidebar"],
                vec!["Select text"],
                vec!["Back", "Forward"],
                vec!["Copy link to page", "Copy Markdown link to page"],
                vec!["Next match", "Previous match"],
                vec!["Document info…", "Settings…"],
                vec!["Close window", "Quit"],
            ]
        );
        let checked: Vec<&str> = menu
            .iter()
            .flatten()
            .filter(|e| e.check.is_some())
            .map(|e| e.label)
            .collect();
        assert_eq!(
            checked,
            vec![
                "Continuous",
                "Dual page",
                "Odd pages left",
                "Gapless mode",
                "Show sidebar",
                "Select text"
            ]
        );
    }

    /// The labels of [`recent`] for `paths`.
    fn labels(paths: &[&str]) -> Vec<String> {
        recent(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())
            .into_iter()
            .map(|entry| entry.label)
            .collect()
    }

    #[test]
    fn a_recent_file_is_named_by_its_file_name() {
        assert_eq!(labels(&["/papers/a.pdf"]), vec!["a.pdf"]);
    }

    #[test]
    fn files_that_share_a_name_take_their_folder_s_name() {
        assert_eq!(
            labels(&["/papers/a.pdf", "/scans/b.pdf", "/scans/a.pdf"]),
            vec!["a.pdf — papers", "b.pdf", "a.pdf — scans"]
        );
    }

    #[test]
    fn files_that_share_a_folder_s_name_take_their_whole_path() {
        assert_eq!(
            labels(&["/one/papers/a.pdf", "/two/papers/a.pdf"]),
            vec!["/one/papers/a.pdf", "/two/papers/a.pdf"]
        );
    }

    #[test]
    fn a_file_with_no_folder_is_named_alone_but_a_collision_with_it_is_not() {
        assert_eq!(labels(&["a.pdf"]), vec!["a.pdf"]);
        assert_eq!(
            labels(&["a.pdf", "/papers/a.pdf"]),
            vec!["a.pdf", "a.pdf — papers"]
        );
    }

    #[test]
    fn recents_keep_their_order_and_their_paths() {
        let entries = recent(&[PathBuf::from("/b.pdf"), PathBuf::from("/a.pdf")]);
        assert_eq!(
            entries,
            vec![
                RecentEntry {
                    path: PathBuf::from("/b.pdf"),
                    label: "b.pdf".into()
                },
                RecentEntry {
                    path: PathBuf::from("/a.pdf"),
                    label: "a.pdf".into()
                },
            ]
        );
    }

    #[test]
    fn no_recents_is_no_entries() {
        assert!(recent(&[]).is_empty());
    }

    #[test]
    fn every_menu_action_has_a_key() {
        let keymap = Keymap::native();
        for entry in menu().iter().flatten() {
            assert!(keymap.key_for(entry.action).is_some(), "{}", entry.label);
        }
    }
}
