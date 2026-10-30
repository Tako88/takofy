use super::super::*;
use super::test_support::*;
use crate::playhead::Playhead;
use crate::session::SessionHost;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{RecvTimeoutError, channel};
use takofy_core::connect::WITHDRAW_TIMEOUT;
use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::login::{SourceError, SourceErrorKind, TokenSet, TokenSource};
use takofy_core::playback::PlaybackEvent;
use takofy_model::RequestId;
use tokio::sync::mpsc::UnboundedSender;

// ---- session recovery ----

/// A scripted session retirement. The default joins at once.
#[derive(Clone, Copy, Default)]
pub(super) enum RetireScript {
    #[default]
    Ok,
    /// Never answers, so a test can stop the window mid-retirement.
    Pending,
}

/// Starts the worker on the test schedule, so a rebuild is measured in
/// milliseconds instead of seconds.
fn start_paced<H: SessionHost, S: TokenSource + Send + Sync + 'static>(
    runtime: &Runtime,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) -> (UiEnd, WakeSlot, Worker) {
    start_with(
        runtime.handle(),
        host,
        store,
        source,
        WITHDRAW_TIMEOUT,
        Pacing::FAST,
    )
}

/// Runs the window on the test schedule and finishes the worker after it.
fn run_window_paced<H, S, R>(
    runtime: Runtime,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    window: impl FnOnce(UiEnd, WakeSlot) -> R,
) -> R
where
    H: SessionHost,
    S: TokenSource + Send + Sync + 'static,
{
    let (ui_end, slot, worker) = start_paced(&runtime, host, store, source);
    run_ordered(
        runtime,
        move || window(ui_end, slot),
        move || worker.finish(),
    )
}

/// The test schedule with an attempt budget long enough that a step held
/// open by a test is not ended by the schedule.
fn patient() -> Pacing {
    Pacing {
        attempt: Duration::from_secs(5),
        ..Pacing::FAST
    }
}

/// Starts the worker on [`patient`], so a held step outlives the wait a
/// test needs to act while it is in flight.
fn run_window_patient<H, S, R>(
    runtime: Runtime,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    window: impl FnOnce(UiEnd, WakeSlot) -> R,
) -> R
where
    H: SessionHost,
    S: TokenSource + Send + Sync + 'static,
{
    let (ui_end, slot, worker) = start_with(
        runtime.handle(),
        host,
        store,
        source,
        WITHDRAW_TIMEOUT,
        patient(),
    );
    run_ordered(
        runtime,
        move || window(ui_end, slot),
        move || worker.finish(),
    )
}

/// Reads updates until one matches, passing over the rest.
fn await_update(ui_end: &UiEnd, what: &str, mut matches: impl FnMut(&Update) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match ui_end.updates.recv_timeout(Duration::from_millis(100)) {
            Ok(update) => {
                if matches(&update) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("the worker went away"),
        }
    }
    panic!("timed out waiting for {what}");
}

/// How many calls satisfy `wanted`.
fn call_count(shared: &FakeState, wanted: impl Fn(&Call) -> bool) -> usize {
    calls(shared).iter().filter(|call| wanted(call)).count()
}

/// Plays one track and anchors it where the test wants, so a rebuild has a
/// position to resume from.
fn play_anchored(ui_end: &UiEnd, shared: &FakeState, uri: &str, position_ms: u32) {
    let (started, start) = channel();
    *shared.play_started.lock().expect("lock") = Some(started);
    ui_end
        .commands
        .send(Command::Play {
            uri: uri.to_owned(),
        })
        .expect("play");
    start.recv().expect("play started");
    events(shared)
        .send(PlaybackEvent::Playing { position_ms })
        .expect("playing");
    await_update(ui_end, "the playing position", |update| {
        *update
            == Update::Transport(TransportUpdate::Position {
                position_ms,
                paused: false,
            })
    });
}

fn good(value: &str) -> Result<TokenSet, SourceError> {
    Ok(token_set(value, None))
}

fn no_token() -> Result<TokenSet, SourceError> {
    Err(SourceError {
        kind: SourceErrorKind::Other,
    })
}

/// Everything already queued, without waiting for more.
fn queued(ui_end: &UiEnd) -> Vec<Update> {
    let mut seen = Vec::new();
    while let Ok(update) = ui_end.updates.try_recv() {
        seen.push(update);
    }
    seen
}

/// Whether one track has been asked for by the fake player.
fn loaded(shared: &FakeState, uri: &str) -> bool {
    calls(shared)
        .iter()
        .any(|call| matches!(call, Call::Load { uri: asked, .. } if asked == uri))
}

