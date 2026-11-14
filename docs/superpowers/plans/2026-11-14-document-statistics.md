# Statistics in Document info Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Document info gains a Statistics section: page count, word count, character count and the character count without spaces, counted on the text thread's existing background walk and cached with each page's text.

**Architecture:** `noido-text` gains `Counts::of(text)`, a pure pass over one page's text. The text thread in `noido-render` counts the pages whose text it has read, and answers a new `Command::Count` — which also starts extraction — with one `SearchUpdate::Counts(Counted)` once every page is counted. The frontend's `Search` keeps it, and `info::show` draws it.

**Tech Stack:** Rust 2024, egui/eframe 0.37, `just` (fmt, clippy, tests), jj.

**Spec:** `docs/superpowers/specs/2026-11-14-document-statistics-design.md`

## Global Constraints

- No new dependencies.
- `just check` (fmt, clippy `-D warnings`, `cargo test --workspace --locked`) is green before every commit.
- Commits are jj: `jj commit -m "<message>"`, one per task, prefix `text`, `render`, `egui`.
- Copy exactly: section title `Statistics`; rows `Pages`, `Words`, `Characters`,
  `Characters (no spaces)`; the placeholder `Counting…` (U+2026, as `Reading…`);
  numbers grouped with a plain space — `94 781`, `1 234 567` — and bare below a
  thousand.
- Nothing is extracted until the reader asks: no work at open, and no work when
  Document info is shown if the text is already extracted.

## Review Focus

- A page with no text layer (a scan) counts 0, and the rows still finish: never stuck on `Counting…`.
- A page whose glyphs fail to extract, or panic, is counted as empty, and the totals still reach done.
- A document with no pages shows zeros at once, not `Counting…` forever.
- An in-place reload while the page is open recounts and never shows the old document's numbers.
- A hyphen at a page's foot ends its word rather than swallowing the next page's first word.

---

### Task 1: `Counts` in `noido-text`

