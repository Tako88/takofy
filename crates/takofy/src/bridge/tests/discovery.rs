//! Discovery's asks: the lookup runs away from the reads, and answers once.
//!
//! Every ask ends in exactly one `Update::Home(HomeAnswer::Discovery(..))`. A
//! lookup that is held, dropped for being a repeat, or cut at its budget never
//! leaves its row waiting and never reaches the window as a Home failure.

use super::super::reads::Read;
use super::super::startup::Startup;
use super::super::*;
use super::test_support::*;
use crate::discovery::LOOKUP_DEADLINE;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use takofy_model::home::{
    DigAnswer, DigAsk, DigOutcome, DiscoveryAnswer, DiscoveryArtist, DiscoveryCard, DiscoveryKind,
    DiscoveryOutcome, DiscoverySeed, HomeAnswer, HomeAsk, OutsideId,
};
use tokio::sync::Semaphore;

/// The seed every test looks up.
fn seed() -> DiscoverySeed {
    DiscoverySeed {
        kind: DiscoveryKind::Artist,
        uri: ARTIST.to_owned(),
        name: "A".to_owned(),
        anchor: None,
    }
}

/// The answer one seed and one outcome make.
fn answer(seed: DiscoverySeed, outcome: DiscoveryOutcome) -> Update {
    Update::Home(HomeAnswer::Discovery(DiscoveryAnswer { seed, outcome }))
}

/// The answer one ring makes.
fn dig_answer(seed: DiscoverySeed, hop: u8, outcome: DigOutcome) -> Update {
    Update::Home(HomeAnswer::Dig(DigAnswer { seed, hop, outcome }))
}

/// One ring's ask, honouring the hop it names.
fn dig_ask(hop: u8) -> DigAsk {
    DigAsk {
        seed: seed(),
        hop,
        frontier: vec![OutsideId::from_id(1)],
        held: Vec::new(),
        skip: Vec::new(),
    }
}

/// A ring that found these cards and gave up on nothing.
fn ring(cards: Vec<DiscoveryCard>) -> DigOutcome {
    DigOutcome::Ring {
        cards,
        passed: Vec::new(),
        exhausted: false,
    }
}

/// One neighbour, as a scripted outcome carries it.
fn card(uri: &str) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: uri.to_owned(),
            name: "Neighbour".to_owned(),
        },
        outside: None,
        portrait: None,
        tracks: Vec::new(),
        latest_year: Some(2001),
    }
}

/// Arms the hold with no permits, so a started lookup waits until the test
/// hands it one.
fn hold_lookups(shared: &FakeState) -> Arc<Semaphore> {
    let hold = Arc::new(Semaphore::new(0));
    *shared.lookup_hold.lock().expect("lock") = Some(Arc::clone(&hold));
    hold
}

/// Makes the next watch wait for the returned sender before it answers, so a
/// barrier can be kept running.
fn hold_watch(shared: &FakeState) -> tokio::sync::oneshot::Sender<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    *shared.watch_gate.lock().expect("lock") = Some(gate);
    release
}

/// The notifier a test hands the loop's own bodies directly.
fn notifier() -> (Notifier, Receiver<Update>) {
    let (tx, rx) = channel();
    (
        Notifier {
            tx,
            slot: WakeSlot::default(),
        },
        rx,
    )
}

/// Reads updates until a discovery answer arrives, passing over the rest.
fn await_discovery(ui_end: &UiEnd) -> Update {
    await_home(ui_end, |update| {
        matches!(update, Update::Home(HomeAnswer::Discovery(_)))
    })
}

/// Reads updates until a dig answer arrives, passing over the rest.
fn await_dig(ui_end: &UiEnd) -> Update {
    await_home(ui_end, |update| {
        matches!(update, Update::Home(HomeAnswer::Dig(_)))
    })
}

/// Reads updates until the one `wanted` names arrives.
fn await_home(ui_end: &UiEnd, wanted: impl Fn(&Update) -> bool) -> Update {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match ui_end.updates.recv_timeout(Duration::from_millis(100)) {
            Ok(update) if wanted(&update) => return update,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("the worker went away"),
        }
    }
    panic!("timed out waiting for the home answer");
}

/// Every outcome one lookup may answer with.
fn outcomes() -> Vec<DiscoveryOutcome> {
    vec![
        DiscoveryOutcome::Off,
        DiscoveryOutcome::Unreachable,
        DiscoveryOutcome::Rejected,
        DiscoveryOutcome::Neighbours {
            cards: vec![card("spotify:artist:neighbour")],
            genre: Some("post-rock".to_owned()),
        },
    ]
}

