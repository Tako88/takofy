//! What the settings page's steps do: finding the program, storing the key,
//! pairing, and opening the two pages the setup names.
//!
//! The daemon's thread owns the process and the socket; this one owns what the
//! page asks beside it. A scan, a keychain write and a pairing child all block,
//! so each runs on a thread of its own and answers through the same sink the
//! daemon's state takes. Reading the key back out of the keychain is the one
//! thing that must not linger: it is saved and dropped.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use takofy_core::credentials::{SoloistKey, SoloistKeyStore, StoreError};
use takofy_core::soloist::expiry::{self, Build};
use takofy_model::home::Day;
use takofy_ui::message::{
    DaemonState, Facts, Freshness, KeyState, PairEnd, PairFault, SetupFault, SoloistAsk,
    SoloistPage, SoloistUpdate, Update,
};
use takofy_ui::secret::Secret;

use super::launch::Launch;
use super::scan::{self, Disk, Scan};
use super::{SOLOIST_LOG_TARGET, Sink, SoloistHost, freshness, may_restart, now_unix};

/// How often the pairing child is asked whether it is still there.
const PAIR_POLL: Duration = Duration::from_millis(250);

/// The thread a scan runs on.
const SCAN_THREAD: &str = "takofy-soloist-scan";
/// The thread a key is stored on.
const STORE_THREAD: &str = "takofy-soloist-key";
/// The thread a pairing child is waited on.
const PAIR_THREAD: &str = "takofy-soloist-pair";

/// Where Spotify Soloist is downloaded.
const DOWNLOAD_PAGE: &str =
    "https://developer.spotify.com/documentation/soloist/reference/downloads-and-updates";
/// Where the listener's own key is made.
const KEY_PAGE: &str = "https://developer.spotify.com/dashboard/soloist";

/// One scan at a time, with the newest request waiting behind the running one.
///
/// The page opening, a click on "Look again" and a dropped file can arrive in
/// one frame. One `--version` child runs at a time, and the request that was
/// asked for last is the one that answers, so a burst ends where the listener
/// pointed.
#[derive(Debug, Default)]
pub(crate) struct Scans {
    running: bool,
    queued: Option<Option<String>>,
}

impl Scans {
    /// Notes a request, and answers whether the caller starts it now.
    pub(crate) fn request(&mut self, path: Option<String>) -> bool {
        if self.running {
            self.queued = Some(path);
            return false;
        }
        self.running = true;
        true
    }

    /// Notes that a scan ended, and answers what to start next.
    pub(crate) fn finished(&mut self) -> Option<Option<String>> {
        match self.queued.take() {
            Some(path) => Some(path),
            None => {
                self.running = false;
                None
            }
        }
    }
}

/// The steps the settings page asks for, run beside the daemon.
pub struct Desk {
    store: Arc<dyn SoloistKeyStore>,
    data_dir: PathBuf,
    name: String,
    host: Option<Arc<SoloistHost>>,
    slot: Arc<Mutex<Slot>>,
}

/// What several threads touch: what the page was told, and what is running.
struct Slot {
    sink: Option<Sink>,
    /// Where the machine last said it was, which decides what may be asked of
    /// it without stopping it.
    daemon: DaemonState,
    /// The program the last scan found, which is what pairing runs.
    program: Option<PathBuf>,
    /// The facts the page was last told, so a stored key can be published
    /// against them.
    facts: Option<Facts>,
    scans: Scans,
    /// Whether a pairing child is running, and the flag that stops it.
    pairing: Option<Arc<AtomicBool>>,
}

impl Desk {
    /// A desk that answers for one window's setup.
    ///
    /// The host is absent under the built-in engine: the setup can still be
    /// prepared, and a stored pin or key takes effect at the next start.
    pub fn new(
        store: Arc<dyn SoloistKeyStore>,
        data_dir: PathBuf,
        name: String,
        host: Option<Arc<SoloistHost>>,
    ) -> Self {
        Self {
            store,
            data_dir,
            name,
            host,
            slot: Arc::new(Mutex::new(Slot {
                sink: None,
                daemon: DaemonState::NotRunning,
                program: None,
                facts: None,
                scans: Scans::default(),
                pairing: None,
            })),
        }
    }