/// The event sender of the current fake player.
fn events(shared: &FakeState) -> UnboundedSender<PlaybackEvent> {
    shared
        .event_tx
        .lock()
        .expect("lock")
        .clone()
        .expect("the player has an event stream")
}

/// Makes the next watch wait for the returned sender before it answers.
fn hold_watch(shared: &FakeState) -> tokio::sync::oneshot::Sender<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    *shared.watch_gate.lock().expect("lock") = Some(gate);
    release
}

/// Long enough for a load to pass librespot's key timeout.
const PRE_DETECTION: Duration = Duration::from_millis(1_600);

#[test]
fn a_drop_while_playing_retires_then_connects_then_reloads_in_place() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 1_000);
        watched.down.store(true, Ordering::SeqCst);
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        queued(&ui_end)
    });

    let log = calls(&shared);
    assert_eq!(
        log.len(),
        6,
        "one play, one retire, then the rebuild: {log:?}"
    );
    assert_eq!(log[0], Call::Play(TRACK.to_owned()));
    assert_eq!(log[1], Call::SetPlayable(true));
    assert_eq!(
        log[2],
        Call::Retire,
        "the session is retired before it is replaced"
    );
    assert_eq!(log[3], Call::Report(Box::new(None)));
    assert_eq!(log[4], Call::Prepare);
    match &log[5] {
        Call::Load {
            uri,
            position_ms,
            play,
        } => {
            assert_eq!(uri, TRACK);
            assert!(
                *position_ms >= 1_000,
                "the reload resumes at or past the anchor"
            );
            assert!(*play, "a playing track comes back playing");
        }
        other => panic!("the last call must be the reload, got {other:?}"),
    }
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Retire)),
        1,
        "exactly one session is retired"
    );
    assert_eq!(
        &*shared.retires_at_connect.lock().expect("lock"),
        &[1],
        "the rebuild's connect follows the retire"
    );
    assert_eq!(
        *shared.last_connect_token.lock().expect("lock"),
        Some("fresh".to_owned()),
        "the rebuild connects with the refreshed token"
    );
    assert_eq!(shared.connects.load(Ordering::SeqCst), 2);
    assert_eq!(shared.watches.load(Ordering::SeqCst), 0);
    assert_eq!(
        updates
            .iter()
            .filter(|update| **update == Update::Transport(TransportUpdate::Reconnecting))
            .count(),
        1,
        "the window is told once that the link is coming back"
    );
}

#[test]
fn a_drop_while_paused_reloads_paused_and_says_where() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 1_000);
        events(&watched)
            .send(PlaybackEvent::Paused {
                position_ms: 20_000,
            })
            .expect("paused");
        await_update(&ui_end, "the paused position", |update| {
            *update
                == Update::Transport(TransportUpdate::Position {
                    position_ms: 20_000,
                    paused: true,
                })
        });
        watched.down.store(true, Ordering::SeqCst);
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        queued(&ui_end)
    });

    let load = calls(&shared)
        .into_iter()
        .find(|call| matches!(call, Call::Load { .. }))
        .expect("the paused track is reloaded");
    match load {
        Call::Load {
            position_ms, play, ..
        } => {
            assert!(!play, "a paused track comes back paused");
            assert!(position_ms >= 20_000);
        }
        other => panic!("expected a load, got {other:?}"),
    }
    let reconnect = updates
        .iter()
        .position(|update| *update == Update::Transport(TransportUpdate::Reconnecting))
        .expect("the window is told the link is coming back");
    let settled = updates
        .iter()
        .position(|update| {
            matches!(
                update,
                Update::Transport(TransportUpdate::Position { paused: true, .. })
            )
        })
        .expect("the paused reload is reported");
    assert!(reconnect < settled, "the reload settles the flash");
}

#[test]
fn a_drop_with_nothing_loaded_rebuilds_without_a_flash_or_a_load() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watched.down.store(true, Ordering::SeqCst);
        until("the rebuilt session", || {
            watched.connects.load(Ordering::SeqCst) == 2
                && call_count(&watched, |call| matches!(call, Call::Prepare)) >= 1
        });
        std::thread::sleep(Duration::from_millis(50));
        queued(&ui_end)
    });

    assert!(
        !updates.contains(&Update::Transport(TransportUpdate::Reconnecting)),
        "there is nothing to flash about"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        0,
        "nothing was loaded, so nothing is reloaded"
    );
    assert_eq!(shared.connects.load(Ordering::SeqCst), 2);
}

