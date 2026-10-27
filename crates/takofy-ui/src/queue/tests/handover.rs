use super::super::*;
use super::test_support::*;

use takofy_model::TrackEntry;

fn landed(landing: &Landing) -> Update {
    Update::Remote(RemoteUpdate::Landed(landing.clone()))
}

fn lost(epoch: u64) -> Update {
    Update::Remote(RemoteUpdate::Lost { epoch })
}

fn labelled(entries: Vec<TrackEntry>) -> Update {
    Update::Remote(RemoteUpdate::Labelled(entries))
}

#[test]
fn adopting_replaces_the_rows_and_sits_on_the_landing_index_without_a_play() {
    let mut queue = started(4, 0);
    let mut transport = Transport::default();

    assert!(queue.adopt(&landing(&["a", "b", "c"], 1), &mut transport));
    assert_eq!(order(&queue), uris(&["a", "b", "c"]));
    assert_eq!(queue.index, Some(1));
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("b"));
    assert_eq!(upcoming(&queue), vec![(2, "c".to_owned())]);
}

#[test]
fn adopt_arms_the_load_guard() {
    let mut queue = started(3, 0);
    let mut transport = Transport::default();
    queue.adopt(&landing(&["a", "b", "c"], 1), &mut transport);

    // The landed track's load has not started yet, so a failure naming the
    // previous track must not advance the landed queue.
    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        None
    );
    assert_eq!(queue.index, Some(1));

    clear_guard(&mut queue, &mut transport);
    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        Some(play("c"))
    );
}

#[test]
fn adopt_sets_the_transport_to_the_landed_track() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    let playing = landing(&["a", "b"], 1);

    assert!(queue.adopt(&playing, &mut transport));
    assert_eq!(transport.state, TransportState::Loading);
    let now_playing = transport.now_playing.clone().expect("a named track");
    assert_eq!(now_playing.uri, "b");
    assert_eq!(now_playing.title, "b", "a row the label owes is its URI");
    assert_eq!(transport.position_ms, 7_000, "the phone's own position");
    assert_eq!(transport.duration_ms, None, "the length is not known yet");

    assert!(queue.adopt(
        &Landing {
            playing: false,
            ..playing
        },
        &mut transport
    ));
    assert_eq!(transport.state, TransportState::Paused);
}

#[test]
fn adopt_carries_the_modes_the_landing_names() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();

    assert!(queue.adopt(
        &Landing {
            shuffle: Some(true),
            repeat_context: Some(true),
            repeat_track: Some(true),
            ..landing(&["a", "b", "c"], 0)
        },
        &mut transport
    ));
    assert!(queue.shuffle());
    assert_eq!(
        queue.repeat(),
        Repeat::One,
        "repeat one outranks repeat all"
    );

    // The landing names the context loop alone, so the track loop it says
    // nothing about stays the one the queue is in.
    assert!(queue.adopt(
        &Landing {
            shuffle: Some(false),
            repeat_context: Some(true),
            ..landing(&["a", "b"], 0)
        },
        &mut transport
    ));
    assert_eq!(queue.repeat(), Repeat::One);
    assert!(!queue.shuffle(), "a landing that asks for off turns it off");
}

#[test]
fn a_play_with_no_override_keeps_the_current_modes() {
    let mut queue = started(3, 0);
    let mut transport = Transport::default();
    queue.set_repeat(Repeat::All);
    queue.toggle_shuffle();

    // A play that names no options is a landing whose flags are all absent.
    assert!(queue.adopt(&landing(&["a", "b", "c"], 0), &mut transport));

    assert!(queue.shuffle(), "a play that names no shuffle keeps it on");
    assert_eq!(
        queue.repeat(),
        Repeat::All,
        "a play that names no repeat keeps the mode it is in"
    );
}

#[test]
fn adopt_clears_the_selection_and_refocus() {
    let mut queue = started(5, 0);
    let mut transport = Transport::default();
    queue.press(1, Press::Replace);
    queue.press(3, Press::Toggle);
    queue.refocus_at(3);
    assert_eq!(picked(&queue), vec![1, 3]);

    queue.adopt(&landing(&["a", "b", "c", "d", "e"], 0), &mut transport);
    assert!(
        picked(&queue).is_empty(),
        "a pick cannot survive other rows"
    );
    assert_eq!(queue.refocus(), None);
}

#[test]
fn adopt_keeps_the_landed_row_current_under_a_shuffle_draw() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    let rows = ["n0", "n1", "n2", "n3", "n4", "n5"];

    assert!(queue.adopt(
        &Landing {
            shuffle: Some(true),
            ..landing(&rows, 2)
        },
        &mut transport
    ));
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("n2"));
    assert_eq!(
        order(&queue)[..3],
        uris(&["n0", "n1", "n2"]),
        "the rows before the current one keep the phone's order"
    );

    let mut drawn: Vec<String> = order(&queue)[3..].to_vec();
    assert_ne!(
        drawn,
        uris(&["n3", "n4", "n5"]),
        "the coming rows are drawn, not listed"
    );
    drawn.sort();
    assert_eq!(drawn, uris(&["n3", "n4", "n5"]), "the draw moved no row");

    queue.toggle_shuffle();
    assert_eq!(order(&queue), uris(&rows), "the snapshot was the landing's");
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("n2"));
}

#[test]
fn an_out_of_range_index_is_refused_and_changes_nothing() {
    let mut queue = started(3, 1);
    let mut transport = Transport::default();
    queue.jump(1, &mut transport);
    let before = transport.clone();
    let rows = order(&queue);

    assert!(!queue.adopt(&landing(&["a", "b"], 2), &mut transport));
    assert_eq!(order(&queue), rows, "a refused landing replaced the rows");
    assert_eq!(queue.index, Some(1));
    assert_eq!(
        transport, before,
        "a refused landing must not touch the transport"
    );
}