**Files:**
- Create: `crates/noido-text/src/count.rs`
- Modify: `crates/noido-text/src/lib.rs`
- Test: `#[cfg(test)] mod tests` inside `count.rs` (the repo's habit, as in `info.rs` and `page.rs`)

**Interfaces:**
- Consumes: `crate::search::line_end_hyphen` (`pub(crate) fn line_end_hyphen(chars: &[char], i: usize) -> Option<(Hyphen, usize)>`, `i` being the hyphen's index and the returned index the one past the line break).
- Produces: `pub struct Counts { pub words: usize, pub chars: usize, pub chars_no_spaces: usize }`, deriving `Clone, Copy, Debug, Default, PartialEq, Eq`; `Counts::of(text: &str) -> Counts`; `impl std::ops::AddAssign for Counts`.

- [ ] **Step 1: Write the failing tests**

In a new `crates/noido-text/src/count.rs`, above an empty `Counts`, write the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_runs_of_non_space_characters() {
        assert_eq!(
            Counts::of("the cat sat\non a mat"),
            Counts { words: 6, chars: 19, chars_no_spaces: 15 }
        );
    }

    #[test]
    fn the_line_breaks_and_spaces_are_not_characters_but_spaces_are() {
        // The newline is noido's, not the document's.
        assert_eq!(
            Counts::of("a b\nc"),
            Counts { words: 3, chars: 4, chars_no_spaces: 3 }
        );
    }

    #[test]
    fn a_line_end_hyphen_does_not_split_a_word() {
        // "statis-"/"tics" is one word; the hyphen is drawn, so it is a
        // character, and it is not a space.
        assert_eq!(
            Counts::of("statis-\ntics"),
            Counts { words: 1, chars: 11, chars_no_spaces: 11 }
        );
        // A hyphen the word owns joins it the same way.
        assert_eq!(
            Counts::of("Anglo-\nSaxon"),
            Counts { words: 1, chars: 11, chars_no_spaces: 11 }
        );
    }

    #[test]
    fn a_soft_hyphen_is_not_a_drawn_character() {
        // A soft hyphen marks where a word may break; it joins the word
        // like a line-end hyphen, but it is not drawn, so it is no
        // character of the page.
        assert_eq!(
            Counts::of("statis\u{ad}\ntics"),
            Counts { words: 1, chars: 10, chars_no_spaces: 10 }
        );
    }

    #[test]
    fn a_hyphen_that_is_not_at_a_line_end_ends_nothing() {
        assert_eq!(
            Counts::of("well-known"),
            Counts { words: 1, chars: 10, chars_no_spaces: 10 }
        );
    }

    #[test]
    fn punctuation_and_digits_are_words() {
        // A lone dash is a word, as it is in Word; nothing here guesses at
        // the language to do better.
        assert_eq!(Counts::of("1997 — ok").words, 3);
    }

    #[test]
    fn empty_text_has_nothing() {
        assert_eq!(Counts::of(""), Counts::default());
    }

    #[test]
    fn pages_add_up() {
        let mut total = Counts::default();
        total += Counts::of("the cat sat");
        total += Counts::of("a dog");
        assert_eq!(total, Counts { words: 5, chars: 16, chars_no_spaces: 13 });
    }
}
```


- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p noido-text count`
Expected: FAIL to compile — `Counts` not defined, `count.rs` not a module.

- [ ] **Step 3: Register the module and write `Counts::of`**

In `crates/noido-text/src/lib.rs`, add `mod count;` beside the other modules and `pub use count::*;` beside the other re-exports.

In `count.rs`:

```rust
/// A text's size in words and characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub words: usize,
    /// Characters the page draws: line breaks are noido's and are not
    /// counted, and a line-end hyphen is drawn, so it is.
    pub chars: usize,
    /// The same without the spaces between words.
    pub chars_no_spaces: usize,
}

impl Counts {
    /// Counts `text`, one page's text as `PageText::text` has it: lines
    /// joined with newlines, spaces inferred from the gaps. A line-end
    /// hyphen inside a word joins the lines rather than splitting the word.
    pub fn of(text: &str) -> Counts { /* one pass, below */ }
}

impl std::ops::AddAssign for Counts {
    fn add_assign(&mut self, other: Counts) { /* three adds */ }
}
```

The pass, which the tests above do not spell out: collect `text`'s chars once, then
walk them with `i` and an `in_word` flag. At each `i`, if `in_word` and
`search::line_end_hyphen(&chars, i)` gives `Some((_, next))`, add one to `chars`
and to `chars_no_spaces` — unless the hyphen is a soft hyphen (`U+00AD`), which
is not drawn — and set `i = next`, leaving
`in_word` set — that is the whole rejoin. Otherwise a whitespace char clears
`in_word`; any other char adds a word when `in_word` was clear, then sets it,
and adds one to `chars_no_spaces` and one to `chars`. `AddAssign` adds the three
fields.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p noido-text count`
Expected: PASS, eight tests.

- [ ] **Step 5: Check lint and commit**

Run: `just check`
Expected: all green.

```bash
jj commit -m "text: count a page's words and characters"
```

---

### Task 2: Counting on the text thread

**Files:**
- Modify: `crates/noido-render/src/text.rs`
- Modify: `crates/noido-render/src/lib.rs` (the `pub use text::{…}` line)
- Test: `crates/noido-render/tests/searcher.rs`

**Interfaces:**
- Consumes: `noido_text::Counts` from Task 1.
- Produces:
  - `pub struct Counted { pub pages: usize, pub counted: usize, pub total: noido_text::Counts }`, deriving `Clone, Copy, Debug, Default, PartialEq, Eq`, with `pub fn done(&self) -> bool` (`self.counted >= self.pages`).
  - `SearchUpdate::Counts(Counted)`.
  - `TextSearcher::count(&self)`, which asks for extraction too.

- [ ] **Step 1: Write the failing tests**

In `crates/noido-render/tests/searcher.rs`, add a helper and the tests:

```rust
/// The counts, once they arrive: one report per walk, never a partial one.
fn until_counted(searcher: &TextSearcher) -> Counted {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        while let Some(update) = searcher.try_recv() {
            if let SearchUpdate::Counts(counted) = update {
                assert!(counted.done(), "a partial report: {counted:?}");
                return counted;
            }
        }
        assert!(Instant::now() < deadline, "counts not complete within 5 s");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn counts_arrive_for_every_page() {
    let engine = TextEngine::new(&["the cat sat", "a dog"]);
    let searcher = start(&engine);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(counted.pages, 2);
    assert_eq!(counted.counted, 2);
    assert_eq!(
        counted.total,
        noido_text::Counts { words: 5, chars: 16, chars_no_spaces: 13 }
    );
}

#[test]
fn reading_starts_only_when_counting_is_asked() {
    let engine = TextEngine::new(&["a", "b"]);
    let searcher = start(&engine);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(engine.calls(), 0);
    searcher.count();
    wait_for(|| engine.calls() == 2);
}

#[test]
fn a_document_already_extracted_is_counted_at_once() {
    let engine = TextEngine::new(&["a", "b"]);
    let searcher = start(&engine);
    searcher.extract();
    wait_for(|| engine.calls() == 2);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(counted.counted, 2);
    assert_eq!(engine.calls(), 2, "the pages were read again");
}

#[test]
fn a_page_read_again_keeps_its_counts() {
    // A reshape re-reads the page's text; it does not change what the page
    // says, so the counts stay and nothing is reported afresh.
    let engine = TextEngine::new(&["a b", "c"]);
    let searcher = start(&engine);
    searcher.count();
    until_counted(&searcher);
    searcher.forget(0);
    wait_for(|| engine.calls() == 3);
    std::thread::sleep(Duration::from_millis(50));
    assert!(searcher.try_recv().is_none(), "reported again");
}

#[test]
fn a_page_whose_read_is_asked_for_first_is_counted_too() {
    // Opening Document info asks for the counts while the visible page's
    // text — read on open, for picking — is still on its way.
    let engine = TextEngine::new(&["a b"]).gated();
    let searcher = start(&engine);
    searcher.count();
    searcher.request_text(0);
    std::thread::sleep(Duration::from_millis(20));
    engine.open_gate();
    let counted = until_counted(&searcher);
    assert_eq!(counted.total.words, 2);
}

#[test]
fn a_panicking_page_is_counted_as_empty() {
    let mut engine = TextEngine::new(&["a b", "c d"]);
    engine.panicking = Some(1);
    let searcher = start(&engine);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(counted.counted, 2);
    assert_eq!(counted.total.words, 2);
}

#[test]
fn a_page_that_cannot_be_read_is_counted_as_empty() {
    let mut engine = TextEngine::new(&["a b", "c d"]);
    engine.failing = Some(1);
    let searcher = start(&engine);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(counted.counted, 2);
    assert_eq!(counted.total.words, 2);
}

#[test]
fn a_page_with_no_text_counts_zero() {
    let engine = TextEngine::new(&["", "a"]);
    let searcher = start(&engine);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(
        counted.total,
        noido_text::Counts { words: 1, chars: 1, chars_no_spaces: 1 }
    );
}

#[test]
fn a_page_boundary_breaks_a_word() {
    // The hyphen at the foot of page 0 ends its word instead of joining
    // page 1's first: counts are kept per page.
    let engine = TextEngine::new(&["statis-", "tics"]);
    let searcher = start(&engine);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(counted.total.words, 2);
    assert_eq!(counted.total.chars, 11);
}

#[test]
fn a_document_with_no_pages_is_counted_at_once() {
    let engine = TextEngine::new(&[]);
    let searcher = start(&engine);
    searcher.count();
    let counted = until_counted(&searcher);
    assert_eq!(counted.pages, 0);
    assert!(counted.done());
    assert_eq!(counted.total, noido_text::Counts::default());
}
```

Import `Counted` beside `MAX_HITS, SearchUpdate, TextSearcher`, and make sure
`noido_text` is a dependency the test file can name (it is: `noido-render`
already depends on it). The last of these needs a read that can be held open,
so `TextEngine` there gains a gate beside `failing` and `panicking` —
`gate: Option<Arc<(Mutex<bool>, Condvar)>>`, waited on at the top of `text()`,
with `gated()` and `open_gate()` as the app harness's engine has them.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p noido-render --test searcher counted`
Expected: FAIL to compile — no `Counted`, no `SearchUpdate::Counts`, no `count`.

- [ ] **Step 3: Add `Counted`, the command and the counting pass**

In `crates/noido-render/src/text.rs`:

```rust
/// What the text thread has counted: the pages that went into the totals,
/// and how many pages there are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    pub pages: usize,
    pub counted: usize,
    pub total: noido_text::Counts,
}

impl Counted {
    pub fn done(&self) -> bool {
        self.counted >= self.pages
    }
}
```

`SearchUpdate` gains `Counts(Counted)` — one per walk, never a partial one.
`Command` gains `Count`.
`TextSearcher::count(&self)` sends `Command::Count` — its doc comment says it
counts the document's words and characters and starts extraction too.

`Worker` gains `counts: Vec<Option<noido_text::Counts>>` (all `None`, sized
like `texts`), `counting: bool` (false) and `reported: bool` (false).

- `Command::Count`: `self.extract = true; self.counting = true;
  self.reported = false;` and then `let _ = self.finish_counts();`.
- `busy()`: also true when `self.counting && self.counts.iter().any(Option::is_none)`.
- A private `finish_counts(&mut self) -> bool` is the whole counting rule:
  count every page whose text is read but whose count is not — whoever read the
  text, the extraction walk or a page's request, which is how the visible
  page's picking read is counted too — then, if every page is counted and no
  report has gone out for this walk, set `reported` and send the totals. It is
  the idle branch of `step()`, after `self.text(page)` when a page was just
  read, and the guard that keeps `busy()` and `step()` from disagreeing: with
  a count missing and every text read, it always makes progress where a
  walk-only rule would spin.
- `count_page(page)` counts one page: `Counts::of(self.text(page).text())` into
  `self.counts[page]`, unless it is counted already. The value is owned before
  `self.counts` is written, so the `&mut self` borrow of `self.text` ends.
- `Command::Forget(page)`: drops the text and **keeps the counts** — a
  reshape re-reads the text and changes nothing the page says, so the rows
  must not fall back to `Counting…` and count up again.

Send the totals with a private `fn report_counts(&self) -> bool` (returning
whether anyone still listens): `pages` is `self.counts.len()`, `counted` the
pages with counts, `total` their sum through `Counts`'s `AddAssign`. Re-export
`Counted` from `crates/noido-render/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p noido-render --test searcher`
Expected: PASS, the whole file, old tests included.

- [ ] **Step 5: Check lint and commit**

Run: `just check`
Expected: green.

```bash
jj commit -m "render: count words and characters on the text thread"
```

---

### Task 3: The Statistics section

**Files:**
- Modify: `crates/frontends/gui-egui/src/search.rs`
- Modify: `crates/frontends/gui-egui/src/info.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs` (the `Action::ShowDocumentInfo` arm, `fn info_page`)
- Test: `info.rs`'s own test module; harness tests in `app.rs`

**Interfaces:**
- Consumes: `noido_render::Counted` and `noido_text::Counts` from Tasks 1–2.
- Produces: `Search::count(&mut self)`; `Search::counts(&self) -> Option<Counted>`; `info::show(page, ctx, source, sizes, settings, counts: Option<Counted>) -> Option<Asked>`; `info::number(n: usize) -> String`.

- [ ] **Step 1: Write the failing `number` test**

In `info.rs`'s test module:

```rust
    #[test]
    fn numbers_are_grouped_by_threes() {
        assert_eq!(number(0), "0");
        assert_eq!(number(7), "7");
        assert_eq!(number(999), "999");
        assert_eq!(number(1000), "1 000");
        assert_eq!(number(94781), "94 781");
        assert_eq!(number(1234567), "1 234 567");
    }
```

Run: `cargo test -p gui-egui numbers_are_grouped`
Expected: FAIL — `number` not found.

- [ ] **Step 2: Implement `number`**

```rust
/// "94 781", "1 234 567": grouped by threes with a plain space, as the page
/// writes "216 × 279 mm".
pub(crate) fn number(n: usize) -> String { /* take the digits from the right */ }
```

Run: `cargo test -p gui-egui numbers_are_grouped`
Expected: PASS.

- [ ] **Step 3: Write the failing harness tests**

In `app.rs`'s test module, beside `with_info_source`:

```rust
    /// A harness whose document is `engine`'s pages, with a searcher on it,
    /// so Document info can count.
    fn info_with_text(engine: &TextEngine) -> Harness {
        let searcher = engine.clone();
        let source = crate::info::Source {
            engine: Arc::new(engine.clone()),
            bytes: Arc::new(Vec::new()),
            path: "/docs/paper.pdf".into(),
        };
        Harness::build_with(Arc::new(engine.clone()), move |app| {
            app.with_source(source).with_searcher(TextSearcher::start(
                Arc::new(searcher),
                Arc::new(Vec::new()),
                noido_render::Transforms::default(),
                Box::new(|| {}),
            ))
        })
    }

    #[test]
    fn document_info_shows_the_statistics() {
        // Two pages: 3 + 2 words, 11 + 5 characters, 9 + 4 without spaces.
        let engine = TextEngine::new(&[&["the cat sat"], &["a dog"]]);
        let mut h = info_with_text(&engine);
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.until(|h| h.text_rect("Words").is_some());
        h.until(|h| h.text_rect("Counting…").is_none());
        for text in [
            "Statistics",
            "Pages",
            "2",
            "Words",
            "5",
            "Characters",
            "16",
            "Characters (no spaces)",
            "13",
        ] {
            assert!(h.text_rect(text).is_some(), "{text:?} not drawn");
        }
    }

    #[test]
    fn document_info_counts_until_the_pages_are_read() {
        let engine = TextEngine::new(&[&["the cat sat"]]).gated();
        let mut h = info_with_text(&engine);
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.frames(10, vec![]);
        assert!(h.text_rect("Counting…").is_some(), "no placeholder");
        engine.open_gate();
        h.until(|h| h.text_rect("3").is_some());
        assert!(h.text_rect("Counting…").is_none(), "still counting");
    }

    #[test]
    fn nothing_is_counted_until_the_page_asks() {
        // The visible pages' text is read on open, for picking; what must
        // wait for Document info is the counting.
        let engine = TextEngine::new(&[&["the cat sat"], &["a dog"]]);
        let mut h = info_with_text(&engine);
        h.frames(20, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(h.app.search_state.counts().is_none(), "counted unasked");
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.until(|h| h.app.search_state.counts().is_some_and(|c| c.done()));
        assert!(engine.calls() >= 2, "not every page was read");
    }

    #[test]
    fn format_no_longer_lists_the_pages() {
        let engine = TextEngine::new(&[&["the cat sat"], &["a dog"]]);
        let mut h = info_with_text(&engine);
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.until(|h| h.text_rect("Words").is_some());
        let pages = h.texts().iter().filter(|t| *t == "Pages").count();
        assert_eq!(pages, 1, "Pages drawn {pages} times");
    }

    #[test]
    fn document_info_counts_the_new_version_after_a_reload() {
        let engine = TextEngine::new(&[&["x"]]);
        let mut h = info_with_text(&engine);
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.until(|h| h.text_rect("Words").is_some());
        h.until(|h| h.text_rect("Counting…").is_none());
        // "alpha beta gamma delta": 4 words, 22 characters.
        h.reload(Arc::new(TextEngine::new(&[&["alpha beta gamma delta"]])), recompiled_key());
        h.until(|h| h.text_rect("22").is_some());
        assert!(h.text_rect("4").is_some(), "the new words");
    }
```

Run: `cargo test -p gui-egui document_info_`
Expected: FAIL — the section and the counts are not drawn.

- [ ] **Step 4: Carry the counts to the page**

`search.rs`: `Search` gains `counts: Option<noido_render::Counted>` and
`counting: bool`, with:

- Its module doc names what it now carries: "…when to search and where to
  jump, and the document's counts of words and characters".

- `pub fn count(&mut self)` — sets `counting` and `extracting`, and asks the
  searcher once (as `extract` does, returning early when `counting` is already
  set).
- `pub fn counts(&self) -> Option<Counted>`.
- `receive()`: a `SearchUpdate::Counts(counted)` arm setting `self.counts = Some(counted)`.
- `set_searcher()`: asks a counting searcher to count, and clears `self.counts`
  so a reload never shows the old document's numbers.

`info.rs`:

- Its module doc says what the page now holds: "the file, what the document
  says about itself, its format, how much text it holds and its fonts".
- `show` and `contents` take `counts: Option<Counted>`.
- Format keeps `Page size`, the version, tagged and outline; its `Pages` row goes.
- A new section after Format, before Fonts:

```rust
    section(ui, "Statistics", |ui| {
        row(ui, "Pages", Some(number(sizes.len())));
        // "Counting…" until every page has been read; the keys stay, so the
        // section does not appear and reflow the page late.
        let shown = |f: fn(&noido_text::Counts) -> usize| match counts {
            Some(counted) if counted.done() => Some(number(f(&counted.total))),
            _ => Some("Counting…".to_owned()),
        };
        row(ui, "Words", shown(|c| c.words));
        row(ui, "Characters", shown(|c| c.chars));
        row(ui, "Characters (no spaces)", shown(|c| c.chars_no_spaces));
    });
```

`app.rs`:

- `Action::ShowDocumentInfo`: call `self.search_state.count()` beside opening the
  page, so the walk starts when the reader asks for the page.
- `fn info_page`: pass `self.search_state.counts()` as `show`'s last argument.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p gui-egui info`
Expected: PASS — the new tests and the existing info tests.

- [ ] **Step 6: Check lint and commit**

Run: `just check`
Expected: green.

```bash
jj commit -m "egui: show pages, words and characters in Document info"
```

---

## By hand after the plan

`just build`, open a book, Ctrl+I: Pages at once, then the three numbers once
`Counting…` goes; compare the word count against a word processor's for the
opening chapter. Open a scan: zeros. Search first, then Ctrl+I: numbers on the
first frame.
