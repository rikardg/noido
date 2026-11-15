//! Reading a document: its bytes mapped through a private clone of the
//! file where the file system can make one, the bytes themselves where it
//! cannot.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use noido_engine::PdfBytes;
use noido_state::DocKey;

/// Files at least this large are mapped through a clone where the file
/// system can make one. Below it the bytes are not worth a temporary inode.
pub const MAPPED_FROM: u64 = 8 * 1024 * 1024;

/// The key a document's state is kept under. A ZIP is sampled at its ends:
/// its central directory at the end names every entry, and hashing a large
/// comic whole would read the whole of it before the first page appears.
pub fn doc_key(bytes: &PdfBytes, path: &Path) -> DocKey {
    if bytes.starts_with(b"PK\x03\x04") {
        DocKey::sampled(bytes, path)
    } else {
        DocKey::new(bytes, path)
    }
}

/// Reads the file at `path`, with a user-facing message on failure.
pub fn load_document(path: &Path) -> Result<PdfBytes, String> {
    load_with(path, MAPPED_FROM, scratch_dir)
}

/// Reads the file at `path`: mapped when it is at least `mapped_from` bytes
/// and the file system can clone it, read otherwise. `scratch` is where a
/// clone goes when the file's own directory will not take it; it is only
/// asked for once that has failed.
fn load_with(
    path: &Path,
    mapped_from: u64,
    scratch: impl FnOnce() -> Option<PathBuf>,
) -> Result<PdfBytes, String> {
    let shown = path.display();
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(format!("{shown}: no such file")),
        Err(e) => return Err(format!("{shown}: {e}")),
    };
    if meta.is_dir() {
        return Err(format!("{shown}: is a directory"));
    }
    let len = meta.len();
    // Before the read, not after it: a FIFO or a `/dev` entry has no length
    // to speak of, and reading it would block.
    if len == 0 {
        return Err(format!("{shown}: file is empty"));
    }
    // A file whose bytes are worth keeping out of RAM: mapped through a
    // clone of it where the file system can make one.
    if len >= mapped_from {
        let dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if let Some(bytes) = PdfBytes::mapped_in(path, dir) {
            return Ok(bytes);
        }
        if let Some(scratch) = scratch()
            && let Some(bytes) = PdfBytes::mapped_in(path, &scratch)
        {
            return Ok(bytes);
        }
    }
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => format!("{shown}: no such file"),
        _ => format!("{shown}: {e}"),
    })?;
    Ok(PdfBytes::from(bytes))
}

