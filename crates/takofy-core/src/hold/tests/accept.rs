use std::sync::Arc;
use std::time::Duration;

use super::super::*;
use super::test_support::*;

use crate::presence::Observation;
use crate::testing::{CLIENT, DEVICE, Reply, SESSION, cluster_of, in_flight, until};
use crate::watch::withdraw;

fn rows() -> Vec<String> {
    vec![TRACK_1.to_owned(), TRACK_2.to_owned(), TRACK_3.to_owned()]
}

fn view(held_here: bool, changed_ms: i64) -> ClusterView {
    ClusterView {
        devices: None,
        holder: (!held_here).then(|| "phone".to_owned()),
        held_here,
        changed_ms,
        playback: None,
    }
}

/// The position one instant of the fake clock lands the snapshot at.
fn position_at(now_ms: i64) -> u32 {
    transfer::landing_position(Some(POSITION_MS), Some(SNAPSHOT_MS), now_ms, false)
}

#[tokio::test]
async fn accept_puts_active_then_answers_success() {
    let world = World::new(timing());
    let id = RequestId(1);
    world.pending(id);

    let landing = world.accept(id, timing()).await.expect("the accept lands");

    assert_eq!(landing.rows, rows());
    assert_eq!(landing.index, 1, "the phone's current track is the row");
    assert_eq!(landing.epoch, 1);
    assert_eq!(
        world.wire.log.entries(),
        ["put:active:start", "put:active:done", "reply:success"],
        "the put comes before the answer, and nothing else is sent"
    );
    assert!(world.shared.presence.active());
}

#[tokio::test]
async fn the_reply_follows_the_done_of_the_put() {
    let world = World::new(timing());
    let id = RequestId(1);
    world.pending(id);

    world.accept(id, timing()).await.expect("the accept lands");

    let entries = world.wire.log.entries();
    let done = entries
        .iter()
        .position(|entry| entry == "put:active:done")
        .expect("the put finished");
    let reply = entries
        .iter()
        .position(|entry| entry == "reply:success")
        .expect("the phone was answered");
    assert!(
        done < reply,
        "the answer must follow the put that landed: {entries:?}"
    );
}

#[tokio::test]
async fn accept_does_not_answer_while_the_put_is_in_flight() {
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

    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.wire.log.entries(),
        ["put:active:start"],
        "nothing is answered while the put is still on the wire"
    );

    release.send(()).expect("release");
    task.await
        .expect("the accept ends")
        .expect("the held accept lands");
    let entries = world.wire.log.entries();
    let done = entries
        .iter()
        .position(|entry| entry == "put:active:done")
        .expect("the released put finished");
    let reply = entries
        .iter()
        .position(|entry| entry == "reply:success")
        .expect("the phone was answered");
    assert!(done < reply, "{entries:?}");
}

#[tokio::test]
async fn a_failed_put_answers_failure_and_never_activates() {
    let world = World::new(timing());
    world.wire.reply(Reply::Refused);
    let id = RequestId(1);
    world.pending(id);

    let result = world.accept(id, timing()).await;

    assert!(matches!(result, Err(ConnectError::PutTimedOut)));
    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:active:done",
            "put:inactive:start",
            "put:inactive:done",
            "reply:failure",
        ],
        "a failed put gives the sound back before it answers failure"
    );
    assert!(!world.shared.presence.active());
}

#[tokio::test]
async fn accept_after_withdraw_refuses_and_sends_nothing() {
    let world = World::new(timing());
    withdraw(&*world.wire, &world.gate, &world.shared.presence, timing())
        .await
        .expect("the withdraw completes");
    let id = RequestId(1);
    world.pending(id);

    assert!(matches!(
        world.accept(id, timing()).await,
        Err(ConnectError::Withdrawn)
    ));
    assert_eq!(world.wire.log.entries(), ["reply:failure"]);
    assert!(world.wire.puts().is_empty());
}

