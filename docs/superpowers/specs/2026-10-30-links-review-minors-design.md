# Links into noido: review minors (issue #6)

Date: 2026-10-30. Agreed with the user. Follows
`2026-10-30-links-into-noido-design.md`.

## The changes

1. **Zero pages:** a page arrival on a document without pages does nothing.
   It no longer sets `chosen`.
2. **A test** that a named-place answer about the old version is dropped
   when the file reloads.
3. **Tests that use Unix paths** in `noido-core`'s `target.rs` run on Unix
   only.
4. **Other schemes:** an argument of the form `<scheme>://…` with a
   scheme other than `file` or `noido` is an error, "<arg>: only local
   files can be opened". A scheme is a letter followed by letters, digits,
   `+`, `-` or `.`, per RFC 3986. A Windows path like `C:\x` has no `//`
   after its colon, so it is not affected.
5. **Moving yourself drops a pending named place:** wherever noido notes
   that the reader moved the view (`reader_moved`), and when the reader
   leaves a place for a jump (`leave_here`), the named-place lookup still
   waiting is cancelled.
6. **The hand-over socket** reads at most 512 bytes of a request line. A
   longer request is ignored and not answered.
7. **Not done:** escaping more Markdown characters in the label. `my_notes`
   would turn into `my\_notes`, and intraword `_` doesn't format in
   CommonMark anyway.
8. **A link with a query** (`file:///a?b.pdf`, an unencoded `?`) is an
   error: "<arg>: has a ?…; encode ? in the file name as %3F". It never
   opens `/a`.

Each fix is its own commit, test first, prefixed with its crate, and says
"A review fix." in the body. The last one says `Fixes #6`.
