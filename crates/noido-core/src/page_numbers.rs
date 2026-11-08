//! Page numbers for display: the label a page declares for itself, usually
//! the number printed on it, and its number in the file. Parentheses always
//! mean a file number; a label never has them. A document with no labels
//! has only one kind of number and needs no parentheses.

/// How long a label gets in the title bar, in characters.
const TITLE_BAR_LABEL: usize = 24;

/// The labels of a document's pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageNumbers {
    /// Per page; empty when no page has a label.
    labels: Vec<Option<String>>,
}

/// A page's number in two parts: the label may be elided, the file part not.
#[derive(Clone, Debug, PartialEq)]
pub struct PageNumber<'a> {
    pub label: Option<&'a str>,
    /// "(79)" beside a differing label or alone where a page has none, "12"
    /// when the document has no labels, `None` when the label equals it.
    pub file: Option<String>,
}

impl PageNumber<'_> {
    /// The parts, joined by a space.
    pub fn text(&self) -> String {
        match (self.label, &self.file) {
            (Some(label), Some(file)) => format!("{label} {file}"),
            (Some(label), None) => label.to_owned(),
            (None, Some(file)) => file.clone(),
            (None, None) => String::new(),
        }
    }
}

impl PageNumbers {
    pub fn new(labels: Vec<Option<String>>) -> Self {
        if labels.iter().all(Option::is_none) {
            return Self::default();
        }
        Self { labels }
    }

    fn label(&self, page: usize) -> Option<&str> {
        self.labels.get(page)?.as_deref()
    }

    /// Whether `label` says the same as the file number of `page`.
    fn is_file_number(label: &str, page: usize) -> bool {
        label == (page + 1).to_string()
    }

    /// Both numbers, for the Pages tab and search cards.
    pub fn both(&self, page: usize) -> PageNumber<'_> {
        let number = page + 1;
        match self.label(page) {
            Some(label) if Self::is_file_number(label, page) => PageNumber {
                label: Some(label),
                file: None,
            },
            Some(label) => PageNumber {
                label: Some(label),
                file: Some(format!("({number})")),
            },
            None if self.labels.is_empty() => PageNumber {
                label: None,
                file: Some(number.to_string()),
            },
            None => PageNumber {
                label: None,
                file: Some(format!("({number})")),
            },
        }
    }

    /// One number, for the outline, in parts: the label alone if there is
    /// one, so it may be elided, else the file number.
    pub fn short_number(&self, page: usize) -> PageNumber<'_> {
        match self.both(page) {
            PageNumber {
                label: Some(label), ..
            } => PageNumber {
                label: Some(label),
                file: None,
            },
            number => number,
        }
    }

    /// One number, for the outline: the label if there is one.
    pub fn short(&self, page: usize) -> String {
        self.short_number(page).text()
    }

    /// The title bar's position: "61 (79 / 300)".
    pub fn title_bar(&self, page: usize, count: usize) -> String {
        let number = page + 1;
        match self.label(page) {
            Some(label) if Self::is_file_number(label, page) => format!("{label} / {count}"),
            Some(label) => format!("{} ({number} / {count})", elided(label)),
            None if self.labels.is_empty() => format!("{number} / {count}"),
            None => format!("({number} / {count})"),
        }
    }
}

/// `label`, cut with an ellipsis if longer than the title bar allows.
fn elided(label: &str) -> String {
    if label.chars().count() <= TITLE_BAR_LABEL {
        return label.to_owned();
    }
    let kept: String = label.chars().take(TITLE_BAR_LABEL - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled(labels: &[(usize, &str)], count: usize) -> PageNumbers {
        let mut all = vec![None; count];
        for (page, label) in labels {
            all[*page] = Some((*label).to_owned());
        }
        PageNumbers::new(all)
    }

    fn book() -> PageNumbers {
        labelled(&[(78, "61"), (4, "5")], 300)
    }

    #[test]
    fn label_differing_from_the_file_number() {
        let n = book();
        assert_eq!(n.both(78).text(), "61 (79)");
        assert_eq!(n.short(78), "61");
        assert_eq!(n.title_bar(78, 300), "61 (79 / 300)");
    }

    #[test]
    fn label_equal_to_the_file_number() {
        let n = book();
        assert_eq!(n.both(4).text(), "5");
        assert_eq!(n.short(4), "5");
        assert_eq!(n.title_bar(4, 300), "5 / 300");
    }

    #[test]
    fn page_without_a_label_where_others_have_one() {
        let n = book();
        assert_eq!(n.both(11).text(), "(12)");
        assert_eq!(n.short(11), "(12)");
        assert_eq!(n.title_bar(11, 300), "(12 / 300)");
    }

    #[test]
    fn document_with_no_labels() {
        let n = PageNumbers::default();
        assert_eq!(n.both(11).text(), "12");
        assert_eq!(n.short(11), "12");
        assert_eq!(n.title_bar(11, 300), "12 / 300");
    }

    #[test]
    fn all_none_behaves_as_no_labels() {
        assert_eq!(PageNumbers::new(vec![None; 3]).short(1), "2");
    }

    #[test]
    fn past_the_end_has_no_label() {
        assert_eq!(book().short(400), "(401)");
    }

    #[test]
    fn both_splits_label_from_file_number() {
        let n = book();
        assert_eq!(
            n.both(78),
            PageNumber {
                label: Some("61"),
                file: Some("(79)".to_owned()),
            }
        );
        assert_eq!(
            n.both(4),
            PageNumber {
                label: Some("5"),
                file: None,
            }
        );
    }

    #[test]
    fn title_bar_elides_a_long_label() {
        let long = labelled(&[(0, "abcdefghijklmnopqrstuvwxyz")], 3);
        assert_eq!(long.title_bar(0, 3), "abcdefghijklmnopqrstuvw… (1 / 3)");
        assert_eq!(long.both(0).label, Some("abcdefghijklmnopqrstuvwxyz"));
    }
}
