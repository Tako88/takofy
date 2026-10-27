use std::collections::HashMap;

use librespot::core::dealer::protocol::PayloadValue;
use protobuf::MessageField;

use super::*;
use crate::loss::Baseline;
use crate::testing::{
    CLIENT, DEVICE, FakeWire, Reply, SESSION, bundle, cluster, empty, in_flight, quiet, recv,
    start_publish, start_pump, until, view_of_event,
};

fn timing() -> Timing {
    Timing {
        id_wait: Duration::from_millis(20),
        withdraw: Duration::from_millis(50),
        gate_wait: Duration::from_millis(30),
        answer: Duration::from_millis(50),
        resolve: Duration::from_millis(20),
        put: Duration::from_millis(50),
        hand_over: Duration::from_millis(50),
        release: Duration::from_millis(20),
        grace: Duration::from_millis(50),
        debounce: Duration::from_millis(20),
    }
}

/// How many puts of one kind the wire has seen.
fn starts(wire: &FakeWire, kind: &str) -> usize {
    let wanted = format!("put:{kind}:start");
    wire.log
        .entries()
        .iter()
        .filter(|entry| entry.as_str() == wanted)
        .count()
}

/// Pins the safety invariant on what the pump, the refresh and the publisher
/// actually sent.
fn assert_never_active(wire: &FakeWire) {
    assert!(
        wire.puts().iter().all(|put| !put.message().is_active),
        "an announcement that takes the sound reached the wire"
    );
    let entries = wire.log.entries();
    assert!(
        !entries.iter().any(|entry| entry.contains(":active:")),
        "an active body reached the wire: {entries:?}"
    );
}

#[tokio::test]
async fn the_first_connection_id_pins_and_sends_the_controller_body() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), gate, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = recv(&mut pump.events).await;

    assert_eq!(wire.connection_id(), "connection-1");
    // `connect`'s own test pins this body against the probed literal, so
    // equality with its only constructor is equality with the wire's bytes.
    assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
}

#[tokio::test]
async fn a_reply_becomes_a_view_and_a_garbage_one_only_fails() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), gate, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let view = view_of_event(recv(&mut pump.events).await);
    assert_eq!(
        view.devices
            .expect("a roster")
            .first()
            .map(|d| d.id.as_str()),
        Some("phone")
    );
    assert_eq!(view.holder.as_deref(), Some("phone"));

    wire.reply(Reply::Garbage);
    pump.ids.send(Ok("connection-2".to_owned())).expect("id");
    assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);
    assert_eq!(
        wire.puts().len(),
        2,
        "the pump keeps announcing after a bad reply"
    );

    wire.reply(Reply::Fail);
    pump.ids.send(Ok("connection-3".to_owned())).expect("id");
    assert_eq!(
        recv(&mut pump.events).await,
        ClusterEvent::Failed,
        "a failed announce is the same silence as an unreadable one"
    );

    wire.reply(Reply::Cluster);
    pump.ids.send(Ok("connection-4".to_owned())).expect("id");
    assert!(matches!(
        recv(&mut pump.events).await,
        ClusterEvent::View(_)
    ));
}

#[tokio::test]
async fn a_missing_connection_id_fails_once_and_a_late_one_still_announces() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), gate, timing());

    assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        pump.events.try_recv().is_err(),
        "the one silence is reported once, not on a loop"
    );

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    assert!(matches!(
        recv(&mut pump.events).await,
        ClusterEvent::View(_)
    ));
    assert_eq!(wire.puts().len(), 1);
}

#[tokio::test]
async fn a_second_connection_id_re_pins_and_announces_again() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), gate, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = recv(&mut pump.events).await;
    pump.ids.send(Ok("connection-2".to_owned())).expect("id");
    let _ = recv(&mut pump.events).await;

    assert_eq!(wire.connection_id(), "connection-2");
    assert_eq!(wire.puts().len(), 2);
}

