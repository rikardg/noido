//! The document's bytes: a private clone of the file, mapped, where the
//! file system can make one; read into memory where it cannot.

use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use memmap2::Mmap;

#[derive(Clone)]
pub enum PdfBytes {
    /// A private clone of the file, mapped: no RAM of its own, and nothing
    /// else can change the bytes underneath.
    Mapped(Arc<Mmap>),
    /// The file's bytes, read.
    Memory(Arc<Vec<u8>>),
}

impl PdfBytes {
    /// The file at `path`, as a clone mapped from `dir`: `None` where the
    /// file system cannot clone, and for anything else that goes wrong. A
    /// clone shares the file's volume, so `dir` has to be on it; a
    /// directory elsewhere simply fails, and the caller reads the file.
    pub fn mapped_in(path: &Path, dir: &Path) -> Option<Self> {
        cloned(path, dir).map(|map| Self::Mapped(Arc::new(map)))
    }

    /// Whether these are a mapped clone rather than bytes read into memory.
    pub fn is_mapped(&self) -> bool {
        matches!(self, Self::Mapped(_))
    }
}

/// A copy-on-write clone of `path`, made in `dir` and mapped. `None` where
/// the file system cannot clone, or anything else goes wrong: the caller
/// reads the file instead.
#[cfg(target_os = "linux")]
fn cloned(path: &Path, dir: &Path) -> Option<Mmap> {
    use rustix::fs::{Mode, OFlags};

    let source = std::fs::File::open(path).ok()?;
    // An unnamed inode: there is nothing to clean up, even after a crash,
    // and nothing else can open it to change the bytes under the mapping.
    let clone = rustix::fs::openat(
        rustix::fs::CWD,
        dir,
        OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .ok()?;
    rustix::fs::ioctl_ficlone(&clone, &source).ok()?;
    // SAFETY: the only handle on the clone is `clone`, which is unnamed and
    // cannot be truncated or written by anything else.
    #[allow(unsafe_code)]
    unsafe { Mmap::map(&clone) }.ok()
}

/// A copy-on-write clone of `path`, made in `dir` and mapped. `None` where
/// the file system cannot clone, or anything else goes wrong: the caller
/// reads the file instead.
#[cfg(target_os = "macos")]
fn cloned(path: &Path, dir: &Path) -> Option<Mmap> {
    cloned_named(path, dir, clone_name)
}

/// [`cloned`], with the names to try taken from `name`: a name can already
/// be there, from a process that was killed between the clone and its
/// unlink, and then another one is used rather than reading the book.
#[cfg(target_os = "macos")]
fn cloned_named(path: &Path, dir: &Path, mut name: impl FnMut() -> String) -> Option<Mmap> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let from = CString::new(path.as_os_str().as_bytes()).ok()?;
    for _ in 0..4 {
        let clone = dir.join(name());
        let to = CString::new(clone.as_os_str().as_bytes()).ok()?;
        // SAFETY: both strings are NUL-terminated and outlive the call.
        #[allow(unsafe_code)]
        let made = unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0;
        if !made {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                continue;
            }
            return None;
        }
        let file = std::fs::File::open(&clone).ok();
        // The clone must lose its name before it is mapped: a name is a way
        // in for anything that can write the document's directory, and the
        // mapping is only sound while nothing else can write the bytes. A
        // clone that cannot be unlinked is no clone at all.
        if std::fs::remove_file(&clone).is_err() {
            return None;
        }
        let file = file?;
        // SAFETY: the only handle on the clone is `file`, and it has no name.
        #[allow(unsafe_code)]
        return unsafe { Mmap::map(&file) }.ok();
    }
    None
}

/// A name for the clone that nothing else is likely to be using.
/// `/dev/urandom` is there on the platforms this runs on; the pid and a
/// counter are what is left if it is not.
#[cfg(target_os = "macos")]
fn clone_name() -> String {
    use std::io::Read;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut bytes = [0u8; 8];
    let random = std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_or(0, |()| u64::from_le_bytes(bytes));
    format!(
        ".noido-clone.{}.{random:x}.{:x}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn cloned(_path: &Path, _dir: &Path) -> Option<Mmap> {
    None
}

impl std::fmt::Debug for PdfBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.is_mapped() { "Mapped" } else { "Memory" };
        write!(f, "PdfBytes::{kind}({} bytes)", self.len())
    }
}