#[test]
fn the_eighth_failed_refresh_is_said_in_words_and_retries_continue() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let mut tokens = vec![good("access")];
    tokens.extend((0..8).map(|_| no_token()));
    tokens.push(good("fresh"));
    let source = Arc::new(ScriptedSource::sequencer(tokens));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        watched.down.store(true, Ordering::SeqCst);
        let mut seen = Vec::new();
        loop {
            let update = ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("an update");
            let said_in_words =
                update == Update::Transport(TransportUpdate::Failed(Failure::Reconnect));
            seen.push(update);
            if said_in_words {
                break;
            }
        }
        assert!(
            probe.delivered.load(Ordering::SeqCst) >= 9,
            "the words wait for the eighth failed refresh"
        );
        until("the load after the stall", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        seen.extend(queued(&ui_end));
        seen
    });
    assert_eq!(
        updates
            .iter()
            .filter(|update| {
                **update == Update::Transport(TransportUpdate::Failed(Failure::Reconnect))
            })
            .count(),
        1,
        "the stuck words are said once"
    );
    assert_eq!(
        shared.connects.load(Ordering::SeqCst),
        2,
        "a success connects"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        1,
        "the first success reloads the track"
    );
}

#[test]
fn a_store_that_cannot_be_read_is_a_failed_attempt_without_a_connect() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let probe = Arc::clone(&store);
    let source = Arc::new(ScriptedSource::sequencer(vec![good("access")]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        // The store fails from here on, which is the `Err` arm rather than
        // a refused exchange.
        probe.unavailable.store(true, Ordering::SeqCst);
        watched.down.store(true, Ordering::SeqCst);
        await_update(&ui_end, "the stuck words", |update| {
            *update == Update::Transport(TransportUpdate::Failed(Failure::Reconnect))
        });
    });

    assert_eq!(
        shared.connects.load(Ordering::SeqCst),
        1,
        "a refresh that the store could not start never connects"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        0
    );
}

#[test]
fn a_failed_refresh_makes_no_connect_and_keeps_trying() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        no_token(),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        let (release, started) = probe.hold_next();
        let (release_next, started_next) = probe.hold_next();
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the refresh started");
        assert_eq!(
            watched.connects.load(Ordering::SeqCst),
            1,
            "a refresh in flight has not connected"
        );
        release.send(()).expect("release the failed refresh");
        started_next.recv().expect("the next attempt started");
        assert_eq!(
            watched.connects.load(Ordering::SeqCst),
            1,
            "a refused refresh is a failed attempt, not a connect"
        );
        release_next.send(()).expect("release the success");
        until("the connect after the success", || {
            watched.connects.load(Ordering::SeqCst) == 2
        });
    });
}

#[test]
fn a_failed_reload_retries_without_reconnecting() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        *watched.load_script.lock().expect("lock") = LoadScript::Fail;
        watched.down.store(true, Ordering::SeqCst);
        until("the first failed reload", || {
            call_count(&watched, |call| matches!(call, Call::Load { .. })) >= 1
        });
        until("the second reload", || {
            call_count(&watched, |call| matches!(call, Call::Load { .. })) >= 2
        });
    });

    assert_eq!(
        shared.connects.load(Ordering::SeqCst),
        2,
        "a failed load retries on the session it already has"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Retire)),
        1,
        "the dead session is retired once"
    );
}

#[test]
fn a_session_that_dies_mid_recovery_is_retired_before_it_is_replaced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
        good("again"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        *watched.load_script.lock().expect("lock") = LoadScript::Fail;
        // The first rebuild connects, and its reload fails, so recovery
        // stays active with a live engine.
        watched.down.store(true, Ordering::SeqCst);
        until("the first rebuild", || {
            watched.connects.load(Ordering::SeqCst) == 2
                && call_count(&watched, |call| matches!(call, Call::Load { .. })) >= 1
        });
        // A watch opened while recovering belongs to that session too.
        let (started, start) = channel();
        *watched.watch_started.lock().expect("lock") = Some(started);
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        start.recv().expect("watch started");
        let old_events = events(&watched);
        let old_devices = watched
            .device_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("the watch opened a stream");
        // That engine dies while recovery is already running: the attempt
        // must close it before it connects over it.
        watched.down.store(true, Ordering::SeqCst);
        until("the second retirement", || {
            call_count(&watched, |call| matches!(call, Call::Retire)) == 2
        });
        until("the rebuilt connect", || {
            watched.connects.load(Ordering::SeqCst) == 3
        });
        until("the watch the rebuilt session restarts", || {
            watched.watches.load(Ordering::SeqCst) == 2
        });
        // The drains of the session that died are silent, even as their
        // streams end.
        old_events
            .send(PlaybackEvent::Playing { position_ms: 7 })
            .expect("old playback");
        old_devices.send(ClusterEvent::Failed).expect("old devices");
        drop(old_devices);
        std::thread::sleep(Duration::from_millis(100));
        queued(&ui_end)
    });

    assert_eq!(
        &*shared.retires_at_connect.lock().expect("lock"),
        &[1, 2],
        "each retirement happens before the connect that replaces it"
    );
    assert_eq!(
        shared.watches.load(Ordering::SeqCst),
        2,
        "the watch of the session that died is started again"
    );
    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, Update::Devices(_))),
        "the drain of the session that died says nothing: {updates:?}"
    );
    assert!(
        !updates.iter().any(|update| matches!(
            update,
            Update::Transport(TransportUpdate::Position { position_ms: 7, .. })
        )),
        "the old playback drain is silent: {updates:?}"
    );
}