#[tokio::test]
async fn withdraw_deletes_then_closes_and_no_later_announce_sends() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = recv(&mut pump.events).await;

    withdraw(&*wire, &gate, &pump.shared.presence, timing())
        .await
        .expect("a withdraw on a watched device completes");
    assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
    assert_eq!(wire.closes.load(Ordering::SeqCst), 1);
    assert_eq!(
        &*wire.exit_calls.lock().expect("lock"),
        &["delete", "close"],
        "the device is deleted before the dealer closes"
    );

    pump.ids.send(Ok("connection-2".to_owned())).expect("id");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        wire.puts().len(),
        1,
        "a connection id after the delete must not re-create the device"
    );
}

#[tokio::test]
async fn a_delete_that_never_answers_releases_within_the_bound() {
    let wire = Arc::new(FakeWire::default());
    wire.delete_hangs.store(true, Ordering::SeqCst);
    let gate = Arc::new(Gate::default());

    let started = std::time::Instant::now();
    let result = withdraw(&*wire, &gate, &Presence::default(), timing()).await;
    let waited = started.elapsed();

    assert!(matches!(result, Err(ConnectError::WithdrawTimedOut)));
    assert!(waited >= timing().withdraw, "the bound is the wait");
    assert!(waited < Duration::from_secs(1), "the exit cannot hang");
    assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
    assert_eq!(
        wire.closes.load(Ordering::SeqCst),
        0,
        "a delete that never answers cannot reach the close"
    );
}

#[tokio::test]
async fn a_withdraw_that_cannot_take_the_gate_still_blocks_later_announces() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    // Held by something a withdraw cannot cut, so the bound is what ends it.
    let held = gate.take().await;

    let result = withdraw(&*wire, &gate, &Presence::default(), timing()).await;
    assert!(
        matches!(result, Err(ConnectError::WithdrawTimedOut)),
        "the held gate is past the bound"
    );
    assert_eq!(wire.deletes.load(Ordering::SeqCst), 0);
    drop(held);

    let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        wire.puts().len(),
        0,
        "a withdraw that timed out still marks the device gone"
    );
}

#[tokio::test]
async fn refresh_without_a_connection_id_sends_nothing() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());

    assert!(matches!(
        refresh(&*wire, &gate, &shared, timing()).await,
        Err(ConnectError::NoConnectionId)
    ));
    assert!(wire.puts().is_empty());

    wire.set_connection_id("connection-1");
    let view = refresh(&*wire, &gate, &shared, timing())
        .await
        .expect("a refresh answers");
    assert_eq!(view.holder.as_deref(), Some("phone"));
    assert_eq!(wire.puts().len(), 1);
}

#[tokio::test]
async fn a_refresh_behind_a_blocked_announce_gives_up_as_busy() {
    let wire = Arc::new(FakeWire::default());
    wire.set_connection_id("connection-1");
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());
    let patient = Timing {
        put: Duration::from_secs(2),
        ..timing()
    };
    let (release, started) = wire.hold();

    let first_wire = Arc::clone(&wire);
    let first_gate = Arc::clone(&gate);
    let first_shared = Arc::clone(&shared);
    let first =
        tokio::spawn(
            async move { refresh(&*first_wire, &first_gate, &first_shared, patient).await },
        );
    in_flight(started).await;

    assert!(matches!(
        refresh(&*wire, &gate, &shared, timing()).await,
        Err(ConnectError::Busy)
    ));
    assert_eq!(wire.puts().len(), 1, "the busy refresh sends nothing");

    release.send(()).expect("release");
    first
        .await
        .expect("the first refresh ends")
        .expect("the held announce answers");
}

