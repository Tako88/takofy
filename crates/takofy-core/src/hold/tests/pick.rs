use std::time::Duration;

use super::super::*;
use super::test_support::*;

use crate::testing::{DEVICE, Reply, cluster_of, pushed, quiet, recv, view_of_event};
use crate::watch::withdraw;

#[tokio::test]
async fn pick_puts_active_then_hands_over_in_that_order() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));

    let result = world.pick("phone").await;

    assert!(result.is_ok(), "the pick lands: {result:?}");
    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:active:done",
            "hand_over:start",
            "hand_over:done",
        ],
        "the sound is taken before it is handed away, and the move finished: \
         a hand-over cut at its budget leaves only its own start behind"
    );
    let moved = world.wire.hand_overs.lock().expect("lock").clone();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].0, "phone", "the target the press named");
    assert!(
        moved[0].1.contains("\"endpoint\":\"transfer\""),
        "the move carries the transfer command: {}",
        moved[0].1
    );
    assert!(
        world.shared.presence.active(),
        "the put that comes with a pick is what makes the hand-over work"
    );
}

#[tokio::test]
async fn a_pick_names_the_current_row_by_the_accounts_own_uid() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    // The account's own names for the rows, which is the only naming the
    // target can match against its context.
    world.wire.rows(vec![
        transfer_out::ContextRow {
            uri: "spotify:track:before".to_owned(),
            uid: "server-before".to_owned(),
        },
        transfer_out::ContextRow {
            uri: TRACK_1.to_owned(),
            uid: "server-nine".to_owned(),
        },
    ]);

    world.pick("phone").await.expect("the pick lands");

    let body = world.wire.hand_overs.lock().expect("lock")[0].1.clone();
    let state = transfer_out::parse_body(&body).expect("the body carries a state");
    assert_eq!(
        state.current_session.current_uid.as_deref(),
        Some("server-nine"),
        "the row the sound is on is named the way the account names it"
    );
}

#[tokio::test]
async fn a_context_that_cannot_be_read_still_moves_the_near_rows() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    world.wire.refuse_context_rows();

    world.pick("phone").await.expect("the pick lands anyway");

    let body = world.wire.hand_overs.lock().expect("lock")[0].1.clone();
    let state = transfer_out::parse_body(&body).expect("the body carries a state");
    assert!(
        state.current_session.current_uid.is_some(),
        "the target is still told which row to start on"
    );
    assert_eq!(
        state.playback.current_track.uri.as_deref(),
        Some(TRACK_1),
        "and which track that is"
    );
}

#[tokio::test]
async fn pick_when_already_active_skips_the_put() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    let id = RequestId(1);
    world.pending(id);
    world.accept(id, timing()).await.expect("the accept lands");
    let puts = world.wire.puts().len();
    let entries = world.wire.log.entries();

    world.pick("phone").await.expect("the pick lands");

    assert_eq!(
        world.wire.puts().len(),
        puts,
        "a device that holds the sound has nothing to put"
    );
    assert_eq!(
        &world.wire.log.entries()[entries.len()..],
        ["hand_over:start", "hand_over:done"],
        "only the hand-over is sent"
    );
}

#[tokio::test]
async fn a_move_without_a_context_refuses_before_the_put() {
    let world = World::new(timing());
    let mut staged = report(Status::Playing);
    staged.context = None;
    world.shared.latest.set(Some(staged));

    let result = world.pick("phone").await;

    assert!(
        matches!(result, Err(ConnectError::NoContext)),
        "a move with nothing to play must not take the sound: {result:?}"
    );
    assert!(
        !world
            .wire
            .log
            .entries()
            .iter()
            .any(|entry| entry.starts_with("put:")),
        "the refusal comes before the activation put: {:?}",
        world.wire.log.entries()
    );
    assert!(world.wire.puts().is_empty());
    assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
    assert!(!world.shared.presence.active());
}

