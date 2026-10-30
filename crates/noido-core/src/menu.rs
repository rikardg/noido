//! The application menu, as data: frontends draw it.

use crate::Action;

/// Which state a menu entry's checkmark shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Continuous,
    Dual,
    OddLeft,
    Sidebar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: Action,
    pub label: &'static str,
    pub check: Option<Check>,
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
        ],
        vec![check(Action::ToggleSidebar, "Show sidebar", Check::Sidebar)],
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
        vec![entry(Action::Quit, "Quit")],
    ]
}

#[cfg(test)]
mod tests {
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
                vec!["Fit width", "Fit page", "Zoom in", "Zoom out"],
                vec!["Continuous", "Dual page", "Odd pages left"],
                vec!["Show sidebar"],
                vec!["Back", "Forward"],
                vec!["Copy link to page", "Copy Markdown link to page"],
                vec!["Next match", "Previous match"],
                vec!["Document info…", "Settings…"],
                vec!["Quit"],
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
            vec!["Continuous", "Dual page", "Odd pages left", "Show sidebar"]
        );
    }

    #[test]
    fn every_menu_action_has_a_key() {
        let keymap = Keymap::native();
        for entry in menu().iter().flatten() {
            assert!(keymap.key_for(entry.action).is_some(), "{}", entry.label);
        }
    }
}