#[tokio::test]
async fn a_skip_waits_for_the_gate_and_the_rest_close_no_strategy() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let held = gate.take().await;

    assert!(
        matches!(
            steer(&*wire, &gate, "phone", &Steer::SkipNext, timing()).await,
            Err(ConnectError::Busy)
        ),
        "a skip gives up rather than wait on a busy wire"
    );
    assert!(
        wire.commands().is_empty(),
        "a skipped steer never reaches the strategy"
    );

    steer(&*wire, &gate, "phone", &Steer::Pause, timing())
        .await
        .expect("a pause names an absolute state, so it never waits");
    assert_eq!(
        wire.commands(),
        vec![(
            "phone".to_owned(),
            r#"{"command":{"endpoint":"pause"}}"#.to_owned(),
            false
        )]
    );
    drop(held);

    for (action, once) in [
        (Steer::Pause, false),
        (Steer::Resume, false),
        (Steer::SeekTo(60_000), false),
        (Steer::SkipNext, true),
        (Steer::SkipPrev, true),
    ] {
        steer(&*wire, &gate, "phone", &action, timing())
            .await
            .expect("a free wire answers");
        assert_eq!(
            wire.commands().last().map(|c| c.2),
            Some(once),
            "{action:?} carries the wrong attempt budget"
        );
    }
}

#[tokio::test]
async fn a_push_becomes_a_view_and_a_garbage_one_only_fails() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), gate, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = recv(&mut pump.events).await;

    let update = ClusterUpdate {
        cluster: MessageField::some(cluster()),
        ..Default::default()
    };
    pump.pushes.send(Ok(update)).expect("push");
    let view = view_of_event(recv(&mut pump.events).await);
    assert_eq!(view.holder.as_deref(), Some("phone"));

    pump.pushes
        .send(Err(librespot::core::Error::failed_precondition(
            std::io::Error::other("garbage"),
        )))
        .expect("push");
    assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);

    let update = ClusterUpdate {
        cluster: MessageField::some(cluster()),
        ..Default::default()
    };
    pump.pushes.send(Ok(update)).expect("push");
    assert!(
        matches!(recv(&mut pump.events).await, ClusterEvent::View(_)),
        "the pump keeps reading after a bad push"
    );
    assert_eq!(wire.puts().len(), 1, "a push needs no announce");
}

#[tokio::test]
async fn withdraw_while_active_releases_before_it_deletes() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());
    shared.presence.activate(Baseline::After(1));

    withdraw(&*wire, &gate, &shared.presence, timing())
        .await
        .expect("the withdraw completes");

    assert_eq!(
        &*wire.exit_calls.lock().expect("lock"),
        &["inactive", "delete", "close"],
        "the sound is given back before the device goes"
    );
    assert_eq!(
        wire.log.entries(),
        ["put:inactive:start", "put:inactive:done"],
        "the give-back is one bounded put"
    );
}

#[tokio::test]
async fn withdraw_when_idle_does_not_release() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());

    withdraw(&*wire, &gate, &shared.presence, timing())
        .await
        .expect("the withdraw completes");

    assert_eq!(
        &*wire.exit_calls.lock().expect("lock"),
        &["delete", "close"],
        "a device that never held the sound has nothing to give back"
    );
    assert!(wire.log.entries().is_empty());
}

#[tokio::test]
async fn a_failed_release_still_deletes() {
    let wire = Arc::new(FakeWire::default());
    wire.refuse_inactive();
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());
    shared.presence.activate(Baseline::After(1));

    withdraw(&*wire, &gate, &shared.presence, timing())
        .await
        .expect("the withdraw completes");

    assert_eq!(
        &*wire.exit_calls.lock().expect("lock"),
        &["inactive", "delete", "close"],
        "a give-back that failed does not cost the device its delete"
    );
    assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
    assert_eq!(wire.closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_hung_release_is_cut_at_its_budget_and_the_delete_still_runs() {
    let wire = Arc::new(FakeWire::default());
    wire.hang_inactive();
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());
    shared.presence.activate(Baseline::After(1));
    let exit = Timing {
        release: Duration::from_millis(20),
        withdraw: Duration::from_millis(200),
        ..timing()
    };

    let started = std::time::Instant::now();
    withdraw(&*wire, &gate, &shared.presence, exit)
        .await
        .expect("the give-back's own bound frees the exit");
    let waited = started.elapsed();

    assert!(waited >= exit.release, "the give-back is bounded");
    assert!(waited < exit.withdraw, "and it is a sub-budget of the exit");
    assert_eq!(
        &*wire.exit_calls.lock().expect("lock"),
        &["inactive", "delete", "close"],
        "a give-back that never answers does not cost the device its delete"
    );
}

