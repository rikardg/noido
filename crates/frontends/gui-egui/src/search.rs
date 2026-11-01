//! Search state: the text thread, the hits of the current query, and when
//! to search and where to jump.

use eframe::egui::Color32;
use noido_core::Hits;
use noido_render::{SearchUpdate, TextSearcher};

/// Seconds after the last change to the field before it is searched.
pub const DELAY: f64 = 0.2;
/// The alpha of the fill over a match, and over the current one.
const HIGHLIGHT_ALPHA: u8 = 90;
const CURRENT_ALPHA: u8 = 140;

/// The fill over a match on the page, from the theme's warning colour.
pub fn highlight_fill(warning: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(warning.r(), warning.g(), warning.b(), HIGHLIGHT_ALPHA)
}

/// The fill over the current match, from the theme's warning colour.
pub fn current_fill(warning: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(warning.r(), warning.g(), warning.b(), CURRENT_ALPHA)
}

/// What to do with the first hit of a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum First {
    /// Go there: the reader just typed the query.
    Jump,
    /// Make it current without moving: the query was re-run on a reload.
    Select,
    /// Nothing: done, or the reader moved first.
    Done,
}

/// What the app should do with a hit that arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrived {
    /// It is current now: go there.
    Jump,
    /// It is current now; the view stays.
    Selected,
}

pub struct Search {
    searcher: Option<TextSearcher>,
    /// Whether extraction was asked for, so a reload asks the new thread too.
    extracting: bool,
    pub hits: Hits,
    /// The field's text as last seen, and when it last changed.
    text: String,
    changed_at: Option<f64>,
    /// The query being searched; `None` with no search.
    searched: Option<String>,
    generation: u64,
    /// Pages searched so far for this query.
    pages_searched: usize,
    /// Set when the search finished: whether it was cut short.
    done: Option<bool>,
    first: First,
    /// The view is where the current hit put it: next and previous go from
    /// that hit rather than from the view.
    pub at_hit: bool,
}

impl Search {
    pub fn new() -> Search {
        Search {
            searcher: None,
            extracting: false,
            hits: Hits::new(),
            text: String::new(),
            changed_at: None,
            searched: None,
            generation: 0,
            pages_searched: 0,
            done: None,
            first: First::Done,
            at_hit: false,
        }
    }

    /// Searches with `searcher` from now on, as after a reload.
    pub fn set_searcher(&mut self, searcher: TextSearcher) {
        if self.extracting {
            searcher.extract();
        }
        self.searcher = Some(searcher);
    }

    /// Starts extracting text, once.
    pub fn extract(&mut self) {
        if self.extracting {
            return;
        }
        self.extracting = true;
        if let Some(searcher) = &self.searcher {
            searcher.extract();
        }
    }

    /// Notes the field's `text` at time `now`. Returns the seconds until it
    /// should be searched, if a search is due: 0 or less means now.
    pub fn edited(&mut self, text: &str, now: f64) -> Option<f64> {
        if text != self.text {
            text.clone_into(&mut self.text);
            self.changed_at = Some(now);
        }
        if self.searched.as_deref() == Some(text) || (text.is_empty() && self.searched.is_none()) {
            self.changed_at = None;
            return None;
        }
        // An emptied field clears at once.
        if text.is_empty() {
            return Some(0.0);
        }
        self.changed_at.map(|at| at + DELAY - now)
    }

    /// Searches the field's text from `from_page`, or clears the search if
    /// it is empty; `first` says what to do with the first hit.
    pub fn start(&mut self, from_page: usize, first: First) {
        self.changed_at = None;
        self.hits.clear();
        self.pages_searched = 0;
        self.done = None;
        self.at_hit = false;
        // Blank text finds nothing; it is no search, not one that never ends.
        if self.text.trim().is_empty() {
            self.searched = None;
            self.first = First::Done;
            if let Some(searcher) = &mut self.searcher {
                self.generation = searcher.search("", 0);
            }
            return;
        }
        self.searched = Some(self.text.clone());
        self.first = first;
        if let Some(searcher) = &mut self.searcher {
            self.generation = searcher.search(&self.text, from_page);
        }
    }

    /// Clears the search and forgets the text, as Esc does.
    pub fn clear(&mut self) {
        self.text.clear();
        self.start(0, First::Done);
    }

    /// Re-runs the search, if any, with the text thread of a new version.
    pub fn rerun(&mut self, from_page: usize) {
        if self.searched.is_some() {
            self.start(from_page, First::Select);
        }
    }