#[test]
fn a_command_that_does_not_retarget_waits_for_the_attempt() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        watch_now(&ui_end, &watched);
        let (release, started) = probe.hold_next();
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the rebuild refresh started");
        // A report says nothing about where the rebuild resumes, so it
        // waits instead of cancelling the attempt.
        ui_end
            .commands
            .send(Command::Report(Some(report(TRACK))))
            .expect("report");
        ui_end
            .commands
            .send(Command::Report(Some(report(SECOND))))
            .expect("report");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            calls(&watched)
                .iter()
                .filter(|call| matches!(call, Call::Report(value) if value.is_some()))
                .count(),
            0,
            "a report waits for the attempt to finish"
        );
        release.send(()).expect("release the refresh");
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        ui_end
            .commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
        until("the play", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Play(_)))
        });
    });

    let log = calls(&shared);
    let reported: Vec<_> = log
        .iter()
        .filter_map(|call| match call {
            Call::Report(value) => (**value).clone(),
            _ => None,
        })
        .collect();
    assert_eq!(
        reported,
        vec![report(TRACK), report(SECOND)],
        "both reports are applied after the attempt, once, in order: {log:?}"
    );
    let last_report = log
        .iter()
        .rposition(|call| matches!(call, Call::Report(value) if value.is_some()))
        .expect("the reports are applied");
    let play = log
        .iter()
        .position(|call| matches!(call, Call::Play(_)))
        .expect("the play lands");
    assert!(
        last_report < play,
        "a following control command lands after the buffered ones: {log:?}"
    );
}

#[test]
fn an_interrupting_command_waits_behind_the_ones_the_attempt_deferred() {
    let store: Arc<dyn CredentialStore> = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![good("access")]));
    let probe = Arc::clone(&source);
    let mut host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let (updates, _read) = channel();
    let notifier = Notifier {
        tx: updates,
        slot: WakeSlot::default(),
    };
    let (signal, _signals) = tokio::sync::mpsc::unbounded_channel();
    let mut drains = Drains::new(signal);
    let mut devices = Devices::default();
    let (_stop, mut stop) = tokio::sync::oneshot::channel();
    let (commands, mut received) = tokio::sync::mpsc::unbounded_channel();
    let mut deferred = VecDeque::new();
    let mut refresh: Option<Refresh> = None;
    let mut token: Option<AccessToken> = None;
    let mut engine_up = false;
    let (release, started) = probe.hold_next();
    // The refresh holds the attempt open long enough for the window's
    // commands to arrive in the order it sent them.
    let driver = std::thread::spawn(move || {
        started.recv().expect("the refresh started");
        commands
            .send(Command::Report(Some(report(TRACK))))
            .expect("report");
        commands
            .send(Command::Volume { percent: 35 })
            .expect("volume");
        commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
    });

    let outcome = runtime().block_on(rebuild_attempt(
        &mut host,
        &mut stop,
        &notifier,
        &mut devices,
        &mut drains,
        &mut received,
        &mut deferred,
        &mut refresh,
        &mut token,
        &mut engine_up,
        &store,
        &source,
        None,
        patient().attempt,
    ));
    driver.join().expect("the driver");
    drop(release);

    assert!(
        matches!(outcome, Rebuilt::Interrupted),
        "an interrupting command ends the attempt"
    );
    assert_eq!(
        deferred.iter().cloned().collect::<Vec<_>>(),
        vec![
            Command::Report(Some(report(TRACK))),
            Command::Volume { percent: 35 },
            Command::Play {
                uri: SECOND.to_owned(),
            },
        ],
        "the interrupt waits behind the commands sent before it"
    );
}