    /// Carries out one step the page asked for.
    ///
    /// The sink is taken on the first call, which is also what makes the
    /// machine publish the state it already has.
    pub fn ask(&self, ask: SoloistAsk, sink: Sink) {
        self.install(sink);
        match ask {
            SoloistAsk::Scan { path } => self.scan(path),
            SoloistAsk::StoreKey(secret) => self.store_key(secret),
            SoloistAsk::Pair => self.pair(),
            SoloistAsk::StopPair => self.stop_pair(),
            SoloistAsk::Open(page) => open_page(page),
        }
    }

    /// Stops a pairing child, so a session that ends mid-pairing leaves none
    /// holding the data directory.
    pub fn shutdown(&self) {
        self.stop_pair();
    }

    /// Names where answers go before any step is asked.
    ///
    /// The page draws the machine's own state from the window's first frame, so
    /// the sink is taken at the start rather than by the first press.
    pub fn watch(&self, sink: Sink) {
        self.install(sink);
    }

    /// Notes where answers go, and asks the machine to say where it is.
    fn install(&self, sink: Sink) {
        let host = {
            let mut slot = self.lock();
            if slot.sink.is_some() {
                return;
            }
            slot.sink = Some(sink.clone());
            self.host.clone()
        };
        let Some(host) = host else {
            return;
        };
        // The machine's own word comes through the same sink, which is how the
        // desk learns what may be restarted without stopping a sound.
        let slot = Arc::clone(&self.slot);
        let forward = sink;
        let remember: Sink = Arc::new(move |update: Update| {
            if let Update::Soloist(SoloistUpdate::Daemon(state)) = &update {
                guard(&slot).daemon = *state;
            }
            forward(update);
        });
        host.watch(remember);
    }

    /// Asks for the program, on a thread that may touch the disk.
    fn scan(&self, path: Option<String>) {
        let mut slot = self.lock();
        if !slot.scans.request(path.clone()) {
            return;
        }
        drop(slot);
        self.start_scan(path);
    }

    /// Runs one scan, and then whatever arrived while it ran.
    fn start_scan(&self, path: Option<String>) {
        let slot = Arc::clone(&self.slot);
        let store = Arc::clone(&self.store);
        let host = self.host.clone();
        let spawned = thread::Builder::new()
            .name(SCAN_THREAD.to_owned())
            .spawn(move || {
                let mut path = path;
                loop {
                    run_scan(&slot, store.as_ref(), host.as_ref(), path);
                    match guard(&slot).scans.finished() {
                        Some(next) => path = next,
                        None => break,
                    }
                }
            });
        if spawned.is_err() {
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: the scan did not start");
            guard(&self.slot).scans.finished();
        }
    }

    /// Saves the pasted key, then forgets it.
    fn store_key(&self, secret: Secret) {
        let Some(key) = SoloistKey::new(secret.into_inner()) else {
            self.publish(SoloistUpdate::Fault(SetupFault::KeyEmpty));
            return;
        };
        let slot = Arc::clone(&self.slot);
        let store = Arc::clone(&self.store);
        let host = self.host.clone();
        let spawned = thread::Builder::new()
            .name(STORE_THREAD.to_owned())
            .spawn(move || {
                if let Err(error) = store.save(&key) {
                    send(&slot, SoloistUpdate::Fault(fault_of(&error)));
                    return;
                }
                // The keychain holds it from here; this thread lets it go.
                drop(key);
                let facts = remember_key(&slot);
                send(&slot, SoloistUpdate::Facts(facts));
                if let Some(host) = host
                    && may_restart(guard(&slot).daemon)
                {
                    host.recheck(None);
                }
            });
        if spawned.is_err() {
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: the key was not stored");
            self.publish(SoloistUpdate::Fault(SetupFault::KeychainFailed));
        }
    }

