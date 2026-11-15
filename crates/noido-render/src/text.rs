//! The text thread: extracts page text on its own copy of the document,
//! keeps it, and searches it, so the window never waits for either.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use noido_core::PageTransform;
use noido_engine::{Document, PdfBytes, PdfEngine};

use crate::{Transformed, Transforms, open_transformed};
use noido_text::{PageText, Query, TextHit};

/// The most hits a search finds; past that it stops. One letter on a long
/// book would otherwise give a million.
pub const MAX_HITS: usize = 5000;

/// What the text thread reports about a search.
#[derive(Debug)]
pub enum SearchUpdate {
    /// One page searched, with its hits (possibly none).
    Page {
        generation: u64,
        page: usize,
        hits: Vec<TextHit>,
    },
    /// Every page searched, or `cut_short` at [`MAX_HITS`].
    Done { generation: u64, cut_short: bool },
    /// A page's text, as asked for with [`TextSearcher::request_text`],
    /// and the transform the page had when it was read.
    Text {
        page: usize,
        text: Arc<PageText>,
        transform: PageTransform,
    },
    /// The document's counts, once every page is counted.
    Counts(Counted),
}

/// What the text thread has counted: the pages that went into the totals,
/// and how many pages there are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    pub pages: usize,
    pub counted: usize,
    pub total: noido_text::Counts,
}

impl Counted {
    /// Whether every page has been counted.
    pub fn done(&self) -> bool {
        self.counted >= self.pages
    }
}

enum Command {
    Search {
        generation: u64,
        query: Query,
        from_page: usize,
    },
    Cancel,
    Text(usize),
    Forget(usize),
    Count,
}

/// Handle to the text thread. Dropping it stops the thread after its
/// current page.
pub struct TextSearcher {
    commands: Sender<Command>,
    results: Receiver<SearchUpdate>,
    generation: u64,
}

impl TextSearcher {
    /// Starts the thread; returns at once. It opens the document only when
    /// first asked for something.
    pub fn start(
        engine: Arc<dyn PdfEngine>,
        bytes: PdfBytes,
        transforms: Transforms,
        waker: Box<dyn Fn() + Send>,
    ) -> TextSearcher {
        let (commands, command_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        // Without a thread nothing is found, as when opening fails.
        let _ = thread::Builder::new()
            .name("noido-text".into())
            .spawn(move || {
                // Nothing to do until the first command.
                let Ok(first) = command_rx.recv() else {
                    return;
                };
                match open_transformed(engine.as_ref(), bytes, &transforms) {
                    Ok(document) => {
                        Worker::new(&document, &result_tx, &*waker).run(first, &command_rx)
                    }
                    Err(e) => eprintln!("noido: text: {e}"),
                }
            });
        TextSearcher {
            commands,
            results,
            generation: 0,
        }
    }

    /// Searches every page for `query`, starting at `from_page` and
    /// wrapping around, replacing any running search. Returns the search's
    /// generation, which its updates carry. A blank query cancels and finds
    /// nothing.
    pub fn search(&mut self, query: &str, from_page: usize) -> u64 {
        self.generation += 1;
        let command = match Query::new(query) {
            Some(query) => Command::Search {
                generation: self.generation,
                query,
                from_page,
            },
            None => Command::Cancel,
        };
        let _ = self.commands.send(command);
        self.generation
    }

    /// Asks for `page`'s text, answered with [`SearchUpdate::Text`] ahead
    /// of any search or background extraction. Pages out of range are
    /// ignored.
    pub fn request_text(&self, page: usize) {
        let _ = self.commands.send(Command::Text(page));
    }

    /// Counts the document's words and characters, every page's text read
    /// for it if it is not read yet, answered with one
    /// [`SearchUpdate::Counts`] once every page is counted.
    pub fn count(&self) {
        let _ = self.commands.send(Command::Count);
    }

    /// Drops `page`'s text, so it is read again when next needed: after
    /// the page was turned.
    pub fn forget(&self, page: usize) {
        let _ = self.commands.send(Command::Forget(page));
    }

    pub fn try_recv(&self) -> Option<SearchUpdate> {
        self.results.try_recv().ok()
    }
}

/// A search under way.
struct Job {
    generation: u64,
    query: Query,
    from_page: usize,
    /// How many pages are searched.
    searched: usize,
    found: usize,
}

struct Worker<'a> {
    document: &'a Transformed,
    results: &'a Sender<SearchUpdate>,
    waker: &'a dyn Fn(),
    /// Each page's text that is worth holding: the text of pages the app
    /// asked for, with the transform it was read with. A page read for a
    /// walk — a search, the counting pass — is dropped once the walk has
    /// used it.
    texts: Vec<Option<(PageTransform, Arc<PageText>)>>,
    /// Whether the app holds each page's text, and so whether it is kept.
    wanted: Vec<bool>,
    /// Pages whose text could not be read, remembered so a walk that drops
    /// the text does not read them — and fail — again.
    failed: Vec<bool>,
    /// Pages whose text was asked for, to answer first.
    requests: VecDeque<usize>,
    /// The next page the counting pass reads.
    idle: usize,
    /// Each page's counts, once counted.
    counts: Vec<Option<noido_text::Counts>>,
    /// Count every page's text.
    counting: bool,
    /// The walk's one report has gone out.
    reported: bool,
    job: Option<Job>,
}

