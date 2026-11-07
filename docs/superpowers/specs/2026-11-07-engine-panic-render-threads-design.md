# Keeping the render threads alive when the engine panics

## Intent

An engine that panics on one page takes its thread with it. Every thread in
`noido-render` is a loop, and a panic inside one page's work unwinds past the
loop: the thread ends, and everything it was for ends with it. In the worst
case the render thread dies on the page the reader is looking at and no page
is ever drawn sharp again — the window keeps showing thumbnails, with
nothing in it saying why.

That is no longer hypothetical: `gettingstartedwithkubernetes_secondedition.pdf`
has tiling patterns with a step of 99999 points, which hayro 0.8 turns into a
65535-pixel tile and vello answers with a panic. 64 of its 276 pages do it,
and the first one kills the render thread for the rest of the session.

A panic is the engine's business, but losing the thread is noido's.

## Goals

- A panic anywhere in a thread's engine work leaves every thread in
  `noido-render` running: the render thread, thumbnails, text and links.
- The page or request that panicked is reported the way that thread already
  reports a failure: an `EngineError` for a render, an empty link list, no
  flash area, no destination, empty text for that page.
- The reader loses that page, not the rest of the document: the pages after
  it still render sharp.
- The panic is not swallowed: the engine's own panic message (with its
  location, and the thread's name) is still printed by the panic hook, and
  the failure reported through the normal path names it too
  (`noido: page 19: render failed: the engine panicked: …`).
- A page whose text panics is remembered as having no text, so extraction
  does not try it again on every pass.

## Non-goals

- Catching panics in engine-hayro, or in the window's own code: a panic in
  the GUI thread still ends the app.
- A new way to show a failed page. A page that fails keeps its thumbnail, as
  any render error does today; there is no error page or badge.
- Retrying. A failed page is not asked for again at that scale, as today.
- Opening the document: a thread that cannot open has nothing left to do, so
  it ends as it does now.
- The properties thread: one shot, and the thread alive or dead changes
  nothing else.
- The engine bug behind the panic itself; that is the hayro 0.9 upgrade,
  a separate change.

## Design

### `noido-render`

One helper in `lib.rs`:

```rust
pub(crate) fn caught<T>(f: impl FnOnce() -> Result<T, EngineError>) -> Result<T, EngineError>
```

`catch_unwind` over `AssertUnwindSafe(f)`. `AssertUnwindSafe` is right here
because nothing reads state a half-finished render left behind: `RenderCache`
is built per render, and the parsed `Pdf` is only read.

The panic payload, read as `&str` or `String`, becomes
`EngineError::Render(format!("the engine panicked: {payload}"))`; anything
else says "the engine panicked". `caught` itself prints nothing — the panic
hook has already printed the location, and the call site reports the failure
it reports for any error.

A second helper for the links thread's `Option`-answering calls:

```rust
pub(crate) fn caught_none<T>(f: impl FnOnce() -> Option<T>) -> Option<T>
```

### Where it wraps

- **Render thread** (`lib.rs::run`): the per-page job,
  `document.page_size(page).and_then(|size| render_whole(…))`. Keep-range
  skipping and queue coalescing stay outside.
- **Thumbnails** (`thumbnails.rs::run`): the same job including
  `thumbnail_scale`, answering `(1.0, Err(…))` on a panic, as it does when
  the page size is unknown.
- **Glyphs** (`lib.rs::glyphs_on_page`): the whole extraction, including the
  `page_size` call it filters with. This covers the text thread and the
  links thread's flash area.
- **Links** (`links.rs::run`): `document.links(page)` (empty list on a
  panic, logged with the page number), `flash_area` and
  `document.named_destination` (`caught_none`).
- **Text** (`text.rs::Worker::text`): the failed page is stored as
  `PageText::new(Vec::new())` rather than left unextracted, so the idle
  extract pass and searches do not panic on it again.

### Tests

One per thread, each with a fake engine that panics on one page and answers
on the others: the render thread, the thumbnail thread, the searcher and the
linker. Each asserts the failure is reported for the panicking page and that
a later page or request is still answered.