    /// Runs the pairing child, if there is something to pair with.
    fn pair(&self) {
        let cancel = Arc::new(AtomicBool::new(false));
        let Some(program) = self.begin_pairing(&cancel) else {
            return;
        };
        if self.data_dir.as_os_str().is_empty() {
            self.refuse(PairFault::NotReady);
            return;
        }
        let key = match self.store.load() {
            Ok(Some(key)) => key,
            Ok(None) | Err(_) => {
                self.refuse(PairFault::NotReady);
                return;
            }
        };
        if std::fs::create_dir_all(&self.data_dir).is_err() {
            self.refuse(PairFault::CouldNotStart);
            return;
        }
        let spawned = Launch::pair(&program, &self.name, &key, &self.data_dir).spawn();
        // The keychain has it, and the child was given it: nothing here keeps it.
        drop(key);
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                log::info!(target: SOLOIST_LOG_TARGET, "soloist: pairing did not start: {error}");
                self.refuse(PairFault::CouldNotStart);
                return;
            }
        };
        let slot = Arc::clone(&self.slot);
        let host = self.host.clone();
        let waited = thread::Builder::new()
            .name(PAIR_THREAD.to_owned())
            .spawn(move || wait_for_pair(&slot, host.as_ref(), child, &cancel));
        if waited.is_err() {
            log::info!(target: SOLOIST_LOG_TARGET, "soloist: pairing was not waited on");
            self.refuse(PairFault::CouldNotStart);
        }
    }

    /// Takes the pairing slot, or answers why the step cannot start.
    fn begin_pairing(&self, cancel: &Arc<AtomicBool>) -> Option<PathBuf> {
        let mut slot = self.lock();
        if slot.pairing.is_some() {
            return None;
        }
        if !may_restart(slot.daemon) {
            drop(slot);
            self.publish(SoloistUpdate::Pairing(PairEnd::Failed(PairFault::Busy)));
            return None;
        }
        let Some(program) = slot.program.clone() else {
            drop(slot);
            self.publish(SoloistUpdate::Pairing(PairEnd::Failed(PairFault::NotReady)));
            return None;
        };
        slot.pairing = Some(Arc::clone(cancel));
        Some(program)
    }

    /// Answers a pairing that was refused before its child ran.
    fn refuse(&self, fault: PairFault) {
        self.lock().pairing = None;
        self.publish(SoloistUpdate::Pairing(PairEnd::Failed(fault)));
    }

    /// Asks the pairing child to stop, which the thread waiting on it carries
    /// out.
    fn stop_pair(&self) {
        let cancel = self.lock().pairing.clone();
        if let Some(cancel) = cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn publish(&self, update: SoloistUpdate) {
        send(&self.slot, update);
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        guard(&self.slot)
    }
}

/// One scan: the program, its build, and whether a key is stored.
fn run_scan(
    slot: &Mutex<Slot>,
    store: &dyn SoloistKeyStore,
    host: Option<&Arc<SoloistHost>>,
    path: Option<String>,
) {
    let found = search(path.as_deref());
    let (facts, program) = match found {
        Scan::NotSoloist => {
            send(slot, SoloistUpdate::Fault(SetupFault::NotSoloist));
            return;
        }
        Scan::Missing { searched_downloads } => (
            facts(None, None, now_unix(), key_state(store), searched_downloads),
            None,
        ),
        Scan::Found(program) => {
            let version = Launch::version(&program).ok();
            let facts = facts(
                Some(&program),
                version.as_deref(),
                now_unix(),
                key_state(store),
                false,
            );
            (facts, Some(program))
        }
    };
    let changed = {
        let mut slot = guard(slot);
        let changed = program != slot.program;
        slot.program = program.clone();
        slot.facts = Some(facts.clone());
        changed
    };
    send(slot, SoloistUpdate::Facts(facts));
    if let Some(program) = program
        && changed
        && let Some(host) = host
        && may_restart(guard(slot).daemon)
    {
        host.recheck(Some(program));
    }
}