#[test]
fn every_outcome_answers_once_with_the_seed_that_asked() {
    for outcome in outcomes() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Answer(outcome.clone());
        let scripted = outcome.clone();

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_library(&ui_end);
            ui_end
                .commands
                .send(Command::LoadHome(HomeAsk::Discovery { seed: seed() }))
                .expect("home");
            let update = await_discovery(&ui_end);
            expect_no_update(&ui_end);
            update
        });

        assert_eq!(
            update,
            answer(seed(), scripted),
            "the outcome reaches the window as itself, for the seed that asked"
        );
        assert_eq!(
            shared.looked_up.lock().expect("lock").clone(),
            vec![seed().uri],
            "one ask is one lookup"
        );
    }
}

#[test]
fn off_and_unreachable_are_different_answers() {
    assert_ne!(
        answer(seed(), DiscoveryOutcome::Off),
        answer(seed(), DiscoveryOutcome::Unreachable),
        "an off setting is not the outside failing to answer"
    );
}

#[test]
fn a_discovery_ask_never_answers_as_a_home_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") =
        DiscoveryScript::Answer(DiscoveryOutcome::Rejected);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LoadHome(HomeAsk::Discovery { seed: seed() }))
            .expect("home");
        await_discovery(&ui_end)
    });

    assert!(
        !matches!(update, Update::HomeFailed { .. }),
        "a seed the lookup refused is drawn as refused, not as a failed read: {update:?}"
    );
}

#[test]
fn a_held_lookup_does_not_hold_a_barrier() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let hold = hold_lookups(&shared);
    let (started_tx, started_rx) = channel();
    *shared.lookup_started.lock().expect("lock") = Some(started_tx);

    let update = run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LoadHome(HomeAsk::Discovery { seed: seed() }))
            .expect("home");
        started_rx.recv().expect("the lookup started");
        ui_end
            .commands
            .send(Command::SetLiked {
                tracks: vec![TRACK.to_owned()],
                liked: true,
            })
            .expect("like");
        let update = ui_end.updates.recv().expect("like");
        assert_eq!(
            hold.available_permits(),
            0,
            "the barrier completed while the lookup was still held"
        );
        update
    });

    assert_eq!(
        update,
        Update::Liked {
            tracks: vec![TRACK.to_owned()],
            liked: true,
        }
    );
    assert_eq!(
        shared.looked_up.lock().expect("lock").clone(),
        vec![seed().uri],
        "the held lookup is the one the barrier ran beside"
    );
}

#[test]
fn an_ask_sent_while_a_barrier_is_held_still_arrives() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Held];
    let release = hold_watch(&shared);
    let (started_tx, started_rx) = channel();
    *shared.watch_started.lock().expect("lock") = Some(started_tx);

    let update = run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        started_rx.recv().expect("the barrier started");
        ui_end
            .commands
            .send(Command::LoadHome(HomeAsk::Discovery { seed: seed() }))
            .expect("home");
        expect_no_update(&ui_end);
        assert!(
            shared.looked_up.lock().expect("lock").is_empty(),
            "an instant waits behind a barrier that is running"
        );
        release.send(()).expect("release the watch");
        await_discovery(&ui_end)
    });

    assert_eq!(
        update,
        answer(seed(), DiscoveryOutcome::Unreachable),
        "the ask a barrier held outlived it rather than being lost"
    );
}