#[tokio::test]
async fn pick_without_a_playing_report_refuses_and_sends_nothing() {
    for latest in [None, Some(report(Status::Paused))] {
        let world = World::new(timing());
        world.shared.latest.set(latest.clone());

        let result = world.pick("phone").await;

        assert!(
            matches!(result, Err(ConnectError::NotPlaying)),
            "a stage with nothing moving cannot be moved: {result:?}"
        );
        assert_eq!(world.wire.log.entries(), Vec::<String>::new());
        assert!(world.wire.puts().is_empty());
        assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
        assert!(!world.shared.presence.active());
    }
}

#[tokio::test]
async fn a_failed_active_put_sends_no_hand_over() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    world.wire.reply(Reply::Refused);

    let result = world.pick("phone").await;

    assert!(matches!(result, Err(ConnectError::PutTimedOut)));
    assert_eq!(
        world.wire.log.entries(),
        [
            "put:active:start",
            "put:active:done",
            "put:inactive:start",
            "put:inactive:done",
        ],
        "a put that failed is given back instead of being handed on"
    );
    assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
    assert!(
        !world.shared.presence.active(),
        "a hold nobody confirmed is not ours"
    );
}

#[tokio::test]
async fn a_hung_hand_over_is_cut_at_its_budget() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    world.wire.hang_hand_over();
    let budget = Duration::from_millis(30);
    let short = Timing {
        hand_over: budget,
        ..timing()
    };

    let started = Instant::now();
    let result = world.pick_with("phone", short).await;

    assert!(
        matches!(result, Err(ConnectError::HandOverTimedOut)),
        "a move that never answers is cut: {result:?}"
    );
    assert!(
        started.elapsed() >= budget,
        "only the budget may end the move"
    );
    assert_eq!(
        world.wire.log.entries().last().map(String::as_str),
        Some("hand_over:start"),
        "the move was cut before it answered"
    );
    assert!(
        world.shared.presence.active(),
        "a cut hand-over is not a give-back: the account still names this device"
    );
    assert!(!world.shared.presence.picking());
    // Every other write and the exit path wait on the gate, so it has to
    // come back with the call.
    let _free = tokio::time::timeout(Duration::from_millis(50), world.gate.take())
        .await
        .expect("the gate is free once the move is cut");
}

#[tokio::test]
async fn a_second_pick_before_the_first_settles_is_refused() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    pump.shared.latest.set(Some(report(Status::Playing)));
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));

    pick(&*wire, &gate, &pump.shared, "phone", timing())
        .await
        .expect("the pick lands");
    let sent = wire.log.entries();

    let result = pick(&*wire, &gate, &pump.shared, "laptop", timing()).await;

    assert!(
        matches!(result, Err(ConnectError::HandOverPending)),
        "the sound cannot be handed on twice at once: {result:?}"
    );
    assert_eq!(wire.log.entries(), sent, "a second pick sends nothing");
    let moved = wire.hand_overs.lock().expect("lock").clone();
    assert_eq!(
        moved
            .iter()
            .map(|(to, _)| to.as_str())
            .collect::<Vec<&str>>(),
        ["phone"],
        "only the first target was asked for"
    );

    // The view naming the target is what ends the hold, so it is what lets
    // the next pick through.
    pump.pushes
        .send(Ok(pushed(cluster_of("phone", REPLY_MS + 1))))
        .expect("push");
    assert_eq!(
        recv(&mut pump.events).await,
        ClusterEvent::Lost { epoch: 1 }
    );
    assert!(!pump.shared.presence.picking());

    pick(&*wire, &gate, &pump.shared, "laptop", timing())
        .await
        .expect("a settled hold can be handed on");
    assert_eq!(
        wire.hand_overs
            .lock()
            .expect("lock")
            .iter()
            .map(|(to, _)| to.as_str())
            .collect::<Vec<&str>>(),
        ["phone", "laptop"]
    );
}

