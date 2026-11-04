//! One window per file: a running noido listens on a local socket named
//! after its file's path, and a later launch on the same file hands over to
//! it, with the page or place to go to, and exits.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
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
    Running {
        /// Its process, when it said (for bringing it to the front).
        pid: Option<u32>,
    },
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
        return match hand_over(stream, timeout, target) {
            Some(pid) => Claim::Running { pid },
            None => Claim::Unavailable,
        };
    }
    match bind(address, name, timeout) {
        Some(listener) => Claim::First(Listener(listener)),
        None => Claim::Unavailable,
    }
}

/// Listens at `address`, replacing a socket file left by a crash: callers
/// have found nobody answering there.
fn bind(
    address: &Address,
    name: Name<'_>,
    timeout: Duration,
) -> Option<interprocess::local_socket::Listener> {
    make_room(address);
    ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .max_spin_time(timeout)
        .create_sync()
        .ok()
}

/// Writes `request` as a line to `stream` and reads the answer line,
/// which `parse` reads; `None` if it doesn't answer within `timeout`. The
/// exchange runs on a thread of its own, since named pipes on Windows
/// have no read timeout. A peer that never answers leaves the thread
/// waiting until this launch exits, moments later.
fn ask<T: Send + 'static>(
    stream: Stream,
    request: String,
    timeout: Duration,
    parse: fn(&str) -> Option<T>,
) -> Option<T> {
    let (answered_tx, answered) = mpsc::channel();
    let exchange = move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let answer = if writeln!(reader.get_mut(), "{request}").is_ok()
            && reader.read_line(&mut line).is_ok()
        {
            parse(line.trim_end())
        } else {
            None
        };
        let _ = answered_tx.send(answer);
    };
    thread::Builder::new()
        .name("noido-hand-over".into())
        .spawn(exchange)
        .ok()?;
    answered.recv_timeout(timeout).unwrap_or(None)
}

/// Accepts connections on `listener` on a thread of its own, handing each
/// to `handle` on a thread of its own, so one that never asks holds up
/// nobody else.
fn accept_loop(
    listener: interprocess::local_socket::Listener,
    name: &'static str,
    handle: impl Fn(Stream) + Clone + Send + 'static,
) {
    // Without the thread, later launches time out and open their own window.
    let _ = thread::Builder::new().name(name.into()).spawn(move || {
        for stream in listener.incoming().filter_map(Result::ok) {
            let handle = handle.clone();
            let _ = thread::Builder::new()
                .name("noido-answer".into())
                .spawn(move || handle(stream));
        }
    });
}

/// Reads one whole request line of at most `max` bytes from `stream`,
/// without its line end; the stream back with it, to answer on. A client
/// can't make the line grow without end.
fn read_request(stream: Stream, max: usize) -> Option<(String, Stream)> {
    // Unsupported on Windows; there a silent launch keeps only its own
    // thread waiting.
    let _ = stream.set_recv_timeout(Some(TIMEOUT));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = (&mut reader).take(max as u64).read_line(&mut line);
    if read.is_err() || !line.ends_with('\n') {
        return None;
    }
    line.truncate(line.trim_end_matches(['\r', '\n']).len());
    Some((line, reader.into_inner()))
}

/// Asks the noido at the other end of `stream` to come forward and go to
/// `target`; `None` if it doesn't answer within `timeout`, else the pid
/// it gave, if any.
fn hand_over(stream: Stream, timeout: Duration, target: Option<&Target>) -> Option<Option<u32>> {
    let request = match target {
        Some(target) => format!("{ACTIVATE} {}", to_fragment(target)),
        None => ACTIVATE.to_owned(),
    };
    ask(stream, request, timeout, parse_answer)
}

/// What an answer says: `None` if it isn't one, else the pid it names
/// (`ok 123`; a bare `ok` names none).
fn parse_answer(line: &str) -> Option<Option<u32>> {
    match line.strip_prefix(OK)? {
        "" => Some(None),
        rest => rest.strip_prefix(' ')?.parse().ok().map(Some),
    }
}

impl Listener {
    /// Answers later launches on a thread of its own; `waker` is called when
    /// one asks the window to come forward.
    pub fn serve(self, waker: Waker) -> Activation {
        let activation = Activation::default();
        let requested = activation.clone();
        accept_loop(self.0, "noido-instance", move |stream| {
            answer(stream, |target| {
                requested.request(target);
                waker();
            });
        });
        activation
    }
}

