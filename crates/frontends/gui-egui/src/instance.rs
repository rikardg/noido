//! One window per file: a running noido listens on a local socket named
//! after its file's path, and a later launch on the same file hands over to
//! it, with the page or place to go to, and exits.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerOptions, Name, Stream, prelude::*};
use noido_core::{Target, parse_fragment, to_fragment};

use crate::watch::Waker;

/// The longest request line read, in bytes.
const MAX_REQUEST: usize = 512;

/// How long a launch waits for the running noido to answer.
const TIMEOUT: Duration = Duration::from_secs(1);

/// What a launch sends, then a space and the target's fragment if it has
/// one (`activate page=52`); and what the running noido answers.
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

/// What a later launch asked of the window.
#[derive(Debug, PartialEq)]
pub struct Request {
    /// Where to go, besides coming forward.
    pub target: Option<Target>,
}

/// Set when a later launch asked the window to come forward.
#[derive(Clone, Default)]
pub struct Activation(Arc<Mutex<Option<Request>>>);

impl Activation {
    /// What was asked since the last call, if anything.
    pub fn take(&self) -> Option<Request> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// Asks the window to come forward, and to go to `target`. A request
    /// without a target keeps the one still waiting.
    pub(crate) fn request(&self, target: Option<Target>) {
        let mut waiting = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let target = target.or_else(|| waiting.take().and_then(|r| r.target));
        *waiting = Some(Request { target });
    }
}

/// Hands over to the noido that has `path` open, if there is one, asking it
/// to go to `target`; otherwise claims the file for this launch. Doesn't
/// read the file.
pub fn claim(path: &Path, target: Option<&Target>) -> Claim {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match address(&canonical) {
        Some(address) => claim_at(&address, TIMEOUT, target),
        None => Claim::Unavailable,
    }
}

/// [`claim`] for the socket at `address`, waiting up to `timeout` for an
/// answer.
fn claim_at(address: &Address, timeout: Duration, target: Option<&Target>) -> Claim {
    let Ok(name) = to_name(address) else {
        return Claim::Unavailable;
    };
    if let Ok(stream) = Stream::connect(name.clone()) {
        // A noido that doesn't answer can't be displaced either.
        return if hand_over(stream, timeout, target) {
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

/// Asks the noido at the other end of `stream` to come forward and go to
/// `target`; false if it doesn't answer within `timeout`. The exchange runs on a thread of its
/// own, since named pipes on Windows have no read timeout.
fn hand_over(stream: Stream, timeout: Duration, target: Option<&Target>) -> bool {
    let request = match target {
        Some(target) => format!("{ACTIVATE} {}", to_fragment(target)),
        None => ACTIVATE.to_owned(),
    };
    let (answered_tx, answered) = mpsc::channel();
    let exchange = move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let ok = writeln!(reader.get_mut(), "{request}").is_ok()
            && reader.read_line(&mut line).is_ok()
            && line.trim_end() == OK;
        let _ = answered_tx.send(ok);
    };
    // A noido that never answers leaves the thread waiting until this
    // launch exits, moments later.
    if thread::Builder::new()
        .name("noido-hand-over".into())
        .spawn(exchange)
        .is_err()
    {
        return false;
    }
    answered.recv_timeout(timeout).unwrap_or(false)
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
                    let requested = requested.clone();
                    let waker = waker.clone();
                    // One thread per launch, so one that never asks holds
                    // up nobody else.
                    let _ = thread::Builder::new()
                        .name("noido-answer".into())
                        .spawn(move || {
                            answer(stream, |target| {
                                requested.request(target);
                                waker();
                            });
                        });
                }
            });
        activation
    }
}

