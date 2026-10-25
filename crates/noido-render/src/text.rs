//! The text thread: extracts page text on its own copy of the document,
//! keeps it, and searches it, so the window never waits for either.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use noido_engine::{Document, PdfBytes, PdfEngine};
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
}

enum Command {
    Extract,
    Search {
        generation: u64,
        query: Query,
        from_page: usize,
    },
    Cancel,
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
                match engine.open(bytes) {
                    Ok(document) => {
                        Worker::new(document.as_ref(), &result_tx, &*waker).run(first, &command_rx)
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

    /// Starts extracting every page's text in the background, if not yet
    /// started.
    pub fn extract(&self) {
        // A send error means the thread is gone; there is nothing to report to.
        let _ = self.commands.send(Command::Extract);
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
    document: &'a dyn Document,
    results: &'a Sender<SearchUpdate>,
    waker: &'a dyn Fn(),
    /// Each page's text, once extracted.
    texts: Vec<Option<PageText>>,
    /// Extract every page when idle.
    extract: bool,
    job: Option<Job>,
}

impl<'a> Worker<'a> {
    fn new(
        document: &'a dyn Document,
        results: &'a Sender<SearchUpdate>,
        waker: &'a dyn Fn(),
    ) -> Worker<'a> {
        Worker {
            document,
            results,
            waker,
            texts: (0..document.page_count()).map(|_| None).collect(),
            extract: false,
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
        self.job.is_some() || (self.extract && self.texts.iter().any(Option::is_none))
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Extract => self.extract = true,
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
        }
    }

    /// Searches or extracts one page; false once nobody listens.
    fn step(&mut self) -> bool {
        let Some(mut job) = self.job.take() else {
            if let Some(page) = self.texts.iter().position(Option::is_none) {
                self.text(page);
            }
            return true;
        };
        let count = self.texts.len();
        let page = (job.from_page + job.searched) % count;
        let mut hits = self.text(page).find(&job.query);
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

    /// `page`'s text, extracted now if not yet.
    fn text(&mut self, page: usize) -> &PageText {
        let document = self.document;
        self.texts[page].get_or_insert_with(|| match document.text(page) {
            Ok(glyphs) => PageText::new(glyphs),
            Err(e) => {
                eprintln!("noido: text of page {}: {e}", page + 1);
                PageText::new(Vec::new())
            }
        })
    }

    fn send(&self, update: SearchUpdate) -> bool {
        let sent = self.results.send(update).is_ok();
        if sent {
            (self.waker)();
        }
        sent
    }
}