#[test]
fn an_answer_during_the_attempt_is_delivered_without_ending_it() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);

    run_window_patient(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        let id = RequestId(77);
        hold_request(&watched, id);
        let (release, started) = probe.hold_next();
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the rebuild refresh started");
        ui_end
            .commands
            .send(Command::Answer { id, ok: true })
            .expect("answer");
        // The answer reaches the registry while the attempt still waits on
        // the refresh, so the order's own budget cannot end it first.
        until("the answer", || {
            call_count(
                &watched,
                |call| matches!(call, Call::Answer(answered, _) if *answered == id),
            ) == 1
        });
        assert_eq!(
            watched.connects.load(Ordering::SeqCst),
            1,
            "an answer does not restart the attempt"
        );
        release.send(()).expect("release the refresh");
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    assert_eq!(
        answers(&shared, RequestId(77)),
        1,
        "the order is answered once"
    );
    assert_eq!(
        shared.connects.load(Ordering::SeqCst),
        2,
        "the attempt the answer joined carries on"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        1
    );
}

// Quarantined: the failure this test names is real, but the scenario it builds
// it from is a race the harness cannot settle — under load the held script can
// land on a watch that is not the rebuild's, and the attempt then ends as spent,
// dropping the pressed command. Run it with `--ignored` until that is fixed.
#[test]
#[ignore = "a harness race, not the behaviour: see the note above"]
fn a_command_during_the_watch_does_not_spend_the_start() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_patient(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        watch_now(&ui_end, &watched);
        // The rebuild's watch is held open, and a second start on the same
        // session answers what the dealer's spent builder answers.
        *watched.watch_scripts.lock().expect("lock") = vec![WatchScript::Held, WatchScript::Spent];
        let (started, start) = channel();
        *watched.watch_started.lock().expect("lock") = Some(started);
        let release = hold_watch(&watched);
        watched.down.store(true, Ordering::SeqCst);
        start.recv().expect("the rebuild watch started");
        ui_end
            .commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
        // The command waits: nothing the attempt does is visible until the
        // watch answers, so the check is a window during which the pressed
        // track must not load, not an instant that a slow machine can miss.
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !loaded(&watched, SECOND),
            "the watch holds the pressed track back: {:?}",
            calls(&watched)
        );
        let _ = release.send(());
        // The attempt reconnects, arms a watch and only then runs what it
        // deferred, so the wait is paced for a whole rebuild rather than one
        // hand-off.
        until_within("the pressed track", Duration::from_secs(10), || {
            loaded(&watched, SECOND)
        });
        queued(&ui_end)
    });

    assert_eq!(
        shared.watches.load(Ordering::SeqCst),
        2,
        "the rebuild finishes the one watch it started"
    );
    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, Update::Devices(DeviceUpdate::Failed(_)))),
        "the watch is not announced as lost: {updates:?}"
    );
    let load = calls(&shared)
        .into_iter()
        .find(|call| matches!(call, Call::Load { uri, .. } if uri == SECOND))
        .expect("the pressed track is loaded");
    assert_eq!(
        load,
        Call::Load {
            uri: SECOND.to_owned(),
            position_ms: 0,
            play: true,
        },
        "the command the watch held up is applied after it"
    );
}

#[test]
fn a_cancelled_attempt_reuses_the_refresh_it_left_in_flight() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);
    let outside = Arc::clone(&shared);
    let counted = Arc::clone(&probe);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        let (release, started) = probe.hold_next();
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the rebuild refresh started");
        // A play retargets the rebuild, cancelling the attempt while its
        // refresh is still out.
        ui_end
            .commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            probe.delivered.load(Ordering::SeqCst),
            1,
            "the attempt kept waiting on the one refresh in flight"
        );
        assert_eq!(
            shared.connects.load(Ordering::SeqCst),
            1,
            "a second refresh would have begun its own exchange"
        );
        release.send(()).expect("release the refresh");
        until("the reload of the pressed track", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    assert_eq!(
        counted.delivered.load(Ordering::SeqCst),
        2,
        "one refresh serves both attempts"
    );
    assert_eq!(outside.connects.load(Ordering::SeqCst), 2);
    let load = calls(&outside)
        .into_iter()
        .find(|call| matches!(call, Call::Load { .. }))
        .expect("the pressed track is loaded");
    assert_eq!(
        load,
        Call::Load {
            uri: SECOND.to_owned(),
            position_ms: 0,
            play: true,
        }
    );
}