    /// Takes in the text thread's results. Returns what to do about the
    /// first hit, when it arrives.
    pub fn receive(&mut self) -> Option<Arrived> {
        let searcher = self.searcher.as_ref()?;
        let mut arrived = None;
        while let Some(update) = searcher.try_recv() {
            match update {
                SearchUpdate::Page {
                    generation,
                    page,
                    hits,
                } if generation == self.generation && self.searched.is_some() => {
                    self.pages_searched += 1;
                    if hits.is_empty() {
                        continue;
                    }
                    self.hits.add_page(page, hits);
                    let Some(index) = self.hits.first_on(page) else {
                        continue;
                    };
                    // Made current at once: pages added later in this loop
                    // may come before it and move its index.
                    arrived = match std::mem::replace(&mut self.first, First::Done) {
                        First::Jump => Some(Arrived::Jump),
                        First::Select => Some(Arrived::Selected),
                        First::Done => continue,
                    };
                    self.hits.set_current(Some(index));
                }
                SearchUpdate::Done {
                    generation,
                    cut_short,
                } if generation == self.generation && self.searched.is_some() => {
                    self.done = Some(cut_short);
                }
                _ => {}
            }
        }
        arrived
    }

    /// The reader moved the page view: a jump still to come is dropped, and
    /// next and previous go from the view.
    pub fn reader_moved(&mut self) {
        if self.first == First::Jump {
            self.first = First::Done;
        }
        self.at_hit = false;
    }

    /// The match counter for the title bar.
    pub fn counter(&self) -> String {
        counter_text(
            self.searched.is_none(),
            self.hits.current(),
            self.hits.len(),
            self.done,
        )
    }

    /// The line above the results, for a document of `page_count` pages.
    pub fn status(&self, page_count: usize) -> String {
        status_text(
            self.searched.as_ref().map(|_| self.pages_searched),
            page_count,
            self.hits.len(),
            self.done,
        )
    }

    /// Tells one query's results from the next's.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The field's text as last seen.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn searched(&self) -> Option<&str> {
        self.searched.as_deref()
    }

    /// Whether the current query has been searched through.
    #[cfg(test)]
    pub fn finished(&self) -> bool {
        self.searched.is_some() && self.done.is_some()
    }

    #[cfg(test)]
    pub fn first(&self) -> First {
        self.first
    }
}

/// The match counter beside the search field: "3 of 41", "41 matches",
/// "No matches"; empty with no query (`blank`) or nothing found yet.
pub fn counter_text(blank: bool, current: Option<usize>, len: usize, done: Option<bool>) -> String {
    let more = if done == Some(true) { "+" } else { "" };
    match (blank, current, len) {
        (true, _, _) => String::new(),
        (false, Some(i), _) => format!("{} of {len}{more}", i + 1),
        (false, None, 0) if done.is_some() => "No matches".into(),
        (false, None, 0) => String::new(),
        (false, None, 1) => format!("1 match{more}"),
        (false, None, _) => format!("{len}{more} matches"),
    }
}

/// The line above the results: progress while searching, "No matches" when
/// done with none, else nothing. `searched` is `None` without a query.
pub fn status_text(
    searched: Option<usize>,
    page_count: usize,
    hits: usize,
    done: Option<bool>,
) -> String {
    match (searched, done) {
        (None, _) => String::new(),
        (Some(pages), None) => format!("Searching… page {pages} of {page_count}"),
        (Some(_), Some(_)) if hits == 0 => "No matches".into(),
        (Some(_), Some(_)) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_use_the_warning_colour() {
        let warning = Color32::from_rgb(0xe5, 0xa5, 0x0a);
        assert_eq!(
            highlight_fill(warning),
            Color32::from_rgba_unmultiplied(0xe5, 0xa5, 0x0a, 90)
        );
        assert_eq!(
            current_fill(warning),
            Color32::from_rgba_unmultiplied(0xe5, 0xa5, 0x0a, 140)
        );
    }

    #[test]
    fn counter_text_cases() {
        assert_eq!(counter_text(false, Some(2), 41, None), "3 of 41");
        assert_eq!(counter_text(false, None, 41, Some(false)), "41 matches");
        assert_eq!(counter_text(false, None, 1, Some(false)), "1 match");
        assert_eq!(counter_text(false, Some(2), 5000, Some(true)), "3 of 5000+");
        assert_eq!(counter_text(false, None, 5000, Some(true)), "5000+ matches");
        assert_eq!(counter_text(false, None, 0, Some(false)), "No matches");
        assert_eq!(counter_text(false, None, 0, None), "");
        assert_eq!(counter_text(true, None, 0, None), "");
    }

    #[test]
    fn status_text_cases() {
        assert_eq!(
            status_text(Some(12), 600, 4, None),
            "Searching… page 12 of 600"
        );
        assert_eq!(status_text(Some(600), 600, 0, Some(false)), "No matches");
        assert_eq!(status_text(Some(600), 600, 3, Some(false)), "");
        assert_eq!(status_text(None, 600, 0, None), "");
    }
}
