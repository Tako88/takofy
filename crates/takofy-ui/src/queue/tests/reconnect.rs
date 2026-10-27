use super::super::*;
use super::test_support::*;

use crate::message::Failure;

#[test]
fn toggle_follows_the_reported_state() {
    let cases = [
        (TransportState::Idle, Some(play("t0"))),
        (TransportState::Loading, Some(Command::Pause)),
        (TransportState::Playing, Some(Command::Pause)),
        (TransportState::Paused, Some(Command::Resume)),
        (TransportState::Unavailable, Some(play("t0"))),
        (TransportState::Stopped, Some(play("t0"))),
        (TransportState::Reconnecting, None),
        (TransportState::Failed(Failure::Other), Some(play("t0"))),
    ];
    for (state, expected) in cases {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, None, &mut transport);
        transport.state = state;
        assert_eq!(queue.toggle(&mut transport), expected, "{state:?}");
    }
}

/// A rebuild is not a load and not a skip: the guard and the streak stand
/// still, so the row that failed does not advance on the rebuild's account.
#[test]
fn reconnecting_asks_for_nothing_and_keeps_the_guard() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    let before = (queue.index, queue.awaiting_start, queue.skips);

    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Reconnecting),
        None
    );
    assert_eq!(
        (queue.index, queue.awaiting_start, queue.skips),
        before,
        "a rebuild is neither a load that started nor a skipped row"
    );
}

#[test]
fn toggle_while_reconnecting_asks_for_nothing() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    transport.state = TransportState::Reconnecting;

    assert_eq!(queue.toggle(&mut transport), None);
    assert_eq!(queue.index, Some(0), "the row stays current");
}

#[test]
fn an_unavailable_after_reconnecting_still_advances() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    clear_guard(&mut queue, &mut transport);
    feed(&mut queue, &mut transport, TransportUpdate::Reconnecting);

    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        Some(play("t1"))
    );
    assert_eq!(queue.skips, 1, "a row that would not play still counts");
}

#[test]
fn a_failed_after_reconnecting_only_clears_the_guard() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    feed(&mut queue, &mut transport, TransportUpdate::Reconnecting);
    assert!(
        queue.awaiting_start,
        "a rebuild says nothing about the load in flight"
    );

    assert_eq!(
        feed(
            &mut queue,
            &mut transport,
            TransportUpdate::Failed(Failure::Reconnect)
        ),
        None
    );
    assert!(!queue.awaiting_start);
    assert_eq!(queue.index, Some(0));
    assert_eq!(queue.skips, 0);
}