impl Deref for PdfBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Mapped(map) => map,
            Self::Memory(bytes) => bytes,
        }
    }
}

impl Default for PdfBytes {
    fn default() -> Self {
        Self::Memory(Arc::new(Vec::new()))
    }
}

impl From<Vec<u8>> for PdfBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Memory(Arc::new(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    /// A directory to make files in, for this test. The system temp
    /// directory is used when its file system can clone, so that the tests
    /// exercise the clone on machines that can make one; the workspace's
    /// `target` beside the source otherwise (a running kernel's `/tmp` is
    /// often a tmpfs, which cannot).
    fn dir() -> tempfile::TempDir {
        for base in [
            std::env::temp_dir(),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        ] {
            if let Ok(dir) = tempfile::TempDir::new_in(&base)
                && clones(dir.path())
            {
                return dir;
            }
        }
        tempfile::TempDir::new().unwrap()
    }

    /// A file of exactly `len` bytes, ending in a marker, in `dir`.
    fn file(dir: &Path, name: &str, len: usize) -> PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&vec![b'a'; len - 4]).unwrap();
        file.write_all(b"TAIL").unwrap();
        path
    }

    /// Whether this machine's temp directory can clone at all: the tests
    /// that need a clone skip where it cannot.
    fn clones(dir: &Path) -> bool {
        let path = file(dir, "probe", 4096);
        PdfBytes::mapped_in(&path, dir).is_some()
    }

    #[test]
    fn a_mapped_clone_reads_like_the_file() {
        let dir = dir();
        if !clones(dir.path()) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(dir.path(), "doc.pdf", 200_000);
        let bytes = PdfBytes::mapped_in(&path, dir.path()).expect("a clone");
        assert!(bytes.is_mapped());
        assert_eq!(&bytes[..], std::fs::read(&path).unwrap().as_slice());
    }

    #[test]
    fn a_mapped_clone_survives_the_file_being_rewritten() {
        let dir = dir();
        if !clones(dir.path()) {
            eprintln!("skipping: this file system cannot clone");
            return;
        }
        let path = file(dir.path(), "doc.pdf", 200_000);
        let bytes = PdfBytes::mapped_in(&path, dir.path()).expect("a clone");
        // The writer truncates and rewrites the document in place, which
        // kills a reader that mapped the file itself.
        std::fs::write(&path, b"short").unwrap();
        assert_eq!(bytes.len(), 200_000);
        assert!(bytes.ends_with(b"TAIL"));
    }

    #[test]
    fn mapping_a_missing_file_gives_none() {
        let dir = dir();
        assert!(PdfBytes::mapped_in(&dir.path().join("nope.pdf"), dir.path()).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_taken_name_gets_another_one() {
        let dir = dir();
        let path = file(dir.path(), "doc.pdf", 200_000);
        // A leftover from a process killed between the clone and its unlink,
        // under the first name the clone is offered.
        std::fs::write(dir.path().join("taken"), b"leftover").unwrap();
        let mut names = ["taken", "free"].into_iter();
        let mapped = cloned_named(&path, dir.path(), || names.next().unwrap().to_string());
        assert!(mapped.is_some(), "the second name was not tried");
        assert_eq!(
            std::fs::read(dir.path().join("taken")).unwrap(),
            b"leftover"
        );
        assert!(!dir.path().join("free").exists(), "the clone kept its name");
    }

    #[test]
    fn bytes_that_are_read_still_answer_as_slices() {
        let bytes = PdfBytes::from(b"%PDF-1.7".to_vec());
        assert!(!bytes.is_mapped());
        assert!(bytes.starts_with(b"%PDF-"));
        assert_eq!(bytes.len(), 8);
        assert!(PdfBytes::default().is_empty());
    }
}