#[tokio::test]
async fn a_withdraw_that_times_out_during_release_still_blocks_announces() {
    let wire = Arc::new(FakeWire::default());
    wire.hang_inactive();
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());
    shared.presence.activate(Baseline::After(1));
    let exit = Timing {
        release: Duration::from_millis(100),
        withdraw: Duration::from_millis(20),
        ..timing()
    };

    assert!(matches!(
        withdraw(&*wire, &gate, &shared.presence, exit).await,
        Err(ConnectError::WithdrawTimedOut)
    ));
    assert_eq!(
        wire.deletes.load(Ordering::SeqCst),
        0,
        "a give-back that ate the exit leaves no time to delete"
    );

    let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        wire.puts().is_empty(),
        "a withdraw that timed out still marks the device gone"
    );
}

#[test]
fn a_target_id_cannot_change_the_command_path() {
    assert_eq!(
        command_endpoint("own", "phone").expect("a plain id is one segment"),
        "/connect-state/v1/player/command/from/own/to/phone"
    );
    assert!(matches!(
        command_endpoint("own", "phone/../other"),
        Err(ConnectError::InvalidTarget)
    ));
    assert!(matches!(
        command_endpoint("own", "phone?query"),
        Err(ConnectError::InvalidTarget)
    ));
}

#[test]
fn a_target_cannot_change_the_path() {
    hand_over_path("own", "phone").expect("a plain id is one segment");
    assert!(
        matches!(
            hand_over_path("own", "own"),
            Err(ConnectError::InvalidTarget)
        ),
        "a move to this device's own id is not a hand-over"
    );
    for target in [
        "",
        ".",
        "..",
        "phone/../other",
        "phone/other",
        "phone?query",
        "phone#fragment",
        "phone%2Fother",
        "ph one",
        "phone\\other",
        "phone\u{0}",
    ] {
        assert!(
            matches!(
                hand_over_path("own", target),
                Err(ConnectError::InvalidTarget)
            ),
            "{target:?} must not widen the path"
        );
    }
    for source in ["", "own/other", "own?query"] {
        assert!(
            matches!(
                hand_over_path(source, "phone"),
                Err(ConnectError::InvalidTarget)
            ),
            "{source:?} must not widen the path"
        );
    }
}

#[tokio::test]
async fn the_pump_ends_when_the_window_goes() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(wire, gate, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = recv(&mut pump.events).await;
    drop(pump.events);

    tokio::time::timeout(Duration::from_secs(2), pump.task)
        .await
        .expect("the pump must end with the window")
        .expect("the pump must not panic");
}

#[tokio::test]
async fn the_pump_ends_when_the_streams_end() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let (events, mut rx) = unbounded_channel();
    let shared = Shared::new(
        Arc::new(Presence::default()),
        Arc::new(Latest::default()),
        Arc::new(Registry::new(timing())),
        events.downgrade(),
    );

    let task = tokio::spawn(pump(
        Arc::clone(&wire),
        gate,
        empty(),
        empty(),
        shared,
        events,
        timing(),
    ));
    assert!(
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("the pump must end")
            .is_none()
    );
    task.await.expect("the pump must not panic");
}

#[test]
fn the_connection_id_is_the_pusher_header() {
    let mut headers = HashMap::new();
    headers.insert(CONNECTION_HEADER.to_owned(), "connection-1".to_owned());
    let message = Message {
        headers,
        payload: PayloadValue::Empty,
        uri: CONNECTION_URI.to_owned(),
    };
    assert_eq!(
        connection_id_of(&message).expect("the header names the id"),
        "connection-1"
    );

    let bare = Message {
        headers: HashMap::new(),
        payload: PayloadValue::Empty,
        uri: CONNECTION_URI.to_owned(),
    };
    assert!(matches!(
        connection_id_of(&bare),
        Err(ConnectError::NoConnectionId)
    ));
}