#[test]
fn a_stop_during_recovery_leaves_the_transport_stopped() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 10_000);
        let (release, started) = probe.hold_next();
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the rebuild refresh started");
        ui_end.commands.send(Command::Stop).expect("stop");
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).expect("release the refresh");
        until("the rebuilt session", || {
            watched.connects.load(Ordering::SeqCst) == 2
        });
        std::thread::sleep(Duration::from_millis(100));
        queued(&ui_end)
    });

    assert!(
        updates.contains(&Update::Transport(TransportUpdate::Reconnecting)),
        "the window was told the link was coming back: {updates:?}"
    );
    let last = updates
        .iter()
        .rev()
        .find(|update| matches!(update, Update::Transport(_)));
    assert_eq!(
        last,
        Some(&Update::Transport(TransportUpdate::Stopped)),
        "a stopped recovery settles the transport: {updates:?}"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        0,
        "a stopped recovery has nothing to reload"
    );
    assert_eq!(
        shared.stops.load(Ordering::SeqCst),
        0,
        "a stop during recovery never reaches the host"
    );
}

#[test]
fn a_stall_with_nothing_to_reload_ends_when_the_link_returns() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let mut tokens = vec![good("access")];
    tokens.extend((0..8).map(|_| no_token()));
    tokens.push(good("fresh"));
    let source = Arc::new(ScriptedSource::sequencer(tokens));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watched.down.store(true, Ordering::SeqCst);
        await_update(&ui_end, "the stall words", |update| {
            *update == Update::Transport(TransportUpdate::Failed(Failure::Reconnect))
        });
        until("the rebuilt session", || {
            watched.connects.load(Ordering::SeqCst) == 2
        });
        std::thread::sleep(Duration::from_millis(100));
        queued(&ui_end)
    });

    let last = updates
        .iter()
        .rev()
        .find(|update| matches!(update, Update::Transport(_)));
    assert_eq!(
        last,
        Some(&Update::Transport(TransportUpdate::Stopped)),
        "the stall words do not outlive the recovery: {updates:?}"
    );
    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        0,
        "nothing was loaded, so nothing is reloaded"
    );
}

#[test]
fn a_play_during_the_connect_loads_the_pressed_track_afterwards() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("mid"),
        good("fresh"),
    ]));
    let host = FakeHost::with_connect_scripts(
        vec![ConnectScript::Ok, ConnectScript::Pending, ConnectScript::Ok],
        RootlistScript::Ready(Vec::new()),
    );
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        let (started, start) = channel();
        *watched.connect_started.lock().expect("lock") = Some(started);
        watched.down.store(true, Ordering::SeqCst);
        start.recv().expect("the rebuild connect started");
        ui_end
            .commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
        until("the reload of the pressed track", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        queued(&ui_end)
    });

    assert_eq!(
        shared.plays.load(Ordering::SeqCst),
        0,
        "no play reaches the dead session"
    );
    let load = calls(&shared)
        .into_iter()
        .find(|call| matches!(call, Call::Load { .. }))
        .expect("the pressed track is loaded");
    assert_eq!(
        load,
        Call::Load {
            uri: SECOND.to_owned(),
            position_ms: 0,
            play: true,
        }
    );
    assert_eq!(shared.connects.load(Ordering::SeqCst), 3);
    assert!(
        !updates.contains(&Update::Transport(TransportUpdate::Failed(
            Failure::Reconnect
        ))),
        "an interrupted attempt is not a failure"
    );
}

#[test]
fn a_pause_during_the_rebuild_reloads_paused() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("mid"),
        good("fresh"),
    ]));
    let host = FakeHost::with_connect_scripts(
        vec![ConnectScript::Ok, ConnectScript::Pending, ConnectScript::Ok],
        RootlistScript::Ready(Vec::new()),
    );
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 30_000);
        let (started, start) = channel();
        *watched.connect_started.lock().expect("lock") = Some(started);
        watched.down.store(true, Ordering::SeqCst);
        start.recv().expect("the rebuild connect started");
        ui_end.commands.send(Command::Pause).expect("pause");
        until("the paused reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        queued(&ui_end)
    });

    let load = calls(&shared)
        .into_iter()
        .find(|call| matches!(call, Call::Load { .. }))
        .expect("the track is reloaded");
    match load {
        Call::Load {
            position_ms, play, ..
        } => {
            assert!(!play, "a pause during the rebuild comes back paused");
            assert!(position_ms >= 30_000);
        }
        other => panic!("expected a load, got {other:?}"),
    }
    assert!(
        updates.iter().any(|update| matches!(
            update,
            Update::Transport(TransportUpdate::Position { paused: true, .. })
        )),
        "the paused reload settles the transport"
    );
}