#[test]
fn lost_pauses_a_playing_or_loading_transport() {
    for state in [TransportState::Playing, TransportState::Loading] {
        let mut queue = started(2, 0);
        let mut transport = Transport {
            state,
            ..Transport::default()
        };
        assert_eq!(
            queue.apply(&lost(1), &mut transport),
            Some(Command::Pause),
            "{state:?} was not paused"
        );
        assert_eq!(queue.lost_epoch, 1);
    }
}

#[test]
fn lost_while_paused_sends_nothing() {
    let mut queue = started(2, 0);
    let mut transport = Transport {
        state: TransportState::Paused,
        ..Transport::default()
    };

    assert_eq!(queue.apply(&lost(2), &mut transport), None);
    assert_eq!(queue.lost_epoch, 2, "a loss while paused is still a loss");
    assert_eq!(
        queue.apply(&lost(1), &mut transport),
        None,
        "an older epoch does not lower the mark"
    );
    assert_eq!(queue.lost_epoch, 2);
}

#[test]
fn lost_then_landed_does_not_play() {
    let mut queue = started(2, 0);
    let mut transport = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };
    assert_eq!(queue.apply(&lost(1), &mut transport), Some(Command::Pause));
    let rows = order(&queue);
    let before = transport.clone();

    assert_eq!(
        queue.apply(&landed(&landing(&["a", "b"], 0)), &mut transport),
        None
    );
    assert_eq!(order(&queue), rows, "a stale landing replaced the rows");
    assert_eq!(queue.index, Some(0));
    assert_eq!(transport, before, "a stale landing moved the transport");
}

#[test]
fn a_landed_with_a_newer_epoch_after_a_loss_is_adopted() {
    let mut queue = started(2, 0);
    let mut transport = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };
    assert_eq!(queue.apply(&lost(1), &mut transport), Some(Command::Pause));

    let landing = Landing {
        epoch: 2,
        ..landing(&["a", "b", "c"], 1)
    };
    assert_eq!(queue.apply(&landed(&landing), &mut transport), None);
    assert_eq!(order(&queue), uris(&["a", "b", "c"]));
    assert_eq!(queue.index, Some(1));
    assert_eq!(transport.state, TransportState::Loading);
}

#[test]
fn label_wants_name_the_unnamed_rows_after_the_current_once() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.adopt(&landing(&["a", "b", "c", "d"], 1), &mut transport);

    assert_eq!(
        queue.label_wants(),
        Some(Command::LabelRows {
            uris: uris(&["c", "d"]),
        }),
        "only the rows after the current one owe a name"
    );
    assert_eq!(queue.label_wants(), None, "a row is asked for once");
}

#[test]
fn a_label_batch_is_capped_and_the_rest_wait_for_the_next_pass() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    let rows: Vec<String> = (0..LABEL_BATCH + 40)
        .map(|row| format!("spotify:track:{row}"))
        .collect();
    let borrowed: Vec<&str> = rows.iter().map(String::as_str).collect();
    queue.adopt(&landing(&borrowed, 0), &mut transport);

    let Some(Command::LabelRows { uris: first }) = queue.label_wants() else {
        panic!("the unnamed rows owe a request");
    };
    assert_eq!(first.len(), LABEL_BATCH, "one request names one batch");
    assert_eq!(first, rows[1..=LABEL_BATCH].to_vec());

    let Some(Command::LabelRows { uris: second }) = queue.label_wants() else {
        panic!("the rows left over owe the next request");
    };
    assert_eq!(
        second,
        rows[LABEL_BATCH + 1..].to_vec(),
        "the next pass names the rest, and nothing twice"
    );
    assert_eq!(queue.label_wants(), None);
}

#[test]
fn relabel_fills_rows_by_uri_and_leaves_the_index() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.adopt(&landing(&["a", "b", "a"], 1), &mut transport);

    let mut a = track("a");
    a.title = "Alpha".to_owned();
    let mut b = track("b");
    b.title = "Beta".to_owned();
    assert_eq!(
        queue.apply(&labelled(vec![a, b, track("gone")]), &mut transport),
        None
    );

    assert_eq!(queue.index, Some(1), "the label moved the current row");
    assert_eq!(order(&queue), uris(&["a", "b", "a"]));
    assert_eq!(queue.tracks[0].title, "Alpha", "the row by URI, not index");
    assert_eq!(queue.tracks[1].title, "Beta");
    assert_eq!(queue.tracks[2].title, "Alpha", "every row with that URI");
    assert!(queue.tracks.iter().all(|entry| entry.named));
    assert_eq!(queue.label_wants(), None, "nothing left to ask for");
}

#[test]
fn a_label_that_names_the_loaded_row_renames_the_playbar() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.adopt(&landing(&["a", "b"], 1), &mut transport);
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.title.as_str()),
        Some("b"),
        "the bar starts on the URI the landing carried"
    );

    let mut b = track("b");
    b.title = "Beta".to_owned();
    b.artists = "The Band".to_owned();
    assert_eq!(
        queue.apply(&labelled(vec![b]), &mut transport),
        None,
        "naming a row asks for nothing"
    );

    let now = transport.now_playing.expect("the loaded row is named");
    assert_eq!(now.uri, "b");
    assert_eq!(now.title, "Beta");
    assert_eq!(now.artists, "The Band");
}