/// Reads one request from `stream`; on "activate" calls `activate` with
/// the target it names, then answers.
fn answer(stream: Stream, activate: impl FnOnce(Option<Target>)) {
    let Some((line, mut stream)) = read_request(stream, MAX_REQUEST) else {
        return;
    };
    let line = line.trim_end();
    let target = match line.strip_prefix(ACTIVATE) {
        Some("") => None,
        Some(rest) if rest.starts_with(' ') => parse_fragment(&rest[1..]),
        _ => return,
    };
    activate(target);
    let _ = writeln!(stream, "{OK} {}", std::process::id());
}

// One socket per executable: the macOS shell listens on it, and launches
// from a terminal hand their files to it.

/// The longest app request line read, in bytes.
const MAX_APP_REQUEST: usize = 4096;

/// How long a sender waits for the answer: a large file loads first.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// A request to open a file, with the place to go to.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // until the shell uses it
pub struct OpenRequest {
    pub path: PathBuf,
    pub target: Option<Target>,
}

/// What the app answers once it has tried the file.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // until the shell uses it
pub enum Answer {
    /// A new window was opened; `pid` is the app's process.
    Opened { pid: Option<u32> },
    /// The file was open already, and its window was brought forward.
    Forward { pid: Option<u32> },
    /// The file could not be opened.
    Failed(String),
}

/// The outcome of asking whether this executable's app runs already.
#[allow(dead_code)] // until the shell uses it
pub enum AppClaim {
    /// It does. `None`: it answered nothing in time.
    Running(Option<Answer>),
    /// It doesn't; this launch listens for later ones.
    First(AppListener),
    /// No socket could be had.
    Unavailable,
}

/// The socket of the app of an executable.
#[allow(dead_code)] // until the shell uses it
pub struct AppListener(interprocess::local_socket::Listener);

/// The way to answer one request; dropped unanswered, it closes the
/// connection and the sender sees no answer.
pub struct Reply(Stream);

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

impl Reply {
    /// Answers with `outcome`; its pid, if any, is this process.
    #[allow(dead_code)] // until the shell uses it
    pub fn send(mut self, outcome: Answer) {
        let pid = std::process::id();
        let line = match outcome {
            Answer::Opened { .. } => format!("opened {pid}"),
            Answer::Forward { .. } => format!("forward {pid}"),
            Answer::Failed(message) => format!("failed {}", one_line(&message)),
        };
        let _ = writeln!(self.0, "{line}");
    }
}

/// `text` on one line.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// What has arrived at an [`AppListener`] since it was last asked.
#[derive(Clone, Default)]
#[allow(dead_code)] // until the shell uses it
pub struct AppRequests(Arc<Mutex<Vec<(OpenRequest, Reply)>>>);

