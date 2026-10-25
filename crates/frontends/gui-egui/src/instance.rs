//! One window per file: a running noido listens on a local socket named
//! after its file's path, and a later launch on the same file hands over to
//! it and exits.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerOptions, Name, Stream, prelude::*};

use crate::watch::Waker;

/// How long a launch waits for the running noido to answer.
const TIMEOUT: Duration = Duration::from_secs(1);

/// What a launch sends, and what the running noido answers.
const ACTIVATE: &str = "activate";
const OK: &str = "ok";

/// Where a document's socket is: a socket file on Unix, a named pipe on
/// Windows.
#[cfg(unix)]
type Address = PathBuf;
#[cfg(windows)]
type Address = String;

/// The outcome of asking whether the file is open already.
pub enum Claim {
    /// It is, and that noido was asked to come forward: this launch is done.
    Running,
    /// It isn't; this launch listens for later ones.
    First(Listener),
    /// No socket could be had: open the file without listening.
    Unavailable,
}

/// The socket of the first noido on a file.
pub struct Listener(interprocess::local_socket::Listener);

/// Set when a later launch asked the window to come forward.
#[derive(Clone, Default)]
pub struct Activation(Arc<AtomicBool>);

impl Activation {
    /// Whether the window was asked to come forward since the last call.
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::Relaxed)
    }

    pub(crate) fn request(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Hands over to the noido that has `path` open, if there is one; otherwise
/// claims the file for this launch. Doesn't read the file.
pub fn claim(path: &Path) -> Claim {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match address(&canonical) {
        Some(address) => claim_at(&address, TIMEOUT),
        None => Claim::Unavailable,
    }
}

/// [`claim`] for the socket at `address`, waiting up to `timeout` for an
/// answer.
fn claim_at(address: &Address, timeout: Duration) -> Claim {
    let Ok(name) = to_name(address) else {
        return Claim::Unavailable;
    };
    if let Ok(stream) = Stream::connect(name.clone()) {
        // A noido that doesn't answer can't be displaced either.
        return if hand_over(stream, timeout) {
            Claim::Running
        } else {
            Claim::Unavailable
        };
    }
    make_room(address);
    // Nobody answered: a socket file left by a crash is replaced.
    match ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .max_spin_time(timeout)
        .create_sync()
    {
        Ok(listener) => Claim::First(Listener(listener)),
        Err(_) => Claim::Unavailable,
    }
}

/// Asks the noido at the other end of `stream` to come forward.
fn hand_over(stream: Stream, timeout: Duration) -> bool {
    let answered = || -> std::io::Result<bool> {
        stream.set_recv_timeout(Some(timeout))?;
        stream.set_send_timeout(Some(timeout))?;
        let mut reader = BufReader::new(stream);
        writeln!(reader.get_mut(), "{ACTIVATE}")?;
        let mut line = String::new();
        reader.read_line(&mut line)?;
        Ok(line.trim_end() == OK)
    };
    answered().unwrap_or(false)
}

impl Listener {
    /// Answers later launches on a thread of its own; `waker` is called when
    /// one asks the window to come forward.
    pub fn serve(self, waker: Waker) -> Activation {
        let activation = Activation::default();
        let requested = activation.clone();
        // Without the thread, later launches time out and open their own window.
        let _ = thread::Builder::new()
            .name("noido-instance".into())
            .spawn(move || {
                for stream in self.0.incoming().filter_map(Result::ok) {
                    answer(stream, || {
                        requested.request();
                        waker();
                    });
                }
            });
        activation
    }
}

/// Reads one request from `stream`; on "activate" calls `activate`, then
/// answers.
fn answer(stream: Stream, activate: impl FnOnce()) {
    if stream.set_recv_timeout(Some(TIMEOUT)).is_err() {
        return;
    }
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_ok() && line.trim_end() == ACTIVATE {
        activate();
        let _ = writeln!(reader.get_mut(), "{OK}");
    }
}

/// A short name for `canonical`, the same for every launch on it.
fn path_hash(canonical: &Path) -> String {
    let hash = blake3::hash(canonical.as_os_str().as_encoded_bytes());
    hash.to_hex()[..32].to_owned()
}

/// The socket file for `canonical`: in the user's runtime directory where
/// there is one, else in the temporary directory.
#[cfg(unix)]
fn address(canonical: &Path) -> Option<Address> {
    let hash = path_hash(canonical);
    Some(match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) => PathBuf::from(runtime)
            .join("noido")
            .join(format!("{hash}.sock")),
        None => std::env::temp_dir().join(format!("noido-{hash}.sock")),
    })
}

/// A named pipe for `canonical`, per user: pipe names are machine-wide.
#[cfg(windows)]
fn address(canonical: &Path) -> Option<Address> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    Some(format!("noido-{user}-{}", path_hash(canonical)))
}

#[cfg(unix)]
fn to_name(address: &Address) -> std::io::Result<Name<'_>> {
    address.as_path().to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn to_name(address: &Address) -> std::io::Result<Name<'_>> {
    address.as_str().to_ns_name::<GenericNamespaced>()
}

/// Creates the socket file's directory.
#[cfg(unix)]
fn make_room(address: &Address) {
    if let Some(dir) = address.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
}

#[cfg(windows)]
fn make_room(_address: &Address) {}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use interprocess::local_socket::{GenericFilePath, ListenerOptions};

    use super::*;

    fn no_waker() -> Waker {
        Arc::new(|| {})
    }

    #[test]
    fn second_claim_hands_over() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let Claim::First(listener) = claim_at(&address, TIMEOUT) else {
            panic!("first claim");
        };
        let activation = listener.serve(no_waker());
        assert!(!activation.take());
        assert!(matches!(claim_at(&address, TIMEOUT), Claim::Running));
        assert!(activation.take());
        assert!(!activation.take(), "taken twice");
    }

    #[test]
    fn different_path_is_first() {
        let dir = tempfile::TempDir::new().unwrap();
        let Claim::First(a) = claim_at(&dir.path().join("a.sock"), TIMEOUT) else {
            panic!("a");
        };
        let _a = a.serve(no_waker());
        assert!(matches!(
            claim_at(&dir.path().join("b.sock"), TIMEOUT),
            Claim::First(_)
        ));
    }

    #[test]
    fn stale_socket_is_taken_over() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        std::fs::write(&address, "").unwrap();
        assert!(matches!(claim_at(&address, TIMEOUT), Claim::First(_)));
    }

    #[test]
    fn silent_listener_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        let silent = ListenerOptions::new().name(name).create_sync().unwrap();
        // Accepts and never answers.
        std::thread::spawn(move || {
            let _held: Vec<_> = silent.incoming().collect();
        });
        let start = Instant::now();
        let claim = claim_at(&address, Duration::from_millis(200));
        assert!(matches!(claim, Claim::Unavailable));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn address_is_short_and_per_path() {
        let a = address(Path::new("/papers/a.pdf")).unwrap();
        let b = address(Path::new("/papers/b.pdf")).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, address(Path::new("/papers/a.pdf")).unwrap());
        // Unix socket paths are limited to about 100 bytes.
        assert!(a.as_os_str().len() < 100, "{a:?}");
    }
}