/// Reads one request from `stream`; on "activate" calls `activate` with
/// the target it names, then answers.
fn answer(stream: Stream, activate: impl FnOnce(Option<Target>)) {
    // Unsupported on Windows; there a silent launch keeps only its own
    // thread waiting.
    let _ = stream.set_recv_timeout(Some(TIMEOUT));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    // A whole line within the limit, or nothing: a client can't make the
    // line grow without end.
    let read = (&mut reader).take(MAX_REQUEST as u64).read_line(&mut line);
    if read.is_err() || !line.ends_with('\n') {
        return;
    }
    let line = line.trim_end();
    let target = match line.strip_prefix(ACTIVATE) {
        Some("") => None,
        Some(rest) if rest.starts_with(' ') => parse_fragment(&rest[1..]),
        _ => return,
    };
    activate(target);
    let _ = writeln!(reader.get_mut(), "{OK}");
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

    use interprocess::local_socket::{GenericFilePath, ListenerOptions, Stream};

    use noido_core::Target;

    use super::*;

    fn no_waker() -> Waker {
        Arc::new(|| {})
    }

    #[test]
    fn second_claim_hands_over() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let Claim::First(listener) = claim_at(&address, TIMEOUT, None) else {
            panic!("first claim");
        };
        let activation = listener.serve(no_waker());
        assert!(activation.take().is_none());
        assert!(matches!(claim_at(&address, TIMEOUT, None), Claim::Running));
        assert_eq!(activation.take(), Some(Request { target: None }));
        assert!(activation.take().is_none(), "taken twice");
    }

    #[test]
    fn different_path_is_first() {
        let dir = tempfile::TempDir::new().unwrap();
        let Claim::First(a) = claim_at(&dir.path().join("a.sock"), TIMEOUT, None) else {
            panic!("a");
        };
        let _a = a.serve(no_waker());
        assert!(matches!(
            claim_at(&dir.path().join("b.sock"), TIMEOUT, None),
            Claim::First(_)
        ));
    }

    #[test]
    fn stale_socket_is_taken_over() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        std::fs::write(&address, "").unwrap();
        assert!(matches!(claim_at(&address, TIMEOUT, None), Claim::First(_)));
    }

    #[test]
    fn silent_listener_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        // Never accepts, so a connection waits unanswered in its backlog.
        let _silent = ListenerOptions::new().name(name).create_sync().unwrap();
        let start = Instant::now();
        let claim = claim_at(&address, Duration::from_millis(200), None);
        assert!(matches!(claim, Claim::Unavailable));
        let waited = start.elapsed();
        assert!(
            waited >= Duration::from_millis(200) && waited < Duration::from_secs(1),
            "waited {waited:?}"
        );
    }

    /// A launch that connects and never asks mustn't hold up the next one.
    #[test]
    fn silent_client_does_not_block_others() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let Claim::First(listener) = claim_at(&address, TIMEOUT, None) else {
            panic!("first claim");
        };
        let _activation = listener.serve(no_waker());
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        let _silent = Stream::connect(name).unwrap();
        assert!(matches!(
            claim_at(&address, Duration::from_millis(300), None),
            Claim::Running
        ));
    }

    #[test]
    fn an_overlong_request_is_ignored() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let Claim::First(listener) = claim_at(&address, TIMEOUT, None) else {
            panic!("first claim");
        };
        let activation = listener.serve(no_waker());
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        let mut reader = BufReader::new(Stream::connect(name).unwrap());
        let long = "a".repeat(MAX_REQUEST);
        writeln!(reader.get_mut(), "{ACTIVATE} nameddest={long}").unwrap();
        let mut answer = String::new();
        let _ = reader.read_line(&mut answer);
        assert_eq!(answer, "", "answered");
        assert!(activation.take().is_none());
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

    /// What the noido on a fresh socket is asked after the launches with
    /// `targets` hand over to it.
    fn handed(targets: &[Option<Target>]) -> Option<Request> {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let Claim::First(listener) = claim_at(&address, TIMEOUT, None) else {
            panic!("first claim");
        };
        let activation = listener.serve(no_waker());
        for target in targets {
            assert!(matches!(
                claim_at(&address, TIMEOUT, target.as_ref()),
                Claim::Running
            ));
        }
        activation.take()
    }

    #[test]
    fn hand_over_carries_the_page() {
        assert_eq!(
            handed(&[Some(Target::Page(51))]),
            Some(Request {
                target: Some(Target::Page(51))
            })
        );
    }

    #[test]
    fn hand_over_carries_a_named_place() {
        let named = Target::Named("sec 2&b".into());
        assert_eq!(
            handed(&[Some(named.clone())]),
            Some(Request {
                target: Some(named)
            })
        );
    }

    #[test]
    fn last_hand_over_wins() {
        let page = |p| Some(Target::Page(p));
        assert_eq!(
            handed(&[page(1), page(5)]),
            Some(Request { target: page(5) })
        );
        assert_eq!(handed(&[page(5), None]), Some(Request { target: page(5) }));
    }
}