/// Where a clone goes when the document's own directory will not hold one:
/// the app's cache directory, made if it is not there. A clone across
/// volumes fails on its own, so a cache directory elsewhere is no worse
/// than none. The XDG rule is the one `noido-settings`' `config_dir`
/// spells out: an absolute `XDG_CACHE_HOME` wins, and `directories` finds
/// the platform's own otherwise.
fn scratch_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("XDG_CACHE_HOME").filter(|dir| Path::new(dir).is_absolute()) {
        Some(dir) => PathBuf::from(dir).join("noido"),
        None => directories::ProjectDirs::from("", "", "noido")?
            .cache_dir()
            .to_path_buf(),
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own. The system temp directory is used
    /// when its file system can clone, so that the tests exercise the
    /// clone on machines that can make one; the workspace's `target`
    /// beside the source otherwise (a running kernel's `/tmp` is often a
    /// tmpfs, which cannot).
    fn temp_dir(name: &str) -> PathBuf {
        let prefix = format!("noido-load-{}-{name}", std::process::id());
        for base in [
            std::env::temp_dir(),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target"),
        ] {
            if let Ok(dir) = tempfile::Builder::new().prefix(&prefix).tempdir_in(&base)
                && clones(dir.path())
            {
                return dir.keep();
            }
        }
        tempfile::Builder::new()
            .prefix(&prefix)
            .tempdir()
            .unwrap()
            .keep()
    }

    /// A file of exactly `len` bytes, ending in a marker, in `dir`.
    fn file(dir: &Path, name: &str, len: usize) -> PathBuf {
        let path = dir.join(name);
        let mut bytes = vec![b'a'; len - 4];
        bytes.extend_from_slice(b"TAIL");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Whether this machine's temp directory can clone at all: the tests
    /// that need a clone skip where it cannot.
    fn clones(dir: &Path) -> bool {
        let path = file(dir, "probe.pdf", 4096);
        PdfBytes::mapped_in(&path, dir).is_some()
    }

    #[test]
    fn missing_file() {
        let path = temp_dir("missing").join("nope.pdf");
        let err = load_with(&path, 0, || None).unwrap_err();
        assert!(err.ends_with(": no such file"), "{err}");
    }

    #[test]
    fn directory() {
        let err = load_with(&temp_dir("dir"), 0, || None).unwrap_err();
        assert!(err.ends_with(": is a directory"), "{err}");
    }

    #[test]
    fn empty_file() {
        let dir = temp_dir("empty");
        let path = dir.join("empty.pdf");
        std::fs::write(&path, b"").unwrap();
        let err = load_with(&path, 0, || None).unwrap_err();
        assert!(err.ends_with(": file is empty"), "{err}");
    }

    #[test]
    fn a_zip_key_sees_a_wide_tail_and_a_pdf_key_sees_the_middle() {
        let path = Path::new("/papers/a");
        let zip = |at: usize, byte: u8| {
            let mut bytes = vec![0u8; 8 * 1024 * 1024];
            bytes[..4].copy_from_slice(b"PK\x03\x04");
            bytes[at] = byte;
            PdfBytes::from(bytes)
        };
        // 2 MiB is outside the sampled ends; 5 MiB is inside the 4 MiB tail.
        assert_eq!(
            doc_key(&zip(2 * 1024 * 1024, 0), path).hex(),
            doc_key(&zip(2 * 1024 * 1024, 1), path).hex()
        );
        assert_ne!(
            doc_key(&zip(5 * 1024 * 1024, 0), path).hex(),
            doc_key(&zip(5 * 1024 * 1024, 1), path).hex()
        );
        let pdf = |at: usize, byte: u8| {
            let mut bytes = vec![0u8; 8 * 1024 * 1024];
            bytes[..5].copy_from_slice(b"%PDF-");
            bytes[at] = byte;
            PdfBytes::from(bytes)
        };
        assert_ne!(
            doc_key(&pdf(5 * 1024 * 1024, 0), path).hex(),
            doc_key(&pdf(5 * 1024 * 1024, 1), path).hex()
        );
    }

    #[test]
    fn reads_fixture() {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../fixtures/two-pages.pdf"
        ));
        let bytes = load_document(path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(!bytes.is_mapped(), "a fixture is far below the threshold");
    }

    #[test]
    fn a_file_at_the_threshold_is_mapped() {
        let dir = temp_dir("mapped");
        if !clones(&dir) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(&dir, "doc.pdf", 200_000);
        let bytes = load_with(&path, 200_000, || None).unwrap();
        assert!(bytes.is_mapped());
        assert!(bytes.ends_with(b"TAIL"));
    }

    #[test]
    fn a_file_below_the_threshold_is_read() {
        let dir = temp_dir("read");
        let path = file(&dir, "doc.pdf", 200_000);
        let bytes = load_with(&path, 200_001, || None).unwrap();
        assert!(!bytes.is_mapped());
        assert!(bytes.ends_with(b"TAIL"));
    }

    #[cfg(unix)]
    #[test]
    fn a_clone_goes_to_the_scratch_directory_when_the_documents_cannot() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("closed");
        let scratch = temp_dir("scratch");
        if !clones(&scratch) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(&dir, "doc.pdf", 200_000);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let bytes = load_with(&path, 200_000, || Some(scratch.clone()));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bytes = bytes.unwrap();
        assert!(
            bytes.is_mapped(),
            "the clone did not use the scratch directory"
        );
        assert!(bytes.ends_with(b"TAIL"));
    }

    #[cfg(unix)]
    #[test]
    fn a_clone_that_cannot_be_made_falls_back_to_the_bytes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("readonly");
        if !clones(&dir) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(&dir, "doc.pdf", 200_000);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let bytes = load_with(&path, 200_000, || None);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bytes = bytes.unwrap();
        assert!(!bytes.is_mapped());
        assert!(bytes.ends_with(b"TAIL"));
    }

    #[test]
    fn a_mapped_document_survives_its_file_being_rewritten() {
        let dir = temp_dir("rewritten");
        if !clones(&dir) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(&dir, "doc.pdf", 200_000);
        let bytes = load_with(&path, 200_000, || None).unwrap();
        assert!(bytes.is_mapped());
        std::fs::write(&path, b"a new, much shorter document").unwrap();
        assert_eq!(bytes.len(), 200_000);
        assert!(bytes.ends_with(b"TAIL"));
    }

    /// What a reload does with a mapped document: the new version is a new
    /// clone, the key over it changes, and the old mapping still reads the
    /// version it was made from.
    #[test]
    fn a_rewritten_document_loads_again_with_its_new_content() {
        let dir = temp_dir("reload");
        if !clones(&dir) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(&dir, "doc.pdf", 200_000);
        let first = load_with(&path, 0, || None).unwrap();
        let key = noido_state::DocKey::new(&first, &path);
        std::fs::write(&path, b"%PDF-1.7 a new version").unwrap();
        let second = load_with(&path, 0, || None).unwrap();
        assert_ne!(noido_state::DocKey::new(&second, &path).hex(), key.hex());
        assert!(second.starts_with(b"%PDF-1.7 a new version"));
        assert!(
            first.ends_with(b"TAIL"),
            "the old mapping changed with the rewrite"
        );
    }

    #[test]
    fn nothing_is_left_beside_the_document() {
        let dir = temp_dir("leftovers");
        let path = file(&dir, "doc.pdf", 200_000);
        let before: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let _ = load_with(&path, 0, || None);
        let after: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
    }
}
