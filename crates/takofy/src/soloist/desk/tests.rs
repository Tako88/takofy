use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use takofy_core::credentials::{SoloistKey, SoloistKeyStore, StoreError};
use takofy_core::soloist::expiry::EXPIRY_DAYS;
use takofy_model::home::Day;
use takofy_ui::message::{
    DaemonState, Freshness, KeyState, PairEnd, PairFault, SetupFault, SoloistAsk, SoloistUpdate,
    Update,
};
use takofy_ui::secret::Secret;

use super::{Desk, Scans, facts, fault_of, pair_end};
use crate::soloist::Sink;

/// How long a test waits for an answer a thread owes it.
const WAIT: Duration = Duration::from_secs(5);

const NAME: &str = "Takofy Soloist";

/// A keychain a test owns.
#[derive(Default)]
struct FakeStore {
    key: Mutex<Option<SoloistKey>>,
    saves: Mutex<usize>,
}

impl FakeStore {
    /// How many times the key was written.
    fn saved(&self) -> usize {
        *self.saves.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SoloistKeyStore for FakeStore {
    fn load(&self) -> Result<Option<SoloistKey>, StoreError> {
        Ok(self
            .key
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn save(&self, key: &SoloistKey) -> Result<(), StoreError> {
        *self.key.lock().unwrap_or_else(PoisonError::into_inner) = Some(key.clone());
        *self.saves.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        Ok(())
    }

    fn clear(&self) -> Result<(), StoreError> {
        *self.key.lock().unwrap_or_else(PoisonError::into_inner) = None;
        Ok(())
    }
}

/// A sink whose updates the test reads.
fn sink() -> (Sink, Receiver<Update>) {
    let (updates, answers) = mpsc::channel();
    let sink: Sink = Arc::new(move |update| {
        let _ = updates.send(update);
    });
    (sink, answers)
}

fn desk(store: Arc<FakeStore>) -> Desk {
    Desk::new(
        store,
        PathBuf::from("/nonexistent/takofy/soloist"),
        NAME.to_owned(),
        None,
    )
}

/// The unix time the Day arithmetic is read against.
const NOW: u64 = 1_700_000_000;
const DAY: u64 = 86_400;

/// A `--version` line that names a build time.
fn version(built: u64) -> String {
    format!("soloist 0.1.0 {built} linux x86_64")
}

fn today() -> Day {
    Day::from_days(i32::try_from(NOW / DAY).expect("a day"))
}

#[test]
fn a_pairing_that_paired_landed() {
    assert_eq!(pair_end(Some(0)), PairEnd::Landed);
}

#[test]
fn an_expired_build_ends_pairing_as_expired() {
    assert_eq!(pair_end(Some(10)), PairEnd::Failed(PairFault::Expired));
}

#[test]
fn a_general_failure_ends_pairing_as_an_exit() {
    assert_eq!(
        pair_end(Some(1)),
        PairEnd::Failed(PairFault::Exited),
        "another Soloist may be holding the data directory, and only the page can say so"
    );
}

#[test]
fn a_child_with_no_exit_code_is_a_failure() {
    assert_eq!(pair_end(None), PairEnd::Failed(PairFault::Exited));
}

#[test]
fn a_build_with_a_day_left_is_expiring() {
    let built = NOW - 89 * DAY;

    let facts = facts(
        Some(Path::new("/opt/soloist/soloist")),
        Some(&version(built)),
        NOW,
        KeyState::Absent,
        false,
    );

    assert_eq!(facts.build, Freshness::Expiring { days_left: 1 });
    assert_eq!(facts.built, super::day_of(built));
    assert_eq!(
        facts.expires,
        Some(Day::from_days(today().days() + 1)),
        "a build expires {EXPIRY_DAYS} days after it was made"
    );
}

#[test]
fn a_build_on_its_last_day_is_expired() {
    let built = NOW - 90 * DAY;

    let facts = facts(None, Some(&version(built)), NOW, KeyState::Stored, false);

    assert_eq!(facts.build, Freshness::Expired);
    assert_eq!(facts.expires, Some(today()), "the last day is today");
}

#[test]
fn a_fresh_build_is_fresh() {
    let facts = facts(None, Some(&version(NOW)), NOW, KeyState::Absent, false);

    assert_eq!(facts.build, Freshness::Fresh);
    assert_eq!(facts.built, Some(today()));
    assert_eq!(facts.expires, Some(Day::from_days(today().days() + 90)));
}

#[test]
fn a_version_that_names_no_build_is_unknown_and_never_blocks_play() {
    let facts = facts(
        Some(Path::new("/opt/soloist/soloist")),
        Some("soloist 0.1.0 linux x86_64"),
        NOW,
        KeyState::Absent,
        false,
    );

    assert_eq!(facts.build, Freshness::Unknown);
    assert_eq!(facts.built, None);
    assert_eq!(facts.expires, None);
}

#[test]
fn a_program_that_printed_nothing_is_still_the_program() {
    let facts = facts(
        Some(Path::new("/opt/soloist/soloist")),
        None,
        NOW,
        KeyState::Unreadable,
        true,
    );

    assert_eq!(facts.program.as_deref(), Some("/opt/soloist/soloist"));
    assert_eq!(facts.build, Freshness::Unknown);
    assert_eq!(facts.key, KeyState::Unreadable);
    assert!(facts.looked_in_downloads);
}

#[test]
fn a_keychain_that_is_unavailable_is_named_as_such() {
    assert_eq!(
        fault_of(&StoreError::Unavailable),
        SetupFault::KeychainUnavailable
    );
    assert_eq!(fault_of(&StoreError::Corrupt), SetupFault::KeychainFailed);
    assert_eq!(
        fault_of(&StoreError::Other("the store said no")),
        SetupFault::KeychainFailed,
        "the store's own text is a detail the page never draws"
    );
}

#[test]
fn one_scan_runs_and_the_newest_waiting_one_follows() {
    let mut scans = Scans::default();

    assert!(scans.request(None), "the first request runs at once");
    assert!(
        !scans.request(Some("/one".to_owned())),
        "a second one waits behind the first"
    );
    assert!(!scans.request(Some("/two".to_owned())), "and is replaced");
    assert_eq!(
        scans.finished(),
        Some(Some("/two".to_owned())),
        "the last one asked for is the one that answers"
    );
    assert_eq!(scans.finished(), None, "and nothing is left to run");
    assert!(scans.request(None), "a finished scan starts a fresh one");
}

#[test]
fn an_empty_key_is_refused_before_the_keychain() {
    let store = Arc::new(FakeStore::default());
    let desk = desk(Arc::clone(&store));
    let (sink, answers) = sink();

    desk.ask(SoloistAsk::StoreKey(Secret::default()), sink);

    assert_eq!(
        answers.recv_timeout(WAIT).expect("an answer"),
        Update::Soloist(SoloistUpdate::Fault(SetupFault::KeyEmpty))
    );
    assert_eq!(store.saved(), 0, "an empty key is never written");
}

#[test]
fn a_pasted_key_is_stored_and_comes_back_as_facts() {
    const KEY: &str = "sentinel-key-3f9a";
    let store = Arc::new(FakeStore::default());
    let desk = desk(Arc::clone(&store));
    let (sink, answers) = sink();

    desk.ask(SoloistAsk::StoreKey(Secret::from_text(KEY)), sink);
    let answer = answers.recv_timeout(WAIT).expect("an answer");
    let printed = format!("{answer:?}");

    let Update::Soloist(SoloistUpdate::Facts(stored)) = answer else {
        panic!("a stored key answers with facts: {printed}");
    };
    assert_eq!(stored.key, KeyState::Stored);
    assert_eq!(store.saved(), 1);
    assert!(
        !printed.contains(KEY),
        "the answer carries the keychain's word, not the key"
    );
}

#[test]
fn pairing_without_a_program_is_refused() {
    let desk = desk(Arc::new(FakeStore::default()));
    let (sink, answers) = sink();

    desk.ask(SoloistAsk::Pair, sink);

    assert_eq!(
        answers.recv_timeout(WAIT).expect("an answer"),
        Update::Soloist(SoloistUpdate::Pairing(PairEnd::Failed(PairFault::NotReady)))
    );
}

#[test]
fn a_live_daemon_would_hold_the_data_directory() {
    let desk = desk(Arc::new(FakeStore::default()));
    super::guard(&desk.slot).daemon = DaemonState::Starting;
    let (sink, answers) = sink();

    desk.ask(SoloistAsk::Pair, sink);

    assert_eq!(
        answers.recv_timeout(WAIT).expect("an answer"),
        Update::Soloist(SoloistUpdate::Pairing(PairEnd::Failed(PairFault::Busy)))
    );
}

#[test]
fn a_pairing_already_running_is_not_started_twice() {
    let desk = desk(Arc::new(FakeStore::default()));
    {
        let mut slot = super::guard(&desk.slot);
        slot.program = Some(PathBuf::from("/opt/soloist/soloist"));
        slot.pairing = Some(Arc::new(AtomicBool::new(false)));
    }
    let (sink, answers) = sink();

    desk.ask(SoloistAsk::Pair, sink);

    assert!(
        answers.recv_timeout(Duration::from_millis(100)).is_err(),
        "the running step is the one that answers"
    );
}

#[test]
fn a_desk_with_nothing_running_shuts_down_quietly() {
    desk(Arc::new(FakeStore::default())).shutdown();
}

#[test]
fn a_pasted_key_never_reaches_a_debug_line() {
    const KEY: &str = "sentinel-key-3f9a";

    let ask = SoloistAsk::StoreKey(Secret::from_text(KEY));
    let printed = format!("{ask:?}");

    assert!(!printed.contains(KEY), "{printed}");
}
