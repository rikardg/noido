# Documents as a mapped clone, not bytes in RAM

Date: 2026-11-14 · Status: small change, agreed in conversation

## Intent

`PdfBytes` is `Arc<Vec<u8>>`: the whole file is read into memory and held
for as long as the document is open — 190–315 MB for the larger books in
`../testfiles/` — and it lives in anonymous memory, which the OS can
never take back. The bytes are read-only, so they need not be in RAM at
all: map a copy-on-write clone of the file instead, which costs no disk
and whose pages are file-backed and reclaimable. Where the file system
cannot clone, keep reading into memory: the same behaviour as today, not
a worse one.

## Measured

- Why not map the file itself: a process that truncates a mapped file
  kills the reader. In a Linux container (kernel 7.2.5), mmap + truncate
  → SIGBUS (signal 7), whether or not the page had been faulted in
  before. macOS/APFS survived three variants of the same test, but that
  is file system behaviour, not a contract.
- APFS, `clonefile(2)`, 1 GB: the clone cost 0 bytes; rewriting the
  source in place left 954 MB pinned by the clone; unlinking the clone
  freed 953 MB at once.
- Linux, `O_TMPFILE` + `FICLONE` + `mmap` + both fds closed (container on
  XFS through overlayfs), 200 MB: the clone cost 0 bytes; rewriting the
  source left 199.2 MB pinned; the mapping still read it; `munmap` freed
  200.4 MB.
- The mapped pages are file-backed, not anonymous: with 200 MB mapped and
  both ends touched, the process's `Anonymous` stayed at 6 MB.
- Whole process: with Lazarus open, its 190 MB file was 194 MB of
  `MALLOC_LARGE` and the app settled at 478 MB. Mapped, that goes.
- Reflink is available by default on Fedora Workstation (btrfs), Fedora
  Server and the RHEL family (XFS with `reflink=1`), macOS (APFS) and
  OpenZFS 2.2+ with block cloning. Not on ext4 (Debian, Ubuntu, the
  `archinstall` default), f2fs, tmpfs, NTFS, or most network and VM
  shares.

## Approach

Chosen: **clone when the file system can, read into memory when it
cannot; never copy.**

- Mapping the file itself: a truncating writer crashes the process.
- Copying to a temp file and mapping the copy: safe, but duplicates the
  file on disk for as long as it is open.
- Catching SIGBUS (`sigsetjmp`/`siglongjmp`): undefined behaviour in
  Rust, and it unwinds through the engine's locks and borrows.
- Preventing the write (leases, mandatory or advisory locks, `mlock`,
  size checks): none binds `pdflatex`, `typst`, `cp` or a downloader, and
  `mlock` also pins the memory being given back.
- A separate process for the reader: the only complete answer where no
  clone is possible, and out of all proportion to the gain.

`memmap2`, `rustix` (Linux: `openat` with `O_TMPFILE`, `ioctl_ficlone`)
and `libc` (macOS: `clonefile`) are all already in `Cargo.lock`.

## Behaviour

- `PdfBytes` becomes two variants, `Mapped(Arc<Mmap>)` and
  `Memory(Arc<Vec<u8>>)`, and derefs to `[u8]`, so `len`, `starts_with`
  and the `DocKey` hash are unchanged.
- `load_document`: below 8 MB, read into memory (my call: a paper's bytes
  are not worth a temp inode); at or above it, try the clone and fall
  back to memory if any step fails — no reflink, an unwritable directory,
  no writable place on the file's volume, `EXDEV`.
- The clone is taken in the document's own directory, so that it shares
  the file's volume. Linux: `openat(dir, O_TMPFILE | O_RDWR)` and
  `ioctl(FICLONE)` — an unnamed inode, so nothing is left behind even
  after a crash. macOS: `clonefile` into a uniquely named temp file
  beside it, opened and mapped, then unlinked. When the directory cannot
  be written, the app's cache directory is tried if it is on the same
  volume; else memory.
- The source file is closed once the clone exists; only the watcher keeps
  watching the path.
- One clone per open version: all five threads and the info page share
  the same `PdfBytes`.
- A superseded generation costs disk, not RAM: a writer that rewrites the
  file pins the old blocks until the document is reloaded or closed, and
  then the clone is dropped and they are freed. Until the file changes, a
  clone costs nothing.
- Reload is unchanged: `load_document` runs again on the new version,
  `DocKey` decides whether anything changed, and the old clone dies with
  the last thread holding it. A reload is now also always a coherent
  snapshot, so a file caught mid-write can no longer open as a damaged
  document.
- Nothing about this is shown in the UI: which way a document opened is
  not reported anywhere (agreed).

## Tests

- The loader returns `Mapped` for a file above the threshold on a
  reflink-capable directory, and `Memory` below it. The threshold is a
  constant and the loader's inner function takes it, so tests use
  kilobytes.
- A clone that cannot happen — an unwritable directory, or a clone the
  test turns off — falls back to `Memory` with the same bytes.
- The crash this removes, as a test: truncate the source after loading,
  then read through the `PdfBytes`; with a clone it still answers. Skipped
  where the temp directory's file system cannot clone, which is the case
  that falls back to memory anyway.
- Reload: a mapped document whose file is rewritten loads again from a
  fresh clone, with the new content and a new `DocKey`; identical content
  is not a reload. That the old mapping's blocks are freed once the old
  bytes are dropped is not tested: there is no portable way to ask.
- Both variants answer the engine identically: the same page count and a
  pixel-identical render of a fixture.
- No file is left in the document's directory after opening.

## Not included

A copy-based fallback for ext4 and the other non-reflink file systems (by
choice: no duplicated file on disk), any handling of mapping a live file
(nothing maps one), mapping on Windows, and any UI or setting for which
path was taken.

Nor any defence against the volume itself going away: a mapping over a
disk that is unplugged, force-unmounted or answering with an I/O error
faults like a truncated file does, where bytes read into memory would
not. The clone removes the writer, not the disk.
