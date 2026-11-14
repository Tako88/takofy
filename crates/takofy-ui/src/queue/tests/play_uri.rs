use super::super::*;
use super::test_support::*;

use crate::message::Failure;

/// The uri these tests link.
const X: &str = "spotify:track:x";

fn at(state: TransportState) -> Transport {
    Transport {
        state,
        ..Transport::default()
    }
}

fn current_uri(queue: &Queue) -> Option<String> {
    queue.current().map(|entry| entry.uri.clone())
}

#[test]
fn a_named_current_row_asks_nothing() {
    let mut queue = started(3, 0);

    assert_eq!(queue.label_wants(), None, "every row is already named");
}

#[test]
fn an_unnamed_current_row_is_asked_once() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.play_uri_next(X, &mut transport, true);

    assert_eq!(
        queue.label_wants(),
        Some(Command::LabelRows { uris: uris(&[X]) }),
        "the row the link started owes a name"
    );
    assert_eq!(queue.label_wants(), None, "a row is asked for once");
}

#[test]
fn a_started_link_is_named_in_the_bar_when_the_label_lands() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(queue.play_uri_next(X, &mut transport, true), Some(play(X)));
    assert_eq!(
        queue.label_wants(),
        Some(Command::LabelRows { uris: uris(&[X]) })
    );

    let mut named = track(X);
    named.title = "Ex".to_owned();
    assert_eq!(
        queue.apply(
            &Update::Remote(RemoteUpdate::Labelled(vec![named])),
            &mut transport
        ),
        None
    );

    let now = transport.now_playing.clone().expect("the row is named");
    assert_eq!(now.title, "Ex", "the bar names the row it started");
    assert!(
        queue.current().is_some_and(|entry| entry.named),
        "the row itself is named"
    );
}

#[test]
fn a_repeated_link_is_asked_again() {
    let mut queue = started(2, 0);
    let mut transport = Transport::default();
    assert!(queue.add_next_uri(X).is_some());
    assert!(queue.label_wants().is_some(), "the row is asked once");

    let mut named = track(X);
    named.title = "Ex".to_owned();
    assert_eq!(
        queue.apply(
            &Update::Remote(RemoteUpdate::Labelled(vec![named])),
            &mut transport
        ),
        None
    );

    assert_eq!(queue.play_uri_next(X, &mut transport, true), Some(play(X)));
    assert_eq!(
        queue.label_wants(),
        Some(Command::LabelRows { uris: uris(&[X]) }),
        "a uri asked for before is asked for again"
    );
}

#[test]
fn a_link_row_waits_behind_what_is_playing() {
    let mut queue = started(3, 0);
    let mut transport = at(TransportState::Playing);
    let before = transport.clone();

    assert_eq!(queue.play_uri_next(X, &mut transport, true), None);
    assert_eq!(order(&queue), uris(&["t0", X, "t1", "t2"]));
    assert_eq!(queue.queued, 1, "the link joins the counted block");
    assert_eq!(current_uri(&queue).as_deref(), Some("t0"));
    assert_eq!(transport, before, "the playing track is untouched");
}

#[test]
fn loading_paused_and_reconnecting_count_as_playing() {
    for state in [
        TransportState::Loading,
        TransportState::Paused,
        TransportState::Reconnecting,
    ] {
        let mut queue = started(3, 0);
        let mut transport = at(state);

        assert_eq!(
            queue.play_uri_next(X, &mut transport, true),
            None,
            "{state:?} started the link"
        );
        assert_eq!(
            current_uri(&queue).as_deref(),
            Some("t0"),
            "{state:?} moved the queue"
        );
        assert!(
            order(&queue).contains(&X.to_owned()),
            "{state:?} dropped the row"
        );
    }
}

#[test]
fn a_link_starts_when_the_transport_is_at_rest() {
    for state in [
        TransportState::Idle,
        TransportState::Stopped,
        TransportState::Unavailable,
        TransportState::Failed(Failure::Search),
    ] {
        let mut queue = started(3, 2);
        let mut transport = at(state);

        assert_eq!(
            queue.play_uri_next(X, &mut transport, true),
            Some(play(X)),
            "{state:?} did not start the link"
        );
        assert_eq!(
            current_uri(&queue).as_deref(),
            Some(X),
            "{state:?} left the old row current"
        );
        assert_eq!(queue.queued, 0, "{state:?} left a counted block behind");
        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", X]));
    }
}

