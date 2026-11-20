//! The Soloist daemon: finding its program, starting it, and the socket to it.
//!
//! One worker thread runs the state machine and owns everything that blocks:
//! the process, the port file and the socket. The sound is started on the
//! caller's runtime, so the daemon's thread never needs one of its own.

pub mod daemon;
pub mod desk;
pub mod launch;
pub mod locate;
pub mod scan;
pub mod socket;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use takofy_core::credentials::{SoloistKey, SoloistKeyStore};
use takofy_core::playback::{ExternalSound, Sound};
use takofy_core::soloist::discovery;
use takofy_core::soloist::expiry::{self, Build};
use takofy_core::soloist::frames::{Command, Frame, decode};
use takofy_core::soloist::{Connection, SoloistFault, SoloistSound};
use takofy_ui::message::{DaemonState, Freshness, SoloistUpdate, Update};

use daemon::{Action, Daemon, Event, SPAWN_WAIT, State};

/// The log target this module's lines are filed under.
pub const SOLOIST_LOG_TARGET: &str = "takofy::soloist";

/// The file the daemon names its socket's port in, beside its data.
const PORT_FILE: &str = "ws.port";
/// The file it names the address in.
const ADDR_FILE: &str = "ws.addr";
/// The file it names its own process in.
const PID_FILE: &str = "soloist.pid";

/// How often the loop looks at the process and the clock.
const POLL: Duration = Duration::from_millis(250);

/// How long the daemon has to answer who it is once a socket is open.
const AUTH_WAIT: Duration = Duration::from_secs(2);

/// The thread's name, which is also how a warning says who it is.
const THREAD: &str = "takofy-soloist";

/// Where the machine publishes what it knows.
///
/// It is a closure rather than the bridge's notifier so this module owes the
/// bridge nothing: the answer leaves on the same queue as the daemon's others,
/// on a thread of its own.
pub type Sink = Arc<dyn Fn(Update) + Send + Sync>;

/// What the host reads from the preferences: the engine's own values only.
#[derive(Debug, Clone)]
pub struct SoloistPrefs {
    /// The program the person named, if they named one.
    pub path: Option<PathBuf>,
    /// The device name this window's sound announces itself under.
    pub name: String,
}

/// The daemon, and the sounds it hands out.
pub struct SoloistHost {
    requests: Sender<Request>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

enum Request {
    /// Asks for a connection that is ready to play.
    Sound {
        reply: SyncSender<Result<Connection, SoloistFault>>,
    },
    /// Names where to publish what the machine knows, from now on.
    Watch(Sink),
    /// Starts the machine over from a new pin or key.
    Recheck(Option<PathBuf>),
    /// Lets go of the data directory, for the one job that needs it alone.
    Free,
    /// Takes the data directory back once that job is done.
    Resume,
    Shutdown,
}

impl SoloistHost {
    /// Starts the worker: locate, read the key, check the build, spawn,
    /// connect.
    pub fn start(
        handle: tokio::runtime::Handle,
        prefs: SoloistPrefs,
        store: Arc<dyn SoloistKeyStore>,
        data_dir: PathBuf,
    ) -> Self {
        let (requests, inbox) = mpsc::channel();
        let worker = Worker::new(handle, prefs, store, data_dir);
        let thread = std::thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || worker.run(inbox));
        Self {
            requests,
            thread: Mutex::new(thread.ok()),
        }
    }

    /// Waits for a daemon that can play, and starts a sound on it.
    ///
    /// Bounded by the wait for a socket: a machine with no daemon answers
    /// with the fault that names why, not with a hang.
    pub fn sound(&self, volume: u16) -> Result<ExternalSound, SoloistFault> {
        let (reply, answer) = mpsc::sync_channel(1);
        if self.requests.send(Request::Sound { reply }).is_err() {
            return Err(SoloistFault::NotRunning);
        }
        let connection = match answer.recv_timeout(SPAWN_WAIT) {
            Ok(Ok(connection)) => connection,
            Ok(Err(fault)) => return Err(fault),
            Err(_) => return Err(SoloistFault::NotRunning),
        };
        let (sound, events) = SoloistSound::start(connection, volume);
        Ok((Box::new(sound) as Box<dyn Sound>, events))
    }

    /// Names the place the machine publishes its state, and publishes the one
    /// it already has.
    pub fn watch(&self, sink: Sink) {
        let _ = self.requests.send(Request::Watch(sink));
    }

    /// Starts the machine over, with a new pin in the preferences if one is
    /// given.
    ///
    /// A live daemon is left alone: it owns the process a sound is already
    /// playing through, so the change waits for the next start.
    pub fn recheck(&self, path: Option<PathBuf>) {
        let _ = self.requests.send(Request::Recheck(path));
    }