impl AppRequests {
    /// Everything that arrived since the last call, oldest first.
    #[allow(dead_code)] // until the shell uses it
    pub fn take(&self) -> Vec<(OpenRequest, Reply)> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// The request line for `request`.
fn app_request_line(request: &OpenRequest) -> String {
    let fragment = request.target.as_ref().map_or("-".to_owned(), to_fragment);
    format!("open {fragment} {}", request.path.to_string_lossy())
}

/// What a request line asks, if it is one.
fn parse_app_request(line: &str) -> Option<OpenRequest> {
    let rest = line.strip_prefix("open ")?;
    let (fragment, path) = rest.split_once(' ')?;
    if path.is_empty() {
        return None;
    }
    let target = match fragment {
        "-" => None,
        fragment => Some(parse_fragment(fragment)?),
    };
    Some(OpenRequest {
        path: PathBuf::from(path),
        target,
    })
}

/// What an answer line says, if it is one. A bare `opened` or `forward`
/// names no pid.
fn parse_app_answer(line: &str) -> Option<Answer> {
    let pid = |rest: &str| match rest {
        "" => Some(None),
        rest => rest.strip_prefix(' ')?.parse().ok().map(Some),
    };
    if let Some(rest) = line.strip_prefix("opened") {
        return Some(Answer::Opened { pid: pid(rest)? });
    }
    if let Some(rest) = line.strip_prefix("forward") {
        return Some(Answer::Forward { pid: pid(rest)? });
    }
    let message = line.strip_prefix("failed")?;
    match message {
        "" => Some(Answer::Failed(String::new())),
        message => Some(Answer::Failed(message.strip_prefix(' ')?.to_owned())),
    }
}

/// The socket of the app of `exe`: next to the per-file ones.
fn app_address(exe: &Path) -> Option<Address> {
    let canonical = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    named_address(&format!("noido-app-{}", path_hash(&canonical)))
}

/// Hands `request` to the app of `exe` if one runs, and waits for its
/// answer. With no `request` it only checks, answering `Running(None)` if
/// one runs. Otherwise this launch becomes the app's listener.
#[allow(dead_code)] // until the shell uses it
pub fn claim_app(exe: &Path, request: Option<&OpenRequest>) -> AppClaim {
    match app_address(exe) {
        Some(address) => claim_app_at(&address, TIMEOUT, ANSWER_TIMEOUT, request),
        None => AppClaim::Unavailable,
    }
}

/// [`claim_app`] for the socket at `address`.
fn claim_app_at(
    address: &Address,
    timeout: Duration,
    answer_timeout: Duration,
    request: Option<&OpenRequest>,
) -> AppClaim {
    let Ok(name) = to_name(address) else {
        return AppClaim::Unavailable;
    };
    if let Ok(stream) = Stream::connect(name.clone()) {
        return AppClaim::Running(request.and_then(|r| exchange(stream, r, answer_timeout)));
    }
    match bind(address, name, timeout) {
        Some(listener) => AppClaim::First(AppListener(listener)),
        None => AppClaim::Unavailable,
    }
}

/// Hands `request` to the app of `exe`, which must run already: never
/// binds. `None` when none listens, or none answers.
#[allow(dead_code)] // until the shell uses it
pub fn send(exe: &Path, request: &OpenRequest) -> Option<Answer> {
    send_at(&app_address(exe)?, request, ANSWER_TIMEOUT)
}

fn send_at(address: &Address, request: &OpenRequest, answer_timeout: Duration) -> Option<Answer> {
    let stream = Stream::connect(to_name(address).ok()?).ok()?;
    exchange(stream, request, answer_timeout)
}

/// Writes `request` to `stream` and reads the answer.
fn exchange(stream: Stream, request: &OpenRequest, answer_timeout: Duration) -> Option<Answer> {
    let line = app_request_line(request);
    if line.len() >= MAX_APP_REQUEST || line.contains(['\r', '\n']) {
        return None;
    }
    ask(stream, line, answer_timeout, parse_app_answer)
}

impl AppListener {
    /// Collects requests on a thread of its own, calling `waker` after
    /// each arrival. They are answered through their [`Reply`].
    #[allow(dead_code)] // until the shell uses it
    pub fn serve(self, waker: Waker) -> AppRequests {
        let requests = AppRequests::default();
        let arrived = requests.clone();
        accept_loop(self.0, "noido-app", move |stream| {
            if let Some(received) = read_app_request(stream) {
                arrived
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(received);
                waker();
            }
        });
        requests
    }
}

/// Reads one request line from `stream`, within the limit.
fn read_app_request(stream: Stream) -> Option<(OpenRequest, Reply)> {
    let (line, stream) = read_request(stream, MAX_APP_REQUEST)?;
    Some((parse_app_request(&line)?, Reply(stream)))
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

/// The socket called `name`, in the directory of the per-file ones.
#[cfg(unix)]
fn named_address(name: &str) -> Option<Address> {
    Some(match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) => PathBuf::from(runtime)
            .join("noido")
            .join(format!("{name}.sock")),
        None => std::env::temp_dir().join(format!("{name}.sock")),
    })
}

/// A named pipe for `canonical`, per user: pipe names are machine-wide.
#[cfg(windows)]
fn address(canonical: &Path) -> Option<Address> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    Some(format!("noido-{user}-{}", path_hash(canonical)))
}

