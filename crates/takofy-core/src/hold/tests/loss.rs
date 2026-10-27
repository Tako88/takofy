use std::time::Duration;

use super::super::*;
use super::test_support::*;

use crate::testing::{CLIENT, DEVICE, SESSION, cluster_of, pushed, quiet, recv, view_of_event};

#[tokio::test]
async fn a_push_naming_another_device_while_active_is_a_loss_before_the_view() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    let id = RequestId(1);
    pending(&pump.shared, &wire, id);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let landing = accept(
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

    pump.pushes
        .send(Ok(pushed(cluster_of("phone", REPLY_MS + 1))))
        .expect("push");

    assert_eq!(
        recv(&mut pump.events).await,
        ClusterEvent::Lost {
            epoch: landing.epoch
        },
        "the loss is reported before the view that names another device"
    );
    let view = view_of_event(recv(&mut pump.events).await);
    assert_eq!(view.holder.as_deref(), Some("phone"));
    assert!(!pump.shared.presence.active());
}

#[tokio::test]
async fn a_stale_push_right_after_activation_is_not_a_loss() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    let id = RequestId(1);
    pending(&pump.shared, &wire, id);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));
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

    pump.pushes
        .send(Ok(pushed(cluster_of("phone", REPLY_MS - 1))))
        .expect("push");

    let view = view_of_event(recv(&mut pump.events).await);
    assert_eq!(view.holder.as_deref(), Some("phone"));
    assert!(
        pump.shared.presence.active(),
        "a push stamped before the reply predates the hold"
    );
    assert_eq!(
        quiet(&mut pump.events, Duration::from_millis(50)).await,
        [],
        "nothing follows a stale push"
    );
}

#[tokio::test]
async fn a_push_naming_us_is_not_a_loss() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    let id = RequestId(1);
    pending(&pump.shared, &wire, id);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));
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

    pump.pushes
        .send(Ok(pushed(cluster_of(DEVICE, REPLY_MS + 1))))
        .expect("push");

    let view = view_of_event(recv(&mut pump.events).await);
    assert!(view.held_here);
    assert!(pump.shared.presence.active(), "a push naming us holds");
    assert_eq!(quiet(&mut pump.events, Duration::from_millis(50)).await, []);
}

#[tokio::test]
async fn a_second_connection_id_while_active_is_a_loss_and_announces_ready() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    let id = RequestId(1);
    pending(&pump.shared, &wire, id);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));
    let landing = accept(
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

    pump.ids.send(Ok("connection-2".to_owned())).expect("id");

    assert_eq!(
        recv(&mut pump.events).await,
        ClusterEvent::Lost {
            epoch: landing.epoch
        },
        "the hold ends with the registration it was taken under"
    );
    let _ = view_of_event(recv(&mut pump.events).await);
    assert_eq!(
        wire.puts().last(),
        Some(&Announcement::ready(DEVICE, CLIENT, SESSION)),
        "the new registration announces a device waiting to be picked"
    );
    assert!(!pump.shared.presence.active());
}

#[tokio::test]
async fn a_second_connection_id_clears_active_before_any_publisher_put_can_take_the_gate() {
    let (wire, gate, mut pump) = a_held_pump(timing()).await;
    let id = RequestId(1);
    pending(&pump.shared, &wire, id);
    wire.answer_with(cluster_of(DEVICE, REPLY_MS));
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

    let held = gate.take().await;
    pump.ids.send(Ok("connection-2".to_owned())).expect("id");

    assert!(
        matches!(recv(&mut pump.events).await, ClusterEvent::Lost { .. }),
        "the loss reaches the window before the gate frees"
    );
    assert!(
        !pump.shared.presence.active(),
        "the hold is gone before any put can read the stage"
    );

    drop(held);
    let _ = view_of_event(recv(&mut pump.events).await);
    assert_eq!(
        wire.puts().last(),
        Some(&Announcement::ready(DEVICE, CLIENT, SESSION)),
        "the announce after the freed gate is a waiting device"
    );
}