    /// Stops the daemon so another process may use the data directory.
    ///
    /// Only one Soloist may hold a data directory at a time — a second one
    /// exits with a general failure — and pairing is the other one. The worker
    /// keeps running, so it can take the directory back afterwards.
    pub fn free(&self) {
        let _ = self.requests.send(Request::Free);
    }

    /// Takes the data directory back, and starts the daemon again.
    pub fn resume(&self) {
        let _ = self.requests.send(Request::Resume);
    }

    /// Waits up to `wait` for the daemon to let go of the data directory.
    ///
    /// The worker kills the child and reaps it before it reads another request,
    /// so this only waits for the worker's turn.
    pub fn freed(&self, data_dir: &Path, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        loop {
            let alive = std::fs::read_to_string(data_dir.join(PID_FILE))
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
                .is_some_and(pid_alive);
            if !alive {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stops the worker, and the daemon only if this window started it.
    pub fn shutdown(&self) {
        let _ = self.requests.send(Request::Shutdown);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

/// Everything the daemon needs, owned by the thread that drives it.
struct Worker {
    handle: tokio::runtime::Handle,
    prefs: SoloistPrefs,
    store: Arc<dyn SoloistKeyStore>,
    data_dir: PathBuf,
    daemon: Daemon,
    /// The program, once it was found.
    located: Option<PathBuf>,
    /// How usable its build is.
    build: Build,
    /// The key, kept for every restart in this session.
    key: Option<SoloistKey>,
    /// The process, when it is one we started.
    child: Option<Child>,
    /// A socket already asked who it is, held for the next sound.
    connection: Option<Connection>,
    /// The sound request the machine is still working towards.
    pending: Option<SyncSender<Result<Connection, SoloistFault>>>,
    /// Where the window hears what the machine knows, once it asked.
    sink: Option<Sink>,
    /// What was published last, so a repeat is not sent.
    published: Option<Snapshot>,
    /// Whether another process holds the data directory right now.
    paused: bool,
}

impl Worker {
    fn new(
        handle: tokio::runtime::Handle,
        prefs: SoloistPrefs,
        store: Arc<dyn SoloistKeyStore>,
        data_dir: PathBuf,
    ) -> Self {
        let daemon = Daemon::new(&prefs.name);
        Self {
            handle,
            prefs,
            store,
            data_dir,
            daemon,
            located: None,
            build: Build::Unknown,
            key: None,
            child: None,
            connection: None,
            pending: None,
            sink: None,
            published: None,
            paused: false,
        }
    }

    fn run(mut self, requests: Receiver<Request>) {
        self.begin();
        loop {
            match requests.recv_timeout(POLL) {
                Ok(Request::Sound { reply }) => {
                    // A person asking for sound is not the machine retrying.
                    self.daemon.retry();
                    self.pending = Some(reply);
                }
                Ok(Request::Watch(sink)) => {
                    self.sink = Some(sink);
                    // The sink is new, so whatever it holds is news to it.
                    self.published = None;
                }
                Ok(Request::Recheck(path)) => self.recheck(path),
                Ok(Request::Free) => {
                    self.stop_child();
                    self.paused = true;
                }
                Ok(Request::Resume) => {
                    self.paused = false;
                    self.daemon.retry();
                }
                Ok(Request::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            self.daemon.event(Event::Tick, now);
            self.watch_child();
            self.advance(now);
            self.serve();
            self.settle();
            self.publish();
        }
        self.stop_child();
    }

    /// Finds the program, reads the key and checks the build once, because
    /// neither changes while this window is open.
    fn begin(&mut self) {
        let found = locate::find(
            self.prefs.path.as_deref(),
            std::env::var_os("PATH").as_deref(),
            locate::exists,
        );
        let Some(exe) = found.path().map(Path::to_path_buf) else {
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: not found");
            return;
        };
        self.build = launch::Launch::version(&exe)
            .map(|output| expiry::classify(&output, now_unix()))
            .unwrap_or(Build::Unknown);
        if !self.build.playable() {
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: build expired");
            self.daemon.expire();
            return;
        }
        self.located = Some(exe);
        self.key = self.store.load().unwrap_or_default();
        if self.key.is_none() {
            // Without a key the daemon cannot be signed in, and pairing is
            // the flag that fixes it, not a restart.
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: no key stored");
            self.daemon.event(
                Event::AuthState {
                    logged_in: false,
                    device_name: String::new(),
                },
                Instant::now(),
            );
            return;
        }
        self.daemon.event(Event::Located, Instant::now());
    }

    /// Starts over from a new pin or key, when that disturbs nothing.
    fn recheck(&mut self, path: Option<PathBuf>) {
        if !recheck_allowed(self.daemon.state()) {
            return;
        }
        if let Some(path) = path {
            self.prefs.path = Some(path);
        }
        self.stop_child();
        self.connection = None;
        self.daemon = Daemon::new(&self.prefs.name);
        self.located = None;
        self.build = Build::Unknown;
        self.key = None;
        self.begin();
    }

    /// Tells the window where the machine is, when that is news.
    fn publish(&mut self) {
        let Some(sink) = self.sink.clone() else {
            return;
        };
        let snapshot = snapshot(self.daemon.state(), self.located.as_deref(), &self.build);
        if self.published.as_ref() == Some(&snapshot) {
            return;
        }
        let state = snapshot.daemon;
        // A state change is the only evidence a listener's machine gives about
        // why nothing plays; it is a kind, never a path, a key or a URI.
        if self.published.as_ref().map(|last| last.daemon) != Some(state) {
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: {state:?}");
        }
        self.published = Some(snapshot);
        sink(Update::Soloist(SoloistUpdate::Daemon(state)));
    }

    fn watch_child(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let Ok(Some(status)) = child.try_wait() else {
            return;
        };
        let code = status.code().unwrap_or(-1);
        self.child = None;
        log::info!(target: SOLOIST_LOG_TARGET, "soloist: exited {code}");
        self.daemon.event(Event::Exited(code), Instant::now());
    }

    fn advance(&mut self, now: Instant) {
        // While another process has the data directory, this one must not take
        // it: a spawn now would exit at once and be counted as a failure.
        if self.paused {
            return;
        }
        match self.daemon.action(now) {
            Action::None => {}
            Action::Spawn => self.spawn(now),
            Action::Connect => self.connect(now),
        }
    }

    fn spawn(&mut self, now: Instant) {
        let (Some(exe), Some(key)) = (self.located.as_deref(), self.key.as_ref()) else {
            return;
        };
        let started = std::fs::create_dir_all(&self.data_dir).and_then(|()| {
            launch::Launch::new(exe, &self.prefs.name, key, &self.data_dir)
                .spawn()
                .map_err(|_| std::io::Error::other("the daemon did not start"))
        });
        match started {
            Ok(child) => {
                log::info!(target: SOLOIST_LOG_TARGET, "soloist: started");
                self.child = Some(child);
                self.daemon.event(Event::Spawned, now);
            }
            Err(_) => {
                // An attempt that never became a process counts like one that
                // ended: the backoff is what stops a loop of them.
                log::info!(target: SOLOIST_LOG_TARGET, "soloist: not started");
                self.daemon.event(Event::Exited(0), now);
            }
        }
    }

    fn connect(&mut self, now: Instant) {
        // After a general failure the only evidence is a process that is
        // there and a socket that answers; with no live pid there is nothing
        // to ask, and the window closes on its own.
        if self.daemon.awaiting_evidence() && !self.live_pid() {
            return;
        }
        let Some(endpoint) = self.endpoint() else {
            return;
        };
        self.daemon.event(Event::PortRead, now);
        let Ok(mut connection) = self.handle.block_on(socket::connect(endpoint)) else {
            return;
        };
        self.daemon.event(Event::Connected, Instant::now());
        let Some((logged_in, device_name)) = self.handle.block_on(auth_of(&mut connection)) else {
            return;
        };
        self.daemon.event(
            Event::AuthState {
                logged_in,
                device_name,
            },
            Instant::now(),
        );
        if self.daemon.state().playable() {
            self.connection = Some(connection);
        }
    }

    /// Answers the sound request, or keeps it while the machine works.
    fn serve(&mut self) {
        let Some(reply) = self.pending.take() else {
            return;
        };
        let state = self.daemon.state();
        if let Some(fault) = state.fault() {
            let _ = reply.send(Err(fault));
            return;
        }
        if !state.playable() {
            self.pending = Some(reply);
            return;
        }
        if self.connection.is_none() && !self.reconnect() {
            // The socket that answered before is gone: the machine starts
            // over, and the request waits for the daemon it brings up.
            self.pending = Some(reply);
            self.daemon.event(Event::Exited(0), Instant::now());
            return;
        }
        match self.connection.take() {
            Some(connection) => {
                let _ = reply.send(Ok(connection));
            }
            None => {
                let _ = reply.send(Err(SoloistFault::Lost));
            }
        }
    }

    /// Opens one more socket to a daemon the machine already trusts.
    fn reconnect(&mut self) -> bool {
        let Some(endpoint) = self.endpoint() else {
            return false;
        };
        let Ok(mut connection) = self.handle.block_on(socket::connect(endpoint)) else {
            return false;
        };
        let Some((logged_in, device_name)) = self.handle.block_on(auth_of(&mut connection)) else {
            return false;
        };
        let ours = takofy_core::connect::is_own_sound_device(&device_name, &self.prefs.name);
        if !(logged_in && ours) {
            return false;
        }
        self.connection = Some(connection);
        true
    }

    /// Closes what a settled machine no longer needs.
    fn settle(&mut self) {
        if self.daemon.state().keeps_child() {
            return;
        }
        self.connection = None;
        self.stop_child();
    }

    /// Kills the process, and only the one this window started.
    fn stop_child(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        let _ = child.wait();
    }

    fn endpoint(&self) -> Option<SocketAddr> {
        let port = std::fs::read_to_string(self.data_dir.join(PORT_FILE)).ok()?;
        let addr = std::fs::read_to_string(self.data_dir.join(ADDR_FILE)).unwrap_or_default();
        discovery::endpoint(&addr, &port)
    }

    /// Whether another instance is alive, which is the case a general failure
    /// cannot rule out.
    fn live_pid(&self) -> bool {
        let Ok(text) = std::fs::read_to_string(self.data_dir.join(PID_FILE)) else {
            return false;
        };
        match text.trim().parse::<u32>() {
            Ok(pid) => pid_alive(pid),
            Err(_) => false,
        }
    }
}

/// What the window was told last.
///
/// The state is what crosses; the program and the build are here so a restart
/// that found the same program is not announced twice.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    daemon: DaemonState,
    program: Option<PathBuf>,
    build: Freshness,
}

/// The snapshot of a machine in `state`, holding `program`.
fn snapshot(state: State, program: Option<&Path>, build: &Build) -> Snapshot {
    Snapshot {
        daemon: mirror(state),
        program: program.map(Path::to_path_buf),
        build: freshness(*build),
    }
}

/// Where the window draws the machine's own state.
fn mirror(state: State) -> DaemonState {
    match state {
        State::Missing => DaemonState::Missing,
        State::NotRunning => DaemonState::NotRunning,
        State::Starting => DaemonState::Starting,
        State::NoSocket => DaemonState::NoSocket,
        State::Unpaired => DaemonState::Unpaired,
        State::Expired => DaemonState::Expired,
        State::Ready => DaemonState::Ready,
        State::Adopted => DaemonState::Adopted,
    }
}

/// Whether a restart may be asked for now.
///
/// A live machine owns the process a sound is already playing through, so a new
/// pin or key waits for the next start rather than stopping it mid-track.
fn recheck_allowed(state: State) -> bool {
    may_restart(mirror(state))
}

/// The same rule, for a caller that only knows the mirror.
pub(crate) fn may_restart(daemon: DaemonState) -> bool {
    matches!(
        daemon,
        DaemonState::Missing
            | DaemonState::NotRunning
            | DaemonState::NoSocket
            | DaemonState::Unpaired
            | DaemonState::Expired
    )
}

/// How usable a build is, as the window draws it.
pub(crate) fn freshness(build: Build) -> Freshness {
    match build {
        Build::Unknown => Freshness::Unknown,
        Build::Fresh => Freshness::Fresh,
        Build::Expiring { days_left } => Freshness::Expiring {
            days_left: u32::try_from(days_left.max(0)).unwrap_or(u32::MAX),
        },
        Build::Expired => Freshness::Expired,
    }
}

/// Asks the daemon who it is, inside the answer's budget.
async fn auth_of(connection: &mut Connection) -> Option<(bool, String)> {
    connection
        .outgoing
        .send(Command::GetAuthState.encode())
        .ok()?;
    let deadline = tokio::time::Instant::now() + AUTH_WAIT;
    let mut answered = None;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Some(text)) = tokio::time::timeout(left, connection.incoming.recv()).await else {
            return answered;
        };
        if let Frame::AuthState {
            logged_in,
            device_name,
            ..
        } = decode(&text)
        {
            if logged_in {
                return Some((true, device_name));
            }
            // The daemon listens before it has restored the session, so "no
            // account" is an answer that may be withdrawn a moment later.
            answered = Some((logged_in, device_name));
        }
    }
}

/// Whether a process named by a pid file is still there.
#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Whether a process named by a pid file is still there.
///
/// Without a way to ask, the file itself is the evidence; the socket and its
/// answer are the truth either way.
#[cfg(not(target_os = "linux"))]
fn pid_alive(_pid: u32) -> bool {
    true
}

/// The wall clock, in unix seconds, which build freshness is read against.
pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