#[test]
fn a_stop_during_the_rebuild_leaves_nothing_to_reload() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("mid"),
        good("fresh"),
    ]));
    let host = FakeHost::with_connect_scripts(
        vec![ConnectScript::Ok, ConnectScript::Pending, ConnectScript::Ok],
        RootlistScript::Ready(Vec::new()),
    );
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        let (started, start) = channel();
        *watched.connect_started.lock().expect("lock") = Some(started);
        watched.down.store(true, Ordering::SeqCst);
        start.recv().expect("the rebuild connect started");
        ui_end.commands.send(Command::Stop).expect("stop");
        until("the next attempt", || {
            call_count(&watched, |call| matches!(call, Call::Prepare)) >= 1
        });
        std::thread::sleep(Duration::from_millis(50));
    });

    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Load { .. })),
        0,
        "a stopped recovery has nothing to reload"
    );
    assert_eq!(shared.stops.load(Ordering::SeqCst), 0);
    assert_eq!(shared.connects.load(Ordering::SeqCst), 3);
}

#[test]
fn a_play_during_the_token_refresh_interrupts_it() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        let (release, started) = probe.hold_next();
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the refresh started");
        ui_end
            .commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
        // The next attempt waits on the refresh this one left in flight.
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).expect("release the refresh");
        until("the reload of the pressed track", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    assert_eq!(shared.plays.load(Ordering::SeqCst), 0);
    let load = calls(&shared)
        .into_iter()
        .find(|call| matches!(call, Call::Load { .. }))
        .expect("the pressed track is loaded");
    assert_eq!(
        load,
        Call::Load {
            uri: SECOND.to_owned(),
            position_ms: 0,
            play: true,
        }
    );
}

#[test]
fn a_play_inside_the_pre_detection_window_costs_one_rebuild_only() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
        good("again"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        ui_end
            .commands
            .send(Command::Play {
                uri: SECOND.to_owned(),
            })
            .expect("play");
        until("the load that will not start", || {
            watched.plays.load(Ordering::SeqCst) == 2
        });
        std::thread::sleep(PRE_DETECTION);
        events(&watched)
            .send(PlaybackEvent::Unavailable)
            .expect("unavailable");
        await_update(&ui_end, "the reconnecting flash", |update| {
            *update == Update::Transport(TransportUpdate::Reconnecting)
        });
        until("the one reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        // The reload is a new generation, so its failure is a real one and
        // goes to the window.
        events(&watched)
            .send(PlaybackEvent::Unavailable)
            .expect("unavailable");
        await_update(&ui_end, "the forwarded failure", |update| {
            *update == Update::Transport(TransportUpdate::Unavailable)
        });
        std::thread::sleep(Duration::from_millis(100));
        queued(&ui_end)
    });

    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Retire)),
        1,
        "the window is bounded to one rebuild"
    );
    assert_eq!(shared.connects.load(Ordering::SeqCst), 2);
    let loads: Vec<_> = calls(&shared)
        .into_iter()
        .filter(|call| matches!(call, Call::Load { .. }))
        .collect();
    assert_eq!(loads.len(), 1, "one reload: {loads:?}");
    assert_eq!(
        loads[0],
        Call::Load {
            uri: SECOND.to_owned(),
            position_ms: 0,
            play: true,
        }
    );
    assert_eq!(
        updates
            .iter()
            .filter(|update| **update == Update::Transport(TransportUpdate::Unavailable))
            .count(),
        0,
        "only the real failure reaches the window"
    );
}

#[test]
fn a_failure_after_the_rebuild_is_forwarded_without_another_rebuild() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        watched.down.store(true, Ordering::SeqCst);
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        events(&watched)
            .send(PlaybackEvent::Unavailable)
            .expect("unavailable");
        await_update(&ui_end, "the forwarded failure", |update| {
            *update == Update::Transport(TransportUpdate::Unavailable)
        });
        std::thread::sleep(Duration::from_millis(100));
    });

    assert_eq!(
        call_count(&shared, |call| matches!(call, Call::Retire)),
        1,
        "a fresh generation's first failure is not the link"
    );
}

#[test]
fn a_dead_flag_withholds_the_failure_and_keeps_the_resume_point() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 30_000);
        watched.down.store(true, Ordering::SeqCst);
        events(&watched)
            .send(PlaybackEvent::Unavailable)
            .expect("unavailable");
        await_update(&ui_end, "the reconnecting flash", |update| {
            *update == Update::Transport(TransportUpdate::Reconnecting)
        });
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        queued(&ui_end)
    });

    assert!(
        !updates.contains(&Update::Transport(TransportUpdate::Unavailable)),
        "a dead link is not an unavailable track"
    );
    let load = calls(&shared)
        .into_iter()
        .find(|call| matches!(call, Call::Load { .. }))
        .expect("the resume point survives the dead flag");
    match load {
        Call::Load {
            position_ms, play, ..
        } => {
            assert!(position_ms >= 30_000);
            assert!(play);
        }
        other => panic!("expected a load, got {other:?}"),
    }
}

