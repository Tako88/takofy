use std::sync::Arc;
use std::time::Duration;

use librespot::protocol::connect::PutStateRequest;

use super::super::*;
use super::test_support::*;

use crate::testing::{
    CLIENT, DEVICE, FakeWire, Reply, cluster_of, in_flight, recv, start_publish, start_pump, until,
};
use crate::watch::{refresh, withdraw};

/// A body without the echoed command, which only the put that answers a
/// request carries.
fn without_command(announcement: &Announcement) -> PutStateRequest {
    let mut body = announcement.message().clone();
    body.last_command_message_id = 0;
    body.last_command_sent_by_device_id.clear();
    body
}

/// A body without the command or the instants a report's age moves.
fn without_command_or_age(announcement: &Announcement) -> PutStateRequest {
    let mut body = without_command(announcement);
    body.started_playing_at = 0;
    body.client_side_timestamp = 0;
    if let Some(state) = body
        .device
        .as_mut()
        .and_then(|device| device.player_state.as_mut())
    {
        state.timestamp = 0;
        state.position_as_of_timestamp = 0;
    }
    body
}

#[tokio::test]
async fn refresh_while_active_sends_the_active_body() {
    let world = World::new(timing());
    world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let id = RequestId(1);
    world.pending(id);
    world.accept(id, timing()).await.expect("the accept lands");
    let held = world.wire.puts()[0].clone();
    world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));

    refresh(&*world.wire, &world.gate, &world.shared, timing())
        .await
        .expect("a refresh answers");

    let puts = world.wire.puts();
    assert_eq!(puts.len(), 2);
    assert_eq!(
        without_command_or_age(&puts[1]),
        without_command_or_age(&held),
        "a refresh re-asserts the hold, not the stage below it"
    );
    assert!(puts[1].message().is_active);
    assert_eq!(
        held.message().last_command_message_id,
        ack().message_id,
        "the phone's command rides the put that answers it"
    );
    assert_eq!(
        puts[1].message().last_command_message_id,
        0,
        "a later put of the same hold does not claim to answer it again"
    );
    assert!(
        world.shared.presence.active(),
        "the refresh's own reply names this device"
    );
}

#[tokio::test]
async fn a_publisher_flip_while_active_sends_the_active_body() {
    let wire = Arc::new(FakeWire::default());
    wire.set_connection_id("connection-1");
    wire.set_now(NOW_MS);
    let gate = Arc::new(Gate::default());
    let publisher = Timing {
        debounce: Duration::from_millis(300),
        ..timing()
    };
    let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
    let _flipping = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, publisher);
    pump.shared.presence.set_playable(true);
    let id = RequestId(1);
    pending(&pump.shared, &wire, id);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));

    // The flip wakes the publisher, and the hold lands inside its debounce,
    // so the body it puts is the stage at put time.
    accept(
        &*wire,
        &gate,
        &pump.shared,
        id,
        &plan(),
        resolved(),
        timing(),
    )
    .await
    .expect("the accept lands");

    until("the publisher's active body", || {
        wire.puts()
            .iter()
            .filter(|put| put.message().is_active)
            .count()
            == 2
    })
    .await;
    let active: Vec<Announcement> = wire
        .puts()
        .into_iter()
        .filter(|put| put.message().is_active)
        .collect();
    assert_eq!(active.len(), 2, "one accept and one flip, one body each");
    assert_eq!(
        without_command_or_age(&active[1]),
        without_command_or_age(&active[0]),
        "a flip while active re-announces the hold, not a waiting device"
    );
}

