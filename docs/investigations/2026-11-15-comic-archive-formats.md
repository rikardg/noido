# Comic archive formats: what to support next

Date: 2026-11-15

## The question

noido reads CBZ (a ZIP of images). Which other comic archive formats are
worth adding, either because the work is small or because the format is
common enough to matter?

This is a format and crate survey, not a sample of a comic collection. The
commonality column is a judgement from format documentation, MIME data and
crate usage; it is not measured over real files. Ask for real CB7 and CBR
files before scheduling either.

## Summary

| Format | Container | Rust crate | Licence | Random page access | Commonality | Recommendation |
|---|---|---|---|---|---|---|
| `.cbz` | ZIP | `zip` 9.5.1 | MIT | Direct by entry index; stored and deflate work | Most common | **Done** in this work |
| `.cbt` | TAR | `tar` 0.4.46 | MIT OR Apache-2.0 | Direct: an entry's byte range in the mapped archive | Uncommon, but trivial | **Next**; small, needs an `Archive` abstraction and a full content key |
| `.cb7` | 7z | `sevenz-rust2` 0.25.0 | Apache-2.0 | Poor in solid archives: a late page decodes the block from its start | Moderate | **Backlog**, only with a block cache or extraction design |
| `.cbr` | RAR | `unrar` 0.5.8 (wraps C++ `unrar_sys`) | MIT OR Apache-2.0 wrapper; UnRAR licence for the decoder | Streaming; solid RAR has the same block problem | Common in scene scans | **Backlog**, pending an explicit UnRAR licensing decision |
| `.cba` | ACE | None maintained | — | — | Very rare | **Drop** unless a decoder and a real sample appear |
| encrypted `.cbz` | ZIP + AES | `zip` without `aes-crypto` | — | — | Rare | **Out of scope** for now |

There is also plain `.zip` with images: the CLI already opens one by content,
because the engine dispatches on `PK\x03\x04`, but the file dialog lists only
`.cbz`. That is deliberate; a future dialog could offer `.zip` only if users
ask for it.

## Per-format notes

### CBT (TAR)

`tar` is mature (290M downloads), pure Rust, and handles only uncompressed
TAR. `Entry::raw_file_position()` is the file's data offset and `size()` is
its length, so on the mmap-backed `PdfBytes` a page is a slice; there is no
decompressor and no per-page seek cost. That makes CBT mechanically simpler
than CBZ once the engine has an archive abstraction.

Two costs are real:

- **Key**: TAR has no central directory and no CRC per entry. The sampled
  ZIP key (`length + first 4 KB + last 4 MB`) would miss a middle-only change,
  so a TAR must use the full content hash until a TAR-specific index exists.
- **Compression**: `.tar.gz`, `.tar.bz2` and `.tar.zst` are not covered by the
  `tar` crate alone. If those appear in practice, they need `flate2`/`bzip2`/
  `zstd` in front, and random access becomes streaming.

### CB7 (7z)

`sevenz-rust2` 0.25.0 is pure Rust, Apache-2.0, and actively released. Its
own documentation is explicit about the limit:

> When decoding a solid archive, the data to be decompressed depends on the
> data in front of it, you cannot simply skip the previous data and only
> decompress the data in the back.

A comic is usually one solid block of page images. Reading page 300 then
means decoding the first 299 pages, which makes lazy rendering and the
texture budget's re-fetch behaviour pathological. A cache or an extraction
step would be needed, with its own memory/disk budget. That is a design, not
a small format addition.

### CBR (RAR)

Common for scanned comics, but the only practical Rust route is `unrar`, which
wraps the C++ UnRAR decoder. It needs a C/C++ toolchain and the UnRAR licence
is not OSI: it permits decompression but restricts using the code to build a
RAR compressor. It also tends to be solid. This is a deliberate licensing and
build decision, not an "add a crate" decision.

### CBA (ACE)

No maintained Rust decoder was found. ACE is proprietary and rare. Drop it
unless a user brings a sample and a decoder worth binding.

### Encrypted ZIP

`zip` is built with `default-features = false, features = ["deflate-flate2"]`,
so AES-encrypted entries are not supported. That is intentional for now;
encrypted comics are rare and a password UI is a separate feature.

## Recommendation

1. **Ship CBZ** in the current work.
2. **Make CBT the next format.** It validates one small `Archive` abstraction
   (ZIP and TAR behind `read(entry) -> bytes`), adds no C dependency, and
   reuses all the image sniffing, ordering and rendering. Use a full content
   key, add `.cbt` to the dialog/MIME/UTI lists, and generate a small fixture.
3. **Revisit CB7 only with a block cache or extraction design** and a real
   sample. Do not add it just because a crate exists.
4. **Take CBR only with an explicit UnRAR licensing decision**, or if a
   pure-Rust RAR decoder with acceptable coverage appears.
5. **Drop CBA** until a sample and decoder exist.

What would change this: a set of real CB7/CBR files showing that non-solid
archives are common, an accepted UnRAR licence position, or a maintained
pure-Rust RAR decoder.