#[tokio::test(start_paused = true)]
async fn a_lookup_past_its_budget_answers_unreachable() {
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Pending;
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();
    let began = tokio::time::Instant::now();

    spawn_lookup(&host, &notifier, &mut lookups, Asked::Seed(seed()));
    while lookups.tasks.join_next().await.is_some() {}

    assert!(
        tokio::time::Instant::now().duration_since(began) >= LOOKUP_DEADLINE,
        "the lookup was cut at its budget, not sooner"
    );
    assert_eq!(
        answers.recv().expect("answer"),
        answer(seed(), DiscoveryOutcome::Unreachable),
        "a lookup the outside never finished is not a row with nothing in it"
    );
    assert!(
        matches!(answers.try_recv(), Err(TryRecvError::Empty)),
        "one cut lookup sends one answer"
    );
    assert!(lookups.in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_lookup_that_came_apart_answers_unreachable() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Panic;
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();

    runtime.block_on(async {
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Seed(seed()));
        while lookups.tasks.join_next().await.is_some() {}
    });

    assert_eq!(
        answers.recv().expect("answer"),
        answer(seed(), DiscoveryOutcome::Unreachable),
        "a lookup that panicked still answers the row that asked"
    );
    assert_eq!(
        shared.looked_up.lock().expect("lock").clone(),
        vec![seed().uri],
        "the lookup ran before it came apart"
    );
    assert!(lookups.in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_host_with_no_lookup_handle_answers_unreachable() {
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    shared.discovery_absent.store(true, Ordering::SeqCst);
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();

    spawn_lookup(&host, &notifier, &mut lookups, Asked::Seed(seed()));

    assert_eq!(
        lookups.tasks.len(),
        0,
        "there is nothing to run the lookup on"
    );
    assert_eq!(
        answers.recv().expect("answer"),
        answer(seed(), DiscoveryOutcome::Unreachable)
    );
    assert!(matches!(answers.try_recv(), Err(TryRecvError::Empty)));
    assert!(
        shared.looked_up.lock().expect("lock").is_empty(),
        "a host with no handle reads nothing"
    );
}

#[test]
fn a_repeat_ask_for_a_running_seed_answers_once() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let hold = hold_lookups(&shared);
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();

    runtime.block_on(async {
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Seed(seed()));
        until("the lookup started", || {
            !shared.looked_up.lock().expect("lock").is_empty()
        });
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Seed(seed()));
        assert_eq!(
            lookups.tasks.len(),
            1,
            "a seed already on its way is not looked up twice"
        );
        hold.add_permits(1);
        while lookups.tasks.join_next().await.is_some() {}
    });

    assert_eq!(
        shared.looked_up.lock().expect("lock").clone(),
        vec![seed().uri],
        "one lookup ran for the two asks"
    );
    assert_eq!(
        answers.recv().expect("answer"),
        answer(seed(), DiscoveryOutcome::Unreachable)
    );
    assert!(
        matches!(answers.try_recv(), Err(TryRecvError::Empty)),
        "the dropped ask lands on the running lookup's one answer"
    );
    assert!(lookups.in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_stop_aborts_a_held_lookup_without_an_answer() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.lookup_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);
    let commands = ui_end.commands;
    let answers = ui_end.updates;

    let window = move || {
        commands
            .send(Command::LoadHome(HomeAsk::Discovery { seed: seed() }))
            .expect("home");
        started_rx.recv().expect("the lookup started");
    };
    let seen = Arc::clone(&shared);
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop cancels the lookup");
        assert_eq!(
            seen.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop cancels the lookup"
        );
        let drained: Vec<Update> = answers.try_iter().collect();
        assert!(
            !drained
                .iter()
                .any(|update| matches!(update, Update::Home(HomeAnswer::Discovery(_)))),
            "a cancelled lookup answers nobody: {drained:?}"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn the_home_batch_guard_answers_a_misrouted_ask() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let (notifier, answers) = notifier();
    let startup = Startup::new();

    runtime
        .block_on(Read::Home(HomeAsk::Discovery { seed: seed() }).run(&host, &notifier, &startup));

    assert_eq!(
        answers.recv().expect("answer"),
        answer(seed(), DiscoveryOutcome::Unreachable),
        "a misrouted ask is answered without blaming a read"
    );
    assert!(
        matches!(answers.try_recv(), Err(TryRecvError::Empty)),
        "the guard answers once"
    );
    assert!(
        shared.looked_up.lock().expect("lock").is_empty(),
        "the guard reads nothing"
    );
}

#[test]
fn a_dig_is_an_instant_and_never_a_read() {
    let command = Command::LoadHome(HomeAsk::Dig(dig_ask(2)));

    assert_eq!(
        classify(&command),
        Class::Instant,
        "a ring answers from the outside, so it costs the batch nothing"
    );
    assert!(
        matches!(
            Immediate::claim(command.clone()),
            Some(Immediate::Dig { .. })
        ),
        "the instant path must claim the ring, or the press is dropped"
    );
    assert!(
        Read::claim(command).is_none(),
        "a ring must not fall back to the home batch"
    );
}

#[test]
fn a_ring_answers_once_with_the_seed_and_hop_that_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let outcome = ring(vec![card("spotify:artist:ring")]);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Ring(outcome.clone());
    let scripted = outcome.clone();

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LoadHome(HomeAsk::Dig(dig_ask(2))))
            .expect("dig");
        let update = await_dig(&ui_end);
        expect_no_update(&ui_end);
        update
    });

    assert_eq!(
        update,
        dig_answer(seed(), 2, scripted),
        "the ring reaches the window as itself, for the seed and hop that asked"
    );
    assert_eq!(
        shared.dug_up.lock().expect("lock").clone(),
        vec![(2, seed().uri)],
        "one ring is one dig"
    );
    assert!(
        shared.looked_up.lock().expect("lock").is_empty(),
        "a ring is not the seed's first lookup"
    );
}

