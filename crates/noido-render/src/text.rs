//! The text thread: extracts page text on its own copy of the document,
//! keeps it, and searches it, so the window never waits for either.

use std::collections::{BTreeMap, VecDeque};
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use noido_core::{AnchorCheck, Anchored, Highlight, PageTransform, Walk, check, requote};
use noido_engine::{Document, PdfBytes, PdfEngine};

use crate::{Transformed, Transforms, open_transformed};
use noido_text::{Joined, PageText, Query, TextHit};

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
    /// Highlights anchored against the document's text. `asked` is the
    /// list [`TextSearcher::anchor`] was given, to compare with the
    /// window's current list.
    Anchored {
        asked: Vec<Highlight>,
        answer: Anchored,
    },
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
    Anchor {
        highlights: Vec<Highlight>,
        fill_missing: bool,
    },
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

    /// Finds the highlights' text again in the document, a few pages at a
    /// time, replacing an anchoring under way; answered with one
    /// [`SearchUpdate::Anchored`]. With `fill_missing`, highlights without
    /// a quote take one from the text where they are.
    pub fn anchor(&self, highlights: Vec<Highlight>, fill_missing: bool) {
        let _ = self.commands.send(Command::Anchor {
            highlights,
            fill_missing,
        });
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

/// Where an anchoring is.
enum Phase {
    /// Checking each highlight where it was, `next` being the one in hand.
    Check {
        next: usize,
        pending: Vec<(usize, Highlight)>,
    },
    /// Walking every page for the highlights that need searching.
    Walk {
        walk: Walk,
        page: usize,
        counts: Vec<usize>,
    },
    /// Giving the highlights found a quote of their new place.
    Requote { todo: Vec<usize>, next: usize },
}

/// An anchoring under way, holding only the pages its step needs.
struct AnchorJob {
    asked: Vec<Highlight>,
    items: Vec<Highlight>,
    fill_missing: bool,
    /// A page it read could not be read.
    failed: bool,
    /// The pages of the window in hand.
    held: BTreeMap<usize, Arc<PageText>>,
    phase: Phase,
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
    anchoring: Option<AnchorJob>,
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
            anchoring: None,
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
            || self.anchoring.is_some()
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
            Command::Anchor {
                highlights,
                fill_missing,
            } => {
                self.drop_anchoring();
                self.anchoring = Some(AnchorJob {
                    items: highlights.clone(),
                    asked: highlights,
                    fill_missing,
                    failed: false,
                    held: BTreeMap::new(),
                    phase: Phase::Check {
                        next: 0,
                        pending: Vec::new(),
                    },
                });
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
            if self.anchoring.is_some() {
                return self.anchor_step();
            }
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

    /// Lets go of the pages an anchoring under way holds.
    fn drop_anchoring(&mut self) {
        if let Some(job) = self.anchoring.take() {
            for page in job.held.into_keys() {
                self.release(page);
            }
        }
    }

    /// Reads `page` into the window in hand.
    fn read_into(&mut self, job: &mut AnchorJob, page: usize) {
        let text = self.text(page).clone();
        job.failed |= self.failed[page];
        job.held.insert(page, text);
    }

    /// Lets go of the held pages before `first`.
    fn release_before(&mut self, job: &mut AnchorJob, first: usize) {
        let gone: Vec<usize> = job.held.range(..first).map(|(&p, _)| p).collect();
        for page in gone {
            job.held.remove(&page);
            self.release(page);
        }
    }

    /// The first page of `window` not held yet.
    fn missing(job: &AnchorJob, window: &RangeInclusive<usize>) -> Option<usize> {
        window.clone().find(|p| !job.held.contains_key(p))
    }

    /// The held pages of `window`, joined.
    fn joined<'j>(
        held: &'j BTreeMap<usize, Arc<PageText>>,
        window: &RangeInclusive<usize>,
    ) -> Joined<'j> {
        let pages: Vec<&PageText> = window.clone().map(|p| &*held[&p]).collect();
        Joined::new(*window.start(), &pages)
    }

    /// One step of the anchoring, reading at most one page, so a command
    /// still cuts in. It mirrors `noido_core::anchor`, a few pages at a
    /// time; false once nobody listens.
    fn anchor_step(&mut self) -> bool {
        let Some(mut job) = self.anchoring.take() else {
            return true;
        };
        let n = self.texts.len();
        loop {
            match &mut job.phase {
                Phase::Check { next, pending } => {
                    let i = *next;
                    if i >= job.asked.len() {
                        if pending.is_empty() || job.failed {
                            return self.answer(job);
                        }
                        let pending = std::mem::take(pending);
                        job.phase = Phase::Walk {
                            walk: Walk::new(pending),
                            page: 0,
                            counts: vec![0; n],
                        };
                        continue;
                    }
                    let h = job.asked[i].clone();
                    let Some(window) = h.window(n) else {
                        if h.quote.is_some() {
                            pending.push((i, h));
                        }
                        *next += 1;
                        continue;
                    };
                    if let Some(page) = Self::missing(&job, &window) {
                        self.read_into(&mut job, page);
                        break;
                    }
                    let joined = Self::joined(&job.held, &window);
                    let checked = check(&h, &joined, job.fill_missing);
                    if let Phase::Check { next, pending } = &mut job.phase {
                        *next += 1;
                        if matches!(checked, AnchorCheck::Search) {
                            pending.push((i, h));
                        }
                    }
                    if let AnchorCheck::Keep(kept) = checked {
                        job.items[i] = kept;
                    }
                    self.release_before(&mut job, usize::MAX);
                }
                Phase::Walk { walk, page, .. } => {
                    if job.failed {
                        // Nothing found is trusted: they keep their places.
                        self.release_before(&mut job, usize::MAX);
                        return self.answer(job);
                    }
                    let k = *page;
                    if k >= n {
                        let Phase::Walk { walk, counts, .. } = std::mem::replace(
                            &mut job.phase,
                            Phase::Requote {
                                todo: Vec::new(),
                                next: 0,
                            },
                        ) else {
                            unreachable!("the phase was just matched");
                        };
                        self.release_before(&mut job, usize::MAX);
                        let mut todo = Vec::new();
                        for (i, h) in walk.finish(&counts) {
                            if !h.orphaned && h.window(n).is_some() {
                                todo.push(i);
                            }
                            job.items[i] = h;
                        }
                        job.phase = Phase::Requote { todo, next: 0 };
                        continue;
                    }
                    let window = walk.window(k, n);
                    if let Some(p) = Self::missing(&job, &window) {
                        self.read_into(&mut job, p);
                        let chars = job.held[&p].char_count();
                        if let Phase::Walk { counts, .. } = &mut job.phase {
                            counts[p] = chars;
                        }
                        break;
                    }
                    let joined = Self::joined(&job.held, &window);
                    if let Phase::Walk { walk, page, .. } = &mut job.phase {
                        walk.page(k, &joined);
                        *page += 1;
                    }
                    // The next window starts a page before the next page.
                    self.release_before(&mut job, k);
                }
                Phase::Requote { todo, next } => {
                    let Some(&i) = todo.get(*next) else {
                        return self.answer(job);
                    };
                    let window = job.items[i]
                        .window(n)
                        .expect("only highlights with a window are listed");
                    if let Some(page) = Self::missing(&job, &window) {
                        self.read_into(&mut job, page);
                        break;
                    }
                    let joined = Self::joined(&job.held, &window);
                    requote(&mut job.items[i], &joined);
                    if let Phase::Requote { next, .. } = &mut job.phase {
                        *next += 1;
                    }
                    self.release_before(&mut job, usize::MAX);
                }
            }
        }
        self.anchoring = Some(job);
        true
    }

    /// Sends the anchoring's one answer.
    fn answer(&mut self, mut job: AnchorJob) -> bool {
        self.release_before(&mut job, usize::MAX);
        let relocated = job
            .items
            .iter()
            .zip(&job.asked)
            .any(|(a, b)| a.start != b.start || a.end != b.end || a.orphaned != b.orphaned);
        self.send(SearchUpdate::Anchored {
            asked: job.asked,
            answer: Anchored {
                items: job.items,
                relocated,
            },
        })
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