#[tokio::test]
async fn accept_gives_up_busy_when_the_gate_is_held() {
    let world = World::new(timing());
    let held = world.gate.take().await;
    let id = RequestId(1);
    world.pending(id);

    assert!(matches!(
        world.accept(id, timing()).await,
        Err(ConnectError::Busy)
    ));
    assert_eq!(world.wire.log.entries(), ["reply:failure"]);
    assert!(world.wire.puts().is_empty());
    drop(held);
}

#[tokio::test]
async fn accept_without_a_connection_id_refuses() {
    let world = World::new(timing());
    world.wire.set_connection_id("");
    let id = RequestId(1);
    world.pending(id);

    assert!(matches!(
        world.accept(id, timing()).await,
        Err(ConnectError::NoConnectionId)
    ));
    assert_eq!(world.wire.log.entries(), ["reply:failure"]);
    assert!(world.wire.puts().is_empty());
}

#[tokio::test]
async fn accept_refuses_when_the_device_cannot_play() {
    let world = World::new(timing());
    world.shared.presence.set_playable(false);
    let id = RequestId(1);
    world.pending(id);

    assert!(matches!(
        world.accept(id, timing()).await,
        Err(ConnectError::NotPlayable)
    ));
    assert_eq!(world.wire.log.entries(), ["reply:failure"]);
    assert!(
        world.wire.puts().is_empty(),
        "a device that cannot play never reaches the wire"
    );
}

#[tokio::test]
async fn accept_with_an_expired_entry_sends_nothing() {
    let short = Timing {
        answer: Duration::from_millis(20),
        ..timing()
    };
    let world = World::new(short);
    let id = RequestId(1);
    world.pending(id);
    until("the entry to expire", || {
        world.shared.registry.ack_of(id).is_none()
    })
    .await;
    let before = world.wire.log.entries();

    assert!(matches!(
        world.accept(id, short).await,
        Err(ConnectError::Expired)
    ));
    assert_eq!(
        world.wire.log.entries(),
        before,
        "the peek answers nothing and sends nothing"
    );
    assert!(world.wire.puts().is_empty());
}

#[tokio::test]
async fn accept_finding_the_entry_gone_after_the_put_releases_and_does_not_activate() {
    let timing = Timing {
        answer: Duration::from_millis(20),
        ..patient()
    };
    let world = World::new(timing);
    let id = RequestId(1);
    world.pending(id);
    let (release, started) = world.wire.hold();
    let wire = Arc::clone(&world.wire);
    let gate = Arc::clone(&world.gate);
    let shared = Arc::clone(&world.shared);
    let task = tokio::spawn(async move {
        accept(&*wire, &gate, &shared, id, &plan(), resolved(), timing).await
    });
    in_flight(started).await;
    until("the entry to expire during the put", || {
        world.shared.registry.ack_of(id).is_none()
    })
    .await;

    release.send(()).expect("release");
    let result = task.await.expect("the accept ends");

    assert!(matches!(result, Err(ConnectError::Expired)));
    assert!(!world.shared.presence.active());
    let entries = world.wire.log.entries();
    let done = entries
        .iter()
        .position(|entry| entry == "put:active:done")
        .expect("the released put finished");
    assert_eq!(
        &entries[done + 1..],
        ["put:inactive:start", "put:inactive:done"],
        "a put that landed after the phone was answered is given back: {entries:?}"
    );
}

#[tokio::test]
async fn a_slow_active_put_is_cut_off_at_the_put_budget_and_releases_best_effort() {
    let timing = Timing {
        put: Duration::from_millis(40),
        ..patient()
    };
    let world = World::new(Timing {
        answer: Duration::from_secs(30),
        ..timing
    });
    let id = RequestId(1);
    world.pending(id);
    let (release, _started) = world.wire.hold();

    let result = world.accept(id, timing).await;

    // The held put was cut, so it never logged its answer.
    assert!(matches!(result, Err(ConnectError::PutTimedOut)));
    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:inactive:start",
            "put:inactive:done",
            "reply:failure",
        ]
    );
    assert!(!world.shared.presence.active());
    let _ = release.send(());
}