#[tokio::test]
async fn a_failed_hand_over_leaves_us_active() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    pump.shared.latest.set(Some(report(Status::Playing)));
    wire.refuse_hand_over();
    let sent = wire.log.entries().len();

    let result = pick(&*wire, &gate, &pump.shared, "phone", timing()).await;

    assert!(result.is_err(), "the failure is returned: {result:?}");
    assert_eq!(
        &wire.log.entries()[sent..],
        ["put:active:start", "put:active:done", "hand_over:start"],
        "a hand-over that failed gives nothing back: the sound never left"
    );
    assert!(
        pump.shared.presence.active(),
        "the account still names this device, so the hold stays"
    );
    assert!(!pump.shared.presence.picking());
    assert_eq!(
        quiet(&mut pump.events, Duration::from_millis(50)).await,
        [],
        "no loss is reported for a move that never happened"
    );
    assert_eq!(
        wire.hand_overs
            .lock()
            .expect("lock")
            .iter()
            .map(|(to, _)| to.as_str())
            .collect::<Vec<&str>>(),
        ["phone"]
    );
}

#[tokio::test]
async fn a_pick_without_a_connection_id_is_refused() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    world.wire.set_connection_id("");

    let result = world.pick("phone").await;

    assert!(
        matches!(result, Err(ConnectError::NoConnectionId)),
        "a device the dealer never named cannot hold the sound: {result:?}"
    );
    assert_eq!(
        world.wire.log.entries(),
        Vec::<String>::new(),
        "a device with no id sends nothing"
    );
    assert!(!world.shared.presence.active());
}

#[tokio::test]
async fn a_pick_while_unplayable_is_refused() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    world.shared.presence.set_playable(false);

    let result = world.pick("phone").await;

    assert!(
        matches!(result, Err(ConnectError::NotPlayable)),
        "a device that cannot play must not take the sound even to pass it on: {result:?}"
    );
    assert_eq!(world.wire.log.entries(), Vec::<String>::new());
    assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
    assert!(!world.shared.presence.active());
}

#[tokio::test]
async fn a_pick_of_this_window_is_refused() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));

    let result = world.pick(DEVICE).await;

    assert!(matches!(result, Err(ConnectError::InvalidTarget)));
    assert_eq!(
        world.wire.log.entries(),
        Vec::<String>::new(),
        "a move to this device itself is not a hand-over"
    );
    assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn pick_after_withdraw_refuses() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));
    withdraw(&*world.wire, &world.gate, &world.shared.presence, timing())
        .await
        .expect("the withdraw completes");
    let sent = world.wire.log.entries();

    let result = world.pick("phone").await;

    assert!(matches!(result, Err(ConnectError::Withdrawn)));
    assert_eq!(
        world.wire.log.entries(),
        sent,
        "a withdrawn device sends nothing"
    );
    assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn a_target_cannot_change_the_path() {
    let world = World::new(timing());
    world.shared.latest.set(Some(report(Status::Playing)));

    let result = world.pick("phone/../../join").await;

    assert!(matches!(result, Err(ConnectError::InvalidTarget)));
    assert_eq!(
        world.wire.log.entries(),
        Vec::<String>::new(),
        "a crafted id is refused before the account is touched"
    );
    assert!(world.wire.hand_overs.lock().expect("lock").is_empty());
    assert!(!world.shared.presence.active());
}

#[tokio::test]
async fn the_pick_completes_through_the_loss_rule() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    pump.shared.latest.set(Some(report(Status::Playing)));

    pick(&*wire, &gate, &pump.shared, "phone", timing())
        .await
        .expect("the pick lands");

    assert_eq!(
        quiet(&mut pump.events, Duration::from_millis(50)).await,
        [],
        "the pick itself pauses nothing: only the cluster may"
    );

    pump.pushes
        .send(Ok(pushed(cluster_of("phone", 0))))
        .expect("push");

    assert_eq!(
        recv(&mut pump.events).await,
        ClusterEvent::Lost { epoch: 1 },
        "the target taking the sound ends the hold through the same rule"
    );
    let view = view_of_event(recv(&mut pump.events).await);
    assert_eq!(view.holder.as_deref(), Some("phone"));
    assert!(!pump.shared.presence.active());
    assert_eq!(
        quiet(&mut pump.events, Duration::from_millis(50)).await,
        [],
        "there is no second path that pauses the local transport"
    );
}
