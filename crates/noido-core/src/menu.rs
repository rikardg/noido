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
    /// The file's name, or as much of it as tells it from the others.
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
            let folder = (counts[&name] > 1)
                .then(|| path.parent().and_then(Path::file_name))
                .flatten()
                .map(|folder| folder.to_string_lossy().into_owned());
            RecentEntry {
                path: path.clone(),
                label: label_of(&name, folder.as_deref()),
            }
        })
        .collect();
    // Two can still be labelled alike, one folder's name being another
    // file's whole path: those get their own path, whose ends say the
    // most.
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

/// The name, with the folder after it when two files are named alike.
fn label_of(name: &str, folder: Option<&str>) -> String {
    match folder {
        Some(folder) => format!("{name} — {folder}"),
        None => name.to_string(),
    }
}

/// `text` cut to at most `max`, as `width_of` measures it, with the middle
/// left out: as much of the start as fits, `…`, and the end, so a file
/// name keeps its extension. `text` unchanged when it fits already.
pub fn elide_to_width(text: &str, max: f32, width_of: &dyn Fn(&str) -> f32) -> String {
    /// A dot and a three-letter extension: what a cut name keeps of its
    /// end while it can.
    const EXTENSION_CHARS: usize = 4;
    if width_of(text) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    // At least the ellipsis, whatever the room.
    let mut best = "…".to_string();
    // A quarter of the room for the end: the start says more, the end has
    // only to carry the rest of the name and its extension — at least a
    // dot and a three-letter one, while there is room for it.
    let fits = |kept: usize| -> Option<String> {
        let tail = (kept / 4).max(EXTENSION_CHARS).min(kept - 1);
        let head = kept - tail;
        let start: String = chars[..head].iter().collect();
        let end: String = chars[chars.len() - tail..].iter().collect();
        let cut = format!("{start}…{end}");
        (width_of(&cut) <= max).then_some(cut)
    };
    // The most characters that fit, by halving: every extra character is
    // wider than the ellipsis it ends instead of.
    let (mut low, mut high) = (2, chars.len() - 1);
    while low <= high {
        let mid = (low + high) / 2;
        match fits(mid) {
            Some(cut) => {
                best = cut;
                low = mid + 1;
            }
            None => high = mid - 1,
        }
    }
    best
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

    /// The whole labels of [`recent`] for `paths`.
    fn labels(paths: &[&str]) -> Vec<String> {
        entries(paths)
            .into_iter()
            .map(|entry| entry.label)
            .collect()
    }

    fn entries(paths: &[&str]) -> Vec<RecentEntry> {
        recent(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())
    }

    /// Widths as a font would give them, at 5 points a character.
    fn five_each(text: &str) -> f32 {
        text.chars().count() as f32 * 5.0
    }

    #[test]
    fn text_that_fits_is_left_alone() {
        let text = "report.pdf";
        assert_eq!(elide_to_width(text, five_each(text), &five_each), text);
    }

    #[test]
    fn text_that_is_too_wide_is_cut_in_the_middle() {
        let text = format!("{}.pdf", "a-long-paper-name-".repeat(6));
        let max = 300.0;
        let cut = elide_to_width(&text, max, &five_each);
        assert!(five_each(&cut) <= max, "{cut} is too wide");
        assert!(cut.contains('…'), "{cut}");
        assert!(cut.starts_with("a-long-paper-name"), "{cut}");
        assert!(cut.ends_with(".pdf"), "{cut}");
    }

    #[test]
    fn a_cut_name_keeps_its_extension_while_there_is_room_for_it() {
        // Room for fourteen characters: nine of the start, the ellipsis,
        // and the four of ".pdf".
        let text = "a-very-long-paper-name-about-something.pdf";
        let cut = elide_to_width(text, 70.0, &five_each);
        assert!(cut.ends_with(".pdf"), "{cut}");
        assert!(five_each(&cut) <= 70.0, "{cut} is too wide");
    }

    #[test]
    fn text_cut_keeps_something_when_the_room_is_tiny() {
        let text = "a-very-long-paper-name-about-something.pdf";
        // Room for the ellipsis and a character or two.
        let cut = elide_to_width(text, 12.0, &five_each);
        assert!(five_each(&cut) <= 12.0, "{cut} is too wide");
        // No room at all still says it was cut.
        assert_eq!(elide_to_width(text, 1.0, &five_each), "…");
    }

    #[test]
    fn a_long_name_is_kept_whole() {
        let name = "x".repeat(120);
        let path = format!("/papers/{name}.pdf");
        assert_eq!(labels(&[&path])[0], format!("{name}.pdf"));
        assert!(!labels(&[&path])[0].contains('…'));
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
    fn a_file_named_like_a_neighbour_s_label_takes_its_whole_path() {
        // Two "a.pdf"s take their folders, and the third is named like
        // the first one's label.
        assert_eq!(
            labels(&["/x/a.pdf", "/y/a.pdf", "a.pdf — x"]),
            vec!["/x/a.pdf", "a.pdf — y", "a.pdf — x"]
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
                    label: "b.pdf".into(),
                },
                RecentEntry {
                    path: PathBuf::from("/a.pdf"),
                    label: "a.pdf".into(),
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