/// The pipe called `name`, per user.
#[cfg(windows)]
fn named_address(name: &str) -> Option<Address> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    Some(format!("{name}-{user}"))
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
        assert!(matches!(
            claim_at(&address, TIMEOUT, None),
            Claim::Running { .. }
        ));
        assert_eq!(activation.take(), Some(Request { target: None }));
        assert!(activation.take().is_none(), "taken twice");
    }

    #[test]
    fn hand_over_reports_the_running_pid() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let Claim::First(listener) = claim_at(&address, TIMEOUT, None) else {
            panic!("first claim");
        };
        let _activation = listener.serve(no_waker());
        assert!(matches!(claim_at(&address, TIMEOUT, None),
            Claim::Running { pid: Some(p) } if p == std::process::id()));
    }

    #[test]
    fn a_bare_ok_still_counts_as_handed_over() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        let server = thread::spawn(move || {
            let stream = listener.incoming().next().unwrap().unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            writeln!(reader.get_mut(), "ok").unwrap();
        });
        assert!(matches!(
            claim_at(&address, TIMEOUT, None),
            Claim::Running { pid: None }
        ));
        server.join().unwrap();
    }

    #[test]
    fn answers_parse_leniently() {
        assert_eq!(parse_answer("ok"), Some(None));
        assert_eq!(parse_answer("ok 123"), Some(Some(123)));
        assert_eq!(parse_answer("ok x"), None);
        assert_eq!(parse_answer("ok 1 2"), None);
        assert_eq!(parse_answer("okay"), None);
        assert_eq!(parse_answer(""), None);
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
            Claim::Running { .. }
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
                Claim::Running { .. }
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

    fn request(path: &str, target: Option<Target>) -> OpenRequest {
        OpenRequest {
            path: path.into(),
            target,
        }
    }

    /// Serves `listener` on a thread that waits for one request, answers
    /// it with `outcome`, and returns what it received.
    fn serve_one(listener: AppListener, outcome: Answer) -> thread::JoinHandle<OpenRequest> {
        let requests = listener.serve(no_waker());
        thread::spawn(move || {
            let start = Instant::now();
            loop {
                if let Some((request, reply)) = requests.take().pop() {
                    reply.send(outcome);
                    return request;
                }
                assert!(start.elapsed() < Duration::from_secs(5), "no request");
                thread::sleep(Duration::from_millis(5));
            }
        })
    }

    fn first_app(address: &Address) -> AppListener {
        let AppClaim::First(listener) = claim_app_at(address, TIMEOUT, ANSWER_TIMEOUT, None) else {
            panic!("first claim");
        };
        listener
    }

    #[test]
    fn app_second_claim_sends_the_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let served = serve_one(first_app(&address), Answer::Opened { pid: None });
        let sent = request("/papers/a.pdf", Some(Target::Page(4)));
        let claim = claim_app_at(&address, TIMEOUT, ANSWER_TIMEOUT, Some(&sent));
        assert!(matches!(
            claim,
            AppClaim::Running(Some(Answer::Opened { pid: Some(p) })) if p == std::process::id()
        ));
        let got = served.join().unwrap();
        assert_eq!(got.path, PathBuf::from("/papers/a.pdf"));
        assert_eq!(got.target, Some(Target::Page(4)));
    }

    #[test]
    fn app_forward_answer_carries_the_pid() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let served = serve_one(first_app(&address), Answer::Forward { pid: None });
        let sent = request("/a.pdf", None);
        let answer = send_at(&address, &sent, ANSWER_TIMEOUT);
        assert!(matches!(answer,
            Some(Answer::Forward { pid: Some(p) }) if p == std::process::id()));
        assert_eq!(served.join().unwrap().target, None);
    }

    #[test]
    fn app_answer_carries_a_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let served = serve_one(first_app(&address), Answer::Failed("no such file".into()));
        let answer = send_at(&address, &request("/x.pdf", None), ANSWER_TIMEOUT);
        assert!(matches!(answer, Some(Answer::Failed(m)) if m == "no such file"));
        served.join().unwrap();
    }

    #[test]
    fn a_path_with_spaces_arrives_whole() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let served = serve_one(first_app(&address), Answer::Opened { pid: None });
        let sent = request(
            "/my papers/a b  c.pdf",
            Some(Target::Named("sec 2&b".into())),
        );
        assert!(send_at(&address, &sent, ANSWER_TIMEOUT).is_some());
        let got = served.join().unwrap();
        assert_eq!(got.path, PathBuf::from("/my papers/a b  c.pdf"));
        assert_eq!(got.target, Some(Target::Named("sec 2&b".into())));
    }

    #[test]
    fn a_claim_without_a_request_only_checks() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let listener = first_app(&address);
        let requests = listener.serve(no_waker());
        assert!(matches!(
            claim_app_at(&address, TIMEOUT, ANSWER_TIMEOUT, None),
            AppClaim::Running(None)
        ));
        thread::sleep(Duration::from_millis(100));
        assert!(requests.take().is_empty());
    }

    #[test]
    fn a_dropped_reply_leaves_the_sender_without_an_answer() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let requests = first_app(&address).serve(no_waker());
        let dropper = thread::spawn(move || {
            let start = Instant::now();
            while requests.take().is_empty() {
                assert!(start.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(5));
            }
        });
        let claim = claim_app_at(
            &address,
            TIMEOUT,
            ANSWER_TIMEOUT,
            Some(&request("/a.pdf", None)),
        );
        assert!(matches!(claim, AppClaim::Running(None)));
        dropper.join().unwrap();
    }

    #[test]
    fn the_waker_is_called_on_arrival() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        let waker: Waker = Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let requests = first_app(&address).serve(waker);
        let _ = send_at(
            &address,
            &request("/a.pdf", None),
            Duration::from_millis(200),
        );
        assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(requests.take().len(), 1);
    }

    #[test]
    fn another_executable_is_another_app() {
        let a = app_address(Path::new("/Applications/Noido.app/Contents/MacOS/noido")).unwrap();
        let b = app_address(Path::new("/tmp/build/noido")).unwrap();
        assert_ne!(a, b);
        assert_eq!(
            a,
            app_address(Path::new("/Applications/Noido.app/Contents/MacOS/noido")).unwrap()
        );
        assert!(a.as_os_str().len() < 100, "{a:?}");
        // Not the socket of a file of the same name.
        assert_ne!(
            a,
            address(Path::new("/Applications/Noido.app/Contents/MacOS/noido")).unwrap()
        );
    }

    #[test]
    fn send_without_an_app_is_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("none.sock");
        assert!(send_at(&address, &request("/a.pdf", None), ANSWER_TIMEOUT).is_none());
        assert!(!address.exists(), "send must not bind");
    }

    #[test]
    fn stale_app_socket_is_taken_over() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        std::fs::write(&address, "").unwrap();
        let claim = claim_app_at(&address, TIMEOUT, ANSWER_TIMEOUT, None);
        assert!(matches!(claim, AppClaim::First(_)));
    }

    #[test]
    fn an_overlong_app_request_is_ignored() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let requests = first_app(&address).serve(no_waker());
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        let mut reader = BufReader::new(Stream::connect(name).unwrap());
        let long = "a".repeat(MAX_APP_REQUEST);
        writeln!(reader.get_mut(), "open - /{long}").unwrap();
        let mut answer = String::new();
        let _ = reader.read_line(&mut answer);
        assert_eq!(answer, "", "answered");
        assert!(requests.take().is_empty());
    }

    #[test]
    fn a_silent_app_gives_no_answer_in_time() {
        let dir = tempfile::TempDir::new().unwrap();
        let address = dir.path().join("a.sock");
        let name = address.as_path().to_fs_name::<GenericFilePath>().unwrap();
        let _silent = ListenerOptions::new().name(name).create_sync().unwrap();
        let start = Instant::now();
        let claim = claim_app_at(
            &address,
            TIMEOUT,
            Duration::from_millis(200),
            Some(&request("/a.pdf", None)),
        );
        assert!(matches!(claim, AppClaim::Running(None)));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn app_answers_parse() {
        assert!(matches!(
            parse_app_answer("opened 7"),
            Some(Answer::Opened { pid: Some(7) })
        ));
        assert!(matches!(
            parse_app_answer("forward 7"),
            Some(Answer::Forward { pid: Some(7) })
        ));
        assert!(matches!(
            parse_app_answer("opened"),
            Some(Answer::Opened { pid: None })
        ));
        assert!(
            matches!(parse_app_answer("failed bad file"), Some(Answer::Failed(m)) if m == "bad file")
        );
        assert!(parse_app_answer("openedx").is_none());
        assert!(parse_app_answer("").is_none());
    }
}