#[test]
fn the_accept_path_fits_inside_the_answer_budget() {
    let timing = Timing::default();

    assert!(
        timing.resolve + timing.gate_wait + timing.put + timing.release < timing.answer,
        "a transfer must be answered before the phone gives up on it"
    );
}

#[tokio::test]
async fn a_successful_accept_marks_active_with_the_replys_changed_time() {
    let world = World::new(timing());
    world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let id = RequestId(1);
    world.pending(id);

    let landing = world.accept(id, timing()).await.expect("the accept lands");

    assert!(world.shared.presence.active());
    assert_eq!(
        world
            .shared
            .presence
            .observe(&view(false, REPLY_MS - 1), Instant::now()),
        Observation::Stale,
        "a push stamped before the reply predates the hold"
    );
    assert_eq!(
        world
            .shared
            .presence
            .observe(&view(false, REPLY_MS + 1), Instant::now()),
        Observation::Lost {
            epoch: landing.epoch
        },
        "a push stamped after the reply is not ours to ignore"
    );
}

#[tokio::test]
async fn an_accept_whose_reply_has_no_changed_time_uses_the_grace_baseline() {
    // The grace is longer than any scheduler stall, so neither assertion
    // below can lose a race with the real clock.
    let timing = Timing {
        grace: Duration::from_secs(30),
        ..timing()
    };
    let world = World::new(timing);
    world.wire.answer_with(cluster_of(DEVICE, 0));
    let id = RequestId(1);
    world.pending(id);

    let landing = world.accept(id, timing).await.expect("the accept lands");

    // An unstamped push is treated as fresh by the `After` baseline, so
    // only the grace baseline ignores it.
    assert_eq!(
        world
            .shared
            .presence
            .observe(&view(false, 0), Instant::now()),
        Observation::Stale
    );
    assert_eq!(
        world.shared.presence.observe(
            &view(false, 0),
            Instant::now() + timing.grace + Duration::from_millis(1)
        ),
        Observation::Lost {
            epoch: landing.epoch
        }
    );
}

#[tokio::test]
async fn the_active_body_is_the_landing_report_as_an_announcement() {
    let world = World::new(timing());
    world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let id = RequestId(1);
    world.pending(id);

    let landing = world.accept(id, timing()).await.expect("the accept lands");

    let state = ActiveState::new(landing.report(), 0).expect("a landing names a track");
    assert_eq!(
        world.wire.puts(),
        vec![Announcement::active(
            DEVICE,
            CLIENT,
            SESSION,
            &state,
            Some(ack()),
            u64::try_from(NOW_MS).expect("a positive instant"),
        )]
    );
    assert_eq!(
        landing.position_ms,
        position_at(NOW_MS),
        "the landing carries the position it was taken at"
    );
}

#[tokio::test]
async fn accept_computes_the_landing_at_the_put_time_not_the_receive_time() {
    let world = World::new(timing());
    let id = RequestId(1);
    world.pending(id);
    world.wire.set_now(NOW_MS);
    // Held by the test, so the accept waits where it reads the clock.
    let held = world.gate.take().await;
    let wire = Arc::clone(&world.wire);
    let gate = Arc::clone(&world.gate);
    let shared = Arc::clone(&world.shared);
    let task = tokio::spawn(async move {
        accept(&*wire, &gate, &shared, id, &plan(), resolved(), timing()).await
    });
    // The accept has peeked its ack and is parked on the gate.
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    let later = NOW_MS + 3_000;
    world.wire.set_now(later);
    drop(held);

    let landing = task
        .await
        .expect("the accept ends")
        .expect("the accept lands");

    assert_eq!(
        landing.position_ms,
        position_at(later),
        "the position is the clock's at the put, not at the receive"
    );
}