#[tokio::test]
async fn a_failed_second_transfer_gives_up_the_hold_it_already_had() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    let first = RequestId(1);
    pending(&pump.shared, &wire, first);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let landing = accept(
        &*wire,
        &gate,
        &pump.shared,
        first,
        &plan(),
        resolved(),
        timing(),
    )
    .await
    .expect("the first accept lands");
    assert!(pump.shared.presence.active());

    // A second hand-over the wire refuses: the phone plays on, so the hold
    // it already had here cannot stand.
    wire.reply(Reply::Refused);
    let second = RequestId(2);
    pending(&pump.shared, &wire, second);

    let result = accept(
        &*wire,
        &gate,
        &pump.shared,
        second,
        &plan(),
        resolved(),
        timing(),
    )
    .await;

    assert!(matches!(result, Err(ConnectError::PutTimedOut)));
    assert!(
        !pump.shared.presence.active(),
        "a hold the phone was told failed is not kept"
    );
    assert_eq!(
        recv(&mut pump.events).await,
        ClusterEvent::Lost {
            epoch: landing.epoch
        },
        "the window is told the hold ended, so it pauses"
    );

    wire.reply(Reply::Cluster);
    refresh(&*wire, &gate, &pump.shared, timing())
        .await
        .expect("a refresh answers");
    assert!(
        !wire.puts().last().expect("a body").message().is_active,
        "the next put does not re-assert a hold the phone was told failed"
    );
}

#[tokio::test]
async fn a_quit_during_an_active_put_still_gives_the_sound_back() {
    let world = World::new(patient());
    let id = RequestId(1);
    world.pending(id);
    let (release, started) = world.wire.hold();
    let wire = Arc::clone(&world.wire);
    let gate = Arc::clone(&world.gate);
    let shared = Arc::clone(&world.shared);
    let task = tokio::spawn(async move {
        accept(&*wire, &gate, &shared, id, &plan(), resolved(), patient()).await
    });
    in_flight(started).await;

    // The window closes where it stands, so the accept is dropped mid-put
    // and never reaches the hold it would have recorded.
    task.abort();
    assert!(task.await.is_err(), "the accept was dropped, not finished");
    let _ = release.send(());

    withdraw(&*world.wire, &world.gate, &world.shared.presence, timing())
        .await
        .expect("the withdraw completes");

    assert!(!world.shared.presence.active());
    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:inactive:start",
            "put:inactive:done"
        ],
        "a put nobody answered is given back on the way out"
    );
}

#[tokio::test]
async fn an_accept_whose_player_dies_during_the_put_answers_failure_and_does_not_activate() {
    let world = World::new(patient());
    let id = RequestId(1);
    world.pending(id);
    let (release, started) = world.wire.hold();
    let wire = Arc::clone(&world.wire);
    let gate = Arc::clone(&world.gate);
    let shared = Arc::clone(&world.shared);
    let task = tokio::spawn(async move {
        accept(&*wire, &gate, &shared, id, &plan(), resolved(), patient()).await
    });
    in_flight(started).await;

    // The player dies while the put is on the wire.
    world.shared.presence.set_playable(false);
    release.send(()).expect("release");
    let result = task.await.expect("the accept ends");

    assert!(matches!(result, Err(ConnectError::NotPlayable)));
    assert!(
        !world.shared.presence.active(),
        "a device that cannot play never keeps the sound"
    );
    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:active:done",
            "put:inactive:start",
            "put:inactive:done",
            "reply:failure",
        ],
        "the sound a landed put took is given back before the phone is told"
    );
}

#[tokio::test]
async fn playable_false_while_active_releases_and_announces_the_controller() {
    let world = World::new(timing());
    world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let id = RequestId(1);
    world.pending(id);
    world.accept(id, timing()).await.expect("the accept lands");
    assert!(world.shared.presence.active());

    world.shared.presence.set_playable(false);
    release(&*world.wire, &world.gate, &world.shared, timing())
        .await
        .expect("the release answers");

    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:active:done",
            "reply:success",
            "put:inactive:start",
            "put:inactive:done",
            "put:controller:start",
            "put:controller:done",
        ],
        "the sound is given back before the device says what it is now"
    );
    assert_eq!(
        world.wire.puts().last(),
        Some(&Announcement::controller(DEVICE, CLIENT))
    );
    assert!(!world.shared.presence.active());
}