impl<'a> Worker<'a> {
    fn new(
        document: &'a Transformed,
        results: &'a Sender<SearchUpdate>,
        waker: &'a dyn Fn(),
    ) -> Worker<'a> {
        Worker {
            document,
            results,
            waker,
            texts: (0..document.page_count()).map(|_| None).collect(),
            wanted: vec![false; document.page_count()],
            failed: vec![false; document.page_count()],
            requests: VecDeque::new(),
            idle: 0,
            counts: (0..document.page_count()).map(|_| None).collect(),
            counting: false,
            reported: false,
            job: None,
        }
    }

    fn run(mut self, first: Command, commands: &Receiver<Command>) {
        self.apply(first);
        loop {
            // Commands first: a newer search replaces the one under way.
            let command = if self.busy() {
                match commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                }
            };
            if let Some(command) = command {
                self.apply(command);
                continue;
            }
            if !self.step() {
                return;
            }
        }
    }

    /// Whether there is work without a command.
    fn busy(&self) -> bool {
        !self.requests.is_empty()
            || self.job.is_some()
            || (self.counting && self.idle < self.idle_end())
    }

    /// Where the counting pass has to reach: every page.
    fn idle_end(&self) -> usize {
        self.texts.len()
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Search {
                generation,
                query,
                from_page,
            } => {
                let count = self.texts.len();
                if count == 0 {
                    self.job = None;
                    self.send(SearchUpdate::Done {
                        generation,
                        cut_short: false,
                    });
                    return;
                }
                self.job = Some(Job {
                    generation,
                    query,
                    from_page: from_page.min(count - 1),
                    searched: 0,
                    found: 0,
                });
            }
            Command::Cancel => self.job = None,
            Command::Text(page) => {
                if page < self.texts.len() {
                    self.wanted[page] = true;
                    self.requests.push_back(page);
                }
            }
            Command::Forget(page) => {
                if page < self.texts.len() {
                    self.wanted[page] = false;
                }
                if let Some(text) = self.texts.get_mut(page) {
                    *text = None;
                }
                // The counts stay: the text is read again for a reshape,
                // which does not change what the page says.
            }
            Command::Count => {
                self.counting = true;
                self.idle = 0;
                self.reported = false;
                // Pages whose text is read already — every page for a reader
                // who searched first — are counted here, so the answer is
                // there the moment the page opens; the rest are counted as
                // they are read.
                let _ = self.finish_counts();
            }
        }
    }

    /// Searches or extracts one page; false once nobody listens.
    fn step(&mut self) -> bool {
        if let Some(page) = self.requests.pop_front() {
            let text = self.text(page).clone();
            let transform = self.texts[page]
                .as_ref()
                .map_or_else(PageTransform::default, |t| t.0);
            return self.send(SearchUpdate::Text {
                page,
                text,
                transform,
            });
        }
        let Some(mut job) = self.job.take() else {
            // One page a step, so a command still cuts in; the text is
            // dropped as soon as it is counted, unless the app holds it.
            if self.counting && self.idle < self.idle_end() {
                let page = self.idle;
                self.idle += 1;
                if self.counts[page].is_none() {
                    self.text(page);
                    self.count_page(page);
                    self.release(page);
                }
            }
            return self.finish_counts();
        };
        let count = self.texts.len();
        let page = (job.from_page + job.searched) % count;
        let text = self.text(page).clone();
        let mut hits = text.find(&job.query);
        // Counted on the way: a walk reads every page, so Document info
        // costs no reads of its own afterwards.
        self.count_page(page);
        self.release(page);
        let room = MAX_HITS - job.found;
        let truncated = hits.len() > room;
        hits.truncate(room);
        job.found += hits.len();
        job.searched += 1;
        let generation = job.generation;
        if !self.send(SearchUpdate::Page {
            generation,
            page,
            hits,
        }) {
            return false;
        }
        let cut_short = truncated || (job.found == MAX_HITS && job.searched < count);
        if cut_short || job.searched == count {
            return self.send(SearchUpdate::Done {
                generation,
                cut_short,
            });
        }
        self.job = Some(job);
        true
    }

    /// `page`'s text, read now if not held. A page whose text cannot be read
    /// is remembered as having none, so the counting pass and later searches
    /// do not ask for it again.
    fn text(&mut self, page: usize) -> &Arc<PageText> {
        if self.texts[page].is_none() {
            // Read before the text, so a turn under way stamps it old.
            let transform = self.document.transform(page);
            let text = if self.failed[page] {
                PageText::new(Vec::new())
            } else {
                match crate::glyphs_on_page(self.document, page) {
                    Ok(glyphs) => PageText::new(glyphs),
                    Err(e) => {
                        eprintln!("noido: text of page {}: {e}", page + 1);
                        self.failed[page] = true;
                        PageText::new(Vec::new())
                    }
                }
            };
            self.texts[page] = Some((transform, Arc::new(text)));
        }
        &self.texts[page].as_ref().expect("the page was just read").1
    }

    /// Counts `page`'s text, unless it is counted already.
    fn count_page(&mut self, page: usize) {
        if self.counts[page].is_some() {
            return;
        }
        // Only a page whose text is in hand: the rest are read for counting
        // by the pass, which counts each as it reads it.
        let Some((_, text)) = &self.texts[page] else {
            return;
        };
        self.counts[page] = Some(noido_text::Counts::of(text.text()));
    }

    /// Drops `page`'s text unless the app asked for it.
    fn release(&mut self, page: usize) {
        if !self.wanted[page] {
            self.texts[page] = None;
        }
    }

    /// Counts every page whose text is read but whose counts are not.
    fn count_missing(&mut self) {
        for page in 0..self.counts.len() {
            if self.texts[page].is_some() {
                self.count_page(page);
            }
        }
    }

    /// Counts every page whose text is read, and sends the document's counts
    /// once, when nothing is left to count. A page counted already keeps its
    /// counts: a reshape re-reads its text and changes nothing it says.
    fn finish_counts(&mut self) -> bool {
        if !self.counting {
            return true;
        }
        self.count_missing();
        if self.reported || !self.counted() {
            return true;
        }
        self.reported = true;
        self.report_counts()
    }

    /// Whether every page is counted.
    fn counted(&self) -> bool {
        self.counts.iter().all(Option::is_some)
    }

    /// Sends the document's counts; false once nobody listens.
    fn report_counts(&self) -> bool {
        let counted = Counted {
            pages: self.counts.len(),
            counted: self.counts.iter().filter(|c| c.is_some()).count(),
            total: self.counts.iter().flatten().fold(
                noido_text::Counts::default(),
                |mut total, &counts| {
                    total += counts;
                    total
                },
            ),
        };
        self.send(SearchUpdate::Counts(counted))
    }

    fn send(&self, update: SearchUpdate) -> bool {
        let sent = self.results.send(update).is_ok();
        if sent {
            (self.waker)();
        }
        sent
    }
}