#[test]
fn a_repeat_ring_for_a_running_hop_answers_once() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let hold = hold_lookups(&shared);
    let outcome = ring(vec![card("spotify:artist:ring")]);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Ring(outcome.clone());
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();

    runtime.block_on(async {
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
        until("the ring started", || {
            !shared.dug_up.lock().expect("lock").is_empty()
        });
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
        assert_eq!(
            lookups.tasks.len(),
            1,
            "a hop already on its way is not dug twice"
        );
        hold.add_permits(1);
        while lookups.tasks.join_next().await.is_some() {}
    });

    assert_eq!(
        shared.dug_up.lock().expect("lock").clone(),
        vec![(2, seed().uri)],
        "one ring ran for the two asks"
    );
    assert_eq!(
        answers.recv().expect("answer"),
        dig_answer(seed(), 2, outcome)
    );
    assert!(
        matches!(answers.try_recv(), Err(TryRecvError::Empty)),
        "the dropped ask lands on the running ring's one answer"
    );
    assert!(lookups.in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_ring_dedupes_by_hop_and_a_seed_is_its_own_key() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Pending;
    let (notifier, _answers) = notifier();
    let mut lookups = Lookups::new();

    runtime.block_on(async {
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
        until("the ring started", || {
            !shared.dug_up.lock().expect("lock").is_empty()
        });
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
        assert_eq!(lookups.tasks.len(), 1, "the same hop is already on its way");
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(3)));
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Seed(seed()));
        assert_eq!(
            lookups.tasks.len(),
            3,
            "a different hop and the seed's first lookup are their own asks"
        );
        lookups.tasks.abort_all();
        while lookups.tasks.join_next().await.is_some() {}
    });
}

#[tokio::test(start_paused = true)]
async fn a_ring_past_its_budget_answers_unreachable() {
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Pending;
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();
    let began = tokio::time::Instant::now();

    spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
    while lookups.tasks.join_next().await.is_some() {}

    assert!(
        tokio::time::Instant::now().duration_since(began) >= LOOKUP_DEADLINE,
        "the ring was cut at its budget, not sooner"
    );
    assert_eq!(
        answers.recv().expect("answer"),
        dig_answer(seed(), 2, DigOutcome::Unreachable),
        "a ring the outside never finished is not a row with nothing in it"
    );
    assert!(
        matches!(answers.try_recv(), Err(TryRecvError::Empty)),
        "one cut ring sends one answer"
    );
    assert!(lookups.in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_ring_that_came_apart_answers_unreachable() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Panic;
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();

    runtime.block_on(async {
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
        while lookups.tasks.join_next().await.is_some() {}
    });

    assert_eq!(
        answers.recv().expect("answer"),
        dig_answer(seed(), 2, DigOutcome::Unreachable),
        "a ring that panicked still answers the row that asked"
    );
    assert_eq!(
        shared.dug_up.lock().expect("lock").clone(),
        vec![(2, seed().uri)],
        "the ring ran before it came apart"
    );
    assert!(lookups.in_flight.lock().expect("lock").is_empty());
}

#[test]
fn the_home_batch_guard_answers_a_misrouted_dig() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let (notifier, answers) = notifier();
    let startup = Startup::new();

    runtime.block_on(Read::Home(HomeAsk::Dig(dig_ask(2))).run(&host, &notifier, &startup));

    assert_eq!(
        answers.recv().expect("answer"),
        dig_answer(seed(), 2, DigOutcome::Unreachable),
        "a misrouted ring is answered without blaming a read"
    );
    assert!(
        matches!(answers.try_recv(), Err(TryRecvError::Empty)),
        "the guard answers once"
    );
    assert!(
        shared.looked_up.lock().expect("lock").is_empty(),
        "the guard reads nothing"
    );
    assert!(
        shared.dug_up.lock().expect("lock").is_empty(),
        "the guard digs nothing"
    );
}

#[test]
fn a_ring_takes_a_lookup_permit() {
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let outcome = ring(vec![card("spotify:artist:ring")]);
    *shared.discovery_script.lock().expect("lock") = DiscoveryScript::Ring(outcome.clone());
    let (notifier, answers) = notifier();
    let mut lookups = Lookups::new();
    lookups.permits = Arc::new(Semaphore::new(0));

    runtime.block_on(async {
        spawn_lookup(&host, &notifier, &mut lookups, Asked::Ring(dig_ask(2)));
        tokio::task::yield_now().await;
        assert!(
            shared.dug_up.lock().expect("lock").is_empty(),
            "a ring reads nothing before it holds a permit"
        );
        lookups.permits.add_permits(1);
        while lookups.tasks.join_next().await.is_some() {}
    });

    assert_eq!(
        shared.dug_up.lock().expect("lock").clone(),
        vec![(2, seed().uri)]
    );
    assert_eq!(
        answers.recv().expect("answer"),
        dig_answer(seed(), 2, outcome)
    );
}