#[tokio::test]
async fn the_first_announce_is_a_controller_until_something_can_play() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), gate, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
    assert!(!wire.puts()[0].message().is_active);
}

#[tokio::test]
async fn becoming_playable_re_announces_ready_once() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    assert!(pump.shared.presence.set_playable(true));
    until("the ready announce", || wire.puts().len() == 2).await;
    assert_eq!(wire.puts()[1], Announcement::ready(DEVICE, CLIENT, SESSION));

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        wire.puts().len(),
        2,
        "one flip is one announce, not one per check"
    );
    assert_never_active(&wire);
}

#[tokio::test]
async fn becoming_unplayable_re_announces_the_controller() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    assert!(pump.shared.presence.set_playable(true));
    until("the ready announce", || wire.puts().len() == 2).await;

    assert!(pump.shared.presence.set_playable(false));
    until("the controller announce", || wire.puts().len() == 3).await;
    assert_eq!(wire.puts()[2], Announcement::controller(DEVICE, CLIENT));
}

#[tokio::test]
async fn a_flip_before_the_connection_id_sends_nothing_until_the_id_arrives() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let patient = Timing {
        id_wait: Duration::from_secs(2),
        ..timing()
    };
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, patient);

    assert!(pump.shared.presence.set_playable(true));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        wire.puts().is_empty(),
        "a put has no connection id to carry yet"
    );

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);
    assert_eq!(
        wire.puts(),
        vec![Announcement::ready(DEVICE, CLIENT, SESSION)],
        "the id announces the stage the device is in"
    );
}

#[tokio::test]
async fn a_flip_after_withdraw_sends_nothing() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    withdraw(&*wire, &gate, &pump.shared.presence, timing())
        .await
        .expect("the withdraw completes");

    assert!(pump.shared.presence.set_playable(true));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        wire.puts().len(),
        1,
        "a device that is gone announces nothing"
    );
}

#[tokio::test]
async fn a_flip_while_an_announce_is_held_waits_for_the_gate() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let patient = Timing {
        put: Duration::from_secs(2),
        gate_wait: Duration::from_secs(2),
        ..timing()
    };
    let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, patient);

    let (release, started) = wire.hold();
    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    in_flight(started).await;

    assert!(pump.shared.presence.set_playable(true));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        wire.puts().len(),
        1,
        "the flip waits behind the announce that holds the gate"
    );

    let _ = release.send(());
    until("the ready announce", || wire.puts().len() == 2).await;
    assert_eq!(wire.puts()[1], Announcement::ready(DEVICE, CLIENT, SESSION));
}

#[tokio::test]
async fn a_refresh_announces_the_current_stage() {
    let wire = Arc::new(FakeWire::default());
    wire.set_connection_id("connection-1");
    let gate = Arc::new(Gate::default());
    let shared = bundle(timing());

    let _ = refresh(&*wire, &gate, &shared, timing())
        .await
        .expect("a controller refresh answers");
    assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
    assert_never_active(&wire);

    assert!(shared.presence.set_playable(true));
    let _ = refresh(&*wire, &gate, &shared, timing())
        .await
        .expect("a ready refresh answers");
    assert_eq!(wire.puts()[1], Announcement::ready(DEVICE, CLIENT, SESSION));
    assert_never_active(&wire);
}

#[tokio::test]
async fn a_put_that_never_answers_is_cut_off_at_the_budget_and_reported_failed() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let short = Timing {
        put: Duration::from_millis(40),
        ..timing()
    };
    let (release, started) = wire.hold();
    let mut pump = start_pump(Arc::clone(&wire), gate, short);

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    in_flight(started).await;

    assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);
    assert_eq!(
        wire.log.entries(),
        vec!["put:controller:start"],
        "a cut put never reached its answer"
    );
    let _ = release.send(());
}