#[test]
fn a_link_never_starts_without_permission() {
    let mut queue = started(3, 2);
    let mut transport = at(TransportState::Idle);
    let before = transport.clone();

    assert_eq!(queue.play_uri_next(X, &mut transport, false), None);
    assert_eq!(
        current_uri(&queue).as_deref(),
        Some("t2"),
        "the last old row is still current"
    );
    assert_eq!(
        transport, before,
        "a start without permission moved the bar"
    );
    assert_eq!(
        order(&queue),
        uris(&["t0", "t1", "t2", X]),
        "the row is still queued"
    );

    let mut queue = armed(2);
    let mut transport = at(TransportState::Idle);

    assert_eq!(queue.play_uri_next(X, &mut transport, false), None);
    assert!(queue.armed(), "the queue is still armed");
    assert_eq!(order(&queue), uris(&[X, "t0", "t1"]));
}

#[test]
fn a_link_starts_an_armed_queue_and_disarms_it() {
    let mut queue = armed(2);
    let mut transport = at(TransportState::Idle);

    assert_eq!(queue.play_uri_next(X, &mut transport, true), Some(play(X)));
    assert!(!queue.armed(), "the link started the queue");
    assert_eq!(order(&queue), uris(&[X, "t0", "t1"]));
    assert_eq!(current_uri(&queue).as_deref(), Some(X));
}

#[test]
fn a_link_starts_an_empty_queue() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();

    assert_eq!(queue.play_uri_next(X, &mut transport, true), Some(play(X)));
    assert_eq!(order(&queue), uris(&[X]));

    let mut queue = Queue::default();
    let mut transport = Transport::default();

    assert_eq!(queue.play_uri_next(X, &mut transport, false), None);
    assert_eq!(
        order(&queue),
        uris(&[X]),
        "the row is held for a later start"
    );
}

#[test]
fn a_link_goes_in_front_of_the_phones_block_and_grows_it() {
    let mut queue = started(3, 0);
    let mut transport = at(TransportState::Playing);
    queue.add_next(vec![track("a")]);
    queue.add_next(vec![track("b")]);

    assert_eq!(queue.play_uri_next(X, &mut transport, true), None);
    assert_eq!(order(&queue), uris(&["t0", X, "a", "b", "t1", "t2"]));
    assert_eq!(queue.queued, 3, "the phone's block grew by the link");
}

#[test]
fn a_non_playable_uri_is_refused_and_changes_nothing() {
    for uri in ["spotify:album:a", "spotify:playlist:p", "not a uri", ""] {
        let mut queue = started(2, 0);
        let mut transport = at(TransportState::Playing);
        let before = transport.clone();

        assert_eq!(
            queue.play_uri_next(uri, &mut transport, true),
            None,
            "{uri:?} was accepted"
        );
        assert_eq!(order(&queue), uris(&["t0", "t1"]));
        assert_eq!(queue.queued, 0);
        assert_eq!(transport, before);

        let mut queue = Queue::default();
        let mut transport = Transport::default();

        assert_eq!(queue.play_uri_next(uri, &mut transport, true), None);
        assert!(order(&queue).is_empty(), "{uri:?} landed on an idle queue");
    }
}

#[test]
fn a_link_that_starts_inside_a_queue_with_a_block_keeps_the_block_counted() {
    let mut queue = started(3, 2);
    let mut transport = at(TransportState::Stopped);
    queue.add_next(vec![track("a")]);
    queue.add_next(vec![track("b")]);

    assert_eq!(queue.play_uri_next(X, &mut transport, true), Some(play(X)));
    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", X, "a", "b"]));
    assert_eq!(queue.queued, 2, "the phone's rows still owe their order");
}

#[test]
fn a_link_with_shuffle_on_starts_the_linked_row_first() {
    let mut queue = armed(3);
    queue.toggle_shuffle();
    let mut transport = at(TransportState::Idle);

    assert_eq!(queue.play_uri_next(X, &mut transport, true), Some(play(X)));
    assert_eq!(current_uri(&queue).as_deref(), Some(X));

    let mut rest = order(&queue);
    rest.remove(0);
    rest.sort();
    assert_eq!(rest, uris(&["t0", "t1", "t2"]), "the draw moved no row");
}