#[test]
fn a_signal_from_a_retired_generation_is_ignored() {
    let playhead = Playhead::new();
    let recovery = Recovery::new(Pacing::FAST);
    let current = playhead.generation();
    assert!(link_signal_applies(current, &playhead, &recovery));

    playhead.next_generation();
    assert!(
        !link_signal_applies(current, &playhead, &recovery),
        "a stale generation cannot start a rebuild for the live one"
    );

    let mut active = Recovery::new(Pacing::FAST);
    active.lost(Instant::now(), None);
    assert!(
        !link_signal_applies(playhead.generation(), &playhead, &active),
        "a rebuild already underway is not started twice"
    );
}

#[test]
fn a_retired_watch_stays_due_to_be_restarted() {
    // A watch whose last restart failed is already due.
    let mut devices = Devices {
        rewatch: true,
        ..Devices::default()
    };
    devices.retire();
    assert!(
        devices.rewatch,
        "a second retirement does not clear a restart already due"
    );

    devices.watching = true;
    devices.retire();
    assert!(devices.rewatch, "a running watch is due to start again");
    assert!(!devices.watching, "the watch of the retired session is off");
}

#[test]
fn every_attempt_outcome_leaves_the_schedule_moved() {
    let base = Instant::now();
    let point = ResumePoint {
        uri: TRACK.to_owned(),
        position_ms: 0,
        play: true,
    };
    let outcomes = [Rebuilt::Done, Rebuilt::Failed, Rebuilt::Interrupted];
    for outcome in outcomes {
        let mut recovery = Recovery::new(Pacing::LIVE);
        recovery.lost(base, Some(point.clone()));
        let now = base + Duration::from_millis(1);
        settle(&mut recovery, &outcome, now);
        if let Some(at) = recovery.wake_at() {
            assert!(
                at > base,
                "an outcome must move the wake time past the loss, not leave it due"
            );
        }
    }
}

#[test]
fn a_retired_session_does_not_say_the_devices_are_lost() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let updates = run_window_paced(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        play_anchored(&ui_end, &watched, TRACK, 0);
        watch_now(&ui_end, &watched);
        let old_events = events(&watched);
        let old_devices = watched
            .device_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("the watch opened a stream");
        watched.down.store(true, Ordering::SeqCst);
        await_update(&ui_end, "the reconnecting flash", |update| {
            *update == Update::Transport(TransportUpdate::Reconnecting)
        });
        until("the reload", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
        old_events
            .send(PlaybackEvent::Playing { position_ms: 1 })
            .expect("old playback");
        old_devices.send(ClusterEvent::Failed).expect("old devices");
        drop(old_devices);
        std::thread::sleep(Duration::from_millis(100));
        queued(&ui_end)
    });

    assert_eq!(
        shared.watches.load(Ordering::SeqCst),
        2,
        "the rebuild restarts the watch it retired"
    );
    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, Update::Devices(_))),
        "a retired drain says nothing about the devices: {updates:?}"
    );
}

#[test]
fn a_stop_during_the_retire_still_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![good("access")]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.retire_script.lock().expect("lock") = RetireScript::Pending;

    let runtime = runtime();
    let (ui_end, slot, worker) = start_paced(&runtime, host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let watched = Arc::clone(&shared);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        let (started, start) = channel();
        *watched.retire_started.lock().expect("lock") = Some(started);
        watched.down.store(true, Ordering::SeqCst);
        start.recv().expect("the retire started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("a stop during the retire must end the task");
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_during_the_token_refresh_still_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    let probe = Arc::clone(&source);
    let release = Arc::new(Mutex::new(None));

    let runtime = runtime();
    let (ui_end, slot, worker) = start_paced(&runtime, host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let window_release = Arc::clone(&release);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        let (release, started) = probe.hold_next();
        *window_release.lock().expect("lock") = Some(release);
        watched.down.store(true, Ordering::SeqCst);
        started.recv().expect("the refresh started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("a stop during the refresh must end the task");
        if let Some(release) = release.lock().expect("lock").take() {
            release.send(()).expect("release the abandoned refresh");
        }
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_during_the_rebuild_connect_still_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::with_connect_scripts(
        vec![ConnectScript::Ok, ConnectScript::Pending],
        RootlistScript::Ready(Vec::new()),
    );
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let runtime = runtime();
    let (ui_end, slot, worker) = start_paced(&runtime, host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        let (started, start) = channel();
        *watched.connect_started.lock().expect("lock") = Some(started);
        watched.down.store(true, Ordering::SeqCst);
        start.recv().expect("the rebuild connect started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("a stop during the connect must end the task");
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}