#[tokio::test]
async fn a_withdraw_cuts_an_in_flight_put_short_and_still_deletes() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let patient = Timing {
        put: Duration::from_secs(2),
        ..timing()
    };
    let (release, started) = wire.hold();
    let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    in_flight(started).await;

    let patient_exit = Timing {
        withdraw: Duration::from_millis(500),
        ..timing()
    };
    withdraw(&*wire, &gate, &pump.shared.presence, patient_exit)
        .await
        .expect("the cut announce frees the gate, so the exit finishes");
    assert_eq!(
        &*wire.exit_calls.lock().expect("lock"),
        &["delete", "close"],
        "a cut put does not cost the device its delete"
    );
    assert_eq!(
        wire.log.entries(),
        vec!["put:controller:start"],
        "the in-flight put was cut, not answered"
    );
    let _ = release.send(());
}

#[tokio::test]
async fn a_slow_publisher_put_does_not_delay_a_cluster_push() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let patient = Timing {
        put: Duration::from_secs(2),
        gate_wait: Duration::from_secs(2),
        ..timing()
    };
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, patient);

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    let (release, started) = wire.hold();
    assert!(pump.shared.presence.set_playable(true));
    in_flight(started).await;

    let update = ClusterUpdate {
        cluster: MessageField::some(cluster()),
        ..Default::default()
    };
    pump.pushes.send(Ok(update)).expect("push");
    let view = view_of_event(recv(&mut pump.events).await);
    assert_eq!(view.holder.as_deref(), Some("phone"));

    let entries = wire.log.entries();
    assert!(
        entries.contains(&"put:ready:start".to_owned()),
        "the held put is the publisher's: {entries:?}"
    );
    assert!(
        !entries.contains(&"put:ready:done".to_owned()),
        "the push was read while the put was still on the wire: {entries:?}"
    );

    let _ = release.send(());
    until("the held put to answer", || {
        wire.log.entries().contains(&"put:ready:done".to_owned())
    })
    .await;
}

#[tokio::test]
async fn a_pump_that_ends_closes_the_stream_the_window_reads() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    // Both dealer streams end, as they do when the session dies.
    drop(pump.ids);
    drop(pump.pushes);

    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        while pump.events.recv().await.is_some() {}
    })
    .await;
    assert!(
        closed.is_ok(),
        "the stream must close when its pump ends, or the window never learns the devices are off"
    );
}

#[tokio::test]
async fn a_failed_flip_put_is_retried_up_to_its_limit() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    wire.reply(Reply::Refused);
    assert!(pump.shared.presence.set_playable(true));
    until("the run of failed puts to end", || {
        starts(&wire, "ready") == usize::from(publish::PUT_ATTEMPTS)
    })
    .await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        starts(&wire, "ready"),
        usize::from(publish::PUT_ATTEMPTS),
        "a failed put is retried, and the retries are bounded"
    );
}

#[tokio::test]
async fn a_failed_flip_put_reports_failed_once_per_run() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    wire.reply(Reply::Refused);
    assert!(pump.shared.presence.set_playable(true));
    until("the run of failed puts to end", || {
        starts(&wire, "ready") == usize::from(publish::PUT_ATTEMPTS)
    })
    .await;

    assert_eq!(
        quiet(&mut pump.events, Duration::from_millis(150)).await,
        vec![ClusterEvent::Failed],
        "one Failed per run of failures, not one per attempt"
    );
}

#[tokio::test]
async fn a_successful_retry_clears_the_failure_run() {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);

    wire.replies([Reply::Refused, Reply::Cluster]);
    assert!(pump.shared.presence.set_playable(true));
    until("the retry to land", || {
        wire.log
            .entries()
            .iter()
            .filter(|entry| entry.as_str() == "put:ready:done")
            .count()
            == 2
    })
    .await;
    assert_eq!(starts(&wire, "ready"), 2, "the failure cost one retry");

    let events = quiet(&mut pump.events, Duration::from_millis(150)).await;
    assert!(
        matches!(
            events.as_slice(),
            [ClusterEvent::Failed, ClusterEvent::View(_)]
        ),
        "one Failed for the run, then the view the landed put answered with: {events:?}"
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        starts(&wire, "ready"),
        2,
        "a put that landed is not retried"
    );
}