/// Waits for the pairing child, and answers when it ends or is stopped.
fn wait_for_pair(
    slot: &Mutex<Slot>,
    host: Option<&Arc<SoloistHost>>,
    mut child: Child,
    cancel: &AtomicBool,
) {
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            guard(slot).pairing = None;
            send(slot, SoloistUpdate::Pairing(PairEnd::Stopped));
            return;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                guard(slot).pairing = None;
                let end = pair_end(status.code());
                send(slot, SoloistUpdate::Pairing(end));
                if end == PairEnd::Landed
                    && let Some(host) = host
                    && may_restart(guard(slot).daemon)
                {
                    host.recheck(None);
                }
                return;
            }
            Ok(None) => thread::sleep(PAIR_POLL),
            Err(_) => {
                guard(slot).pairing = None;
                send(
                    slot,
                    SoloistUpdate::Pairing(PairEnd::Failed(PairFault::Exited)),
                );
                return;
            }
        }
    }
}

/// How a pairing child's exit names what happened.
fn pair_end(code: Option<i32>) -> PairEnd {
    match code {
        Some(0) => PairEnd::Landed,
        Some(code) if expiry::exit_code_means_expired(code) => PairEnd::Failed(PairFault::Expired),
        _ => PairEnd::Failed(PairFault::Exited),
    }
}

/// Why a key could not be stored, as a kind.
fn fault_of(error: &StoreError) -> SetupFault {
    match error {
        StoreError::Unavailable => SetupFault::KeychainUnavailable,
        StoreError::Corrupt | StoreError::Other(_) => SetupFault::KeychainFailed,
    }
}

/// What a scan found, as the page draws it.
fn facts(
    program: Option<&Path>,
    version: Option<&str>,
    now_unix: u64,
    key: KeyState,
    looked_in_downloads: bool,
) -> Facts {
    let built_at = version.and_then(expiry::parse_build);
    let build = built_at
        .map(|at| expiry::classify_build(at, now_unix))
        .unwrap_or(Build::Unknown);
    let built = built_at.and_then(day_of);
    Facts {
        program: program.map(|path| path.display().to_string()),
        build: freshness(build),
        built,
        expires: built.and_then(|day| day_after(day, expiry::EXPIRY_DAYS)),
        key,
        looked_in_downloads,
    }
}

/// The day a unix second count falls on.
fn day_of(seconds: u64) -> Option<Day> {
    i32::try_from(seconds / 86_400).ok().map(Day::from_days)
}

/// A day, counted forward.
fn day_after(day: Day, days: i64) -> Option<Day> {
    i32::try_from(i64::from(day.days()) + days)
        .ok()
        .map(Day::from_days)
}

/// Whether the keychain holds a key, without keeping what it holds.
fn key_state(store: &dyn SoloistKeyStore) -> KeyState {
    match store.load() {
        Ok(Some(_)) => KeyState::Stored,
        Ok(None) => KeyState::Absent,
        Err(_) => KeyState::Unreadable,
    }
}

/// Notes that the keychain now holds a key, and answers the facts to publish.
fn remember_key(slot: &Mutex<Slot>) -> Facts {
    let mut slot = guard(slot);
    let mut facts = slot.facts.clone().unwrap_or(Facts {
        program: None,
        build: Freshness::Unknown,
        built: None,
        expires: None,
        key: KeyState::Absent,
        looked_in_downloads: false,
    });
    facts.key = KeyState::Stored;
    slot.facts = Some(facts.clone());
    facts
}

/// Looks for the program, with the disk as the process has it.
fn search(typed: Option<&str>) -> Scan {
    let path_env = std::env::var_os("PATH");
    let profile = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let downloads = scan::downloads_dir(profile.as_deref());
    scan::scan(
        typed.map(Path::new),
        path_env.as_deref(),
        downloads.as_deref(),
        &Disk,
    )
}

/// Opens one of the setup pages in the listener's own browser.
fn open_page(page: SoloistPage) {
    let url = match page {
        SoloistPage::Download => DOWNLOAD_PAGE,
        SoloistPage::Key => KEY_PAGE,
    };
    if open::that_detached(url).is_err() {
        log::info!(target: SOLOIST_LOG_TARGET, "soloist: the browser did not open");
    }
}

/// Hands one update to the window, when one is listening.
fn send(slot: &Mutex<Slot>, update: SoloistUpdate) {
    let sink = guard(slot).sink.clone();
    if let Some(sink) = sink {
        sink(Update::Soloist(update));
    }
}

fn guard(slot: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
