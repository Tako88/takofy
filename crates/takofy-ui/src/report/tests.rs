use std::time::Duration;

use takofy_model::{Landing, Options, Order, TrackEntry};

use super::*;
use crate::message::Failure;
use crate::remote::react;
use crate::repeat::Repeat;

fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn tracks(count: usize) -> Vec<TrackEntry> {
    (0..count).map(|i| track(&format!("t{i}"))).collect()
}

fn started(count: usize, index: usize) -> (Queue, Transport) {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks(count), index, None, &mut transport);
    transport.duration_ms = Some(200_000);
    (queue, transport)
}

fn uris(entries: &[TrackEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.uri.clone()).collect()
}

fn landing(context: Option<&str>) -> Landing {
    Landing {
        rows: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        index: 1,
        position_ms: 7_000,
        playing: true,
        context: context.map(str::to_owned),
        shuffle: Some(false),
        repeat_context: Some(false),
        repeat_track: Some(false),
        epoch: 1,
    }
}

/// The modes a report names, which a queue report always does.
fn options(report: &Report) -> Options {
    report.options.expect("a queue report names its modes")
}

fn send(reporter: &mut Reporter, queue: &Queue, transport: &Transport) -> Option<Report> {
    reporter
        .next(queue, transport, 0)
        .expect("the state is due")
        .into_iter()
        .next()
}

#[test]
fn no_current_entry_is_none() {
    assert!(report_of(&Queue::default(), &Transport::default(), 0).is_none());

    let mut queue = Queue::default();
    queue.append(tracks(2));
    assert!(
        report_of(&queue, &Transport::default(), 0).is_none(),
        "an armed queue has rows and nothing current"
    );
}

#[test]
fn a_failed_transport_is_none() {
    let (queue, mut transport) = started(2, 0);
    transport.state = TransportState::Failed(Failure::Other);

    assert!(report_of(&queue, &transport, 0).is_none());
}

#[test]
fn status_maps_from_the_transport_state() {
    let table = [
        (TransportState::Playing, Status::Playing),
        (TransportState::Loading, Status::Loading),
        (TransportState::Reconnecting, Status::Loading),
        (TransportState::Paused, Status::Paused),
        (TransportState::Stopped, Status::Paused),
        (TransportState::Unavailable, Status::Paused),
        (TransportState::Idle, Status::Paused),
    ];

    for (state, expected) in table {
        let (queue, mut transport) = started(2, 0);
        transport.state = state;
        assert_eq!(
            report_of(&queue, &transport, 0).map(|report| report.status),
            Some(expected),
            "{state:?}"
        );
    }
}

#[test]
fn prev_and_next_are_the_nearest_rows() {
    let (queue, transport) = started(6, 3);
    let report = report_of(&queue, &transport, 0).expect("a current entry");

    assert_eq!(uris(&report.prev), vec!["t0", "t1", "t2"]);
    assert_eq!(uris(&report.next), vec!["t4", "t5"]);
}

#[test]
fn repeat_one_is_repeat_track_and_all_is_repeat_context() {
    let (mut queue, transport) = started(3, 0);
    queue.cycle_repeat();
    assert_eq!(queue.repeat(), Repeat::All);
    let report = report_of(&queue, &transport, 0).expect("a current entry");
    assert!(options(&report).repeat_context);
    assert!(!options(&report).repeat_track);

    queue.cycle_repeat();
    assert_eq!(queue.repeat(), Repeat::One);
    let report = report_of(&queue, &transport, 0).expect("a current entry");
    assert!(options(&report).repeat_track);
    assert!(!options(&report).repeat_context);
}

#[test]
fn shuffle_is_carried() {
    let (mut queue, transport) = started(4, 0);
    queue.toggle_shuffle();

    let report = report_of(&queue, &transport, 0).expect("a current entry");
    assert!(options(&report).shuffle);
}

#[test]
fn a_play_from_a_page_remembers_the_context_it_came_from() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks(3), 1, Some("spotify:playlist:p"), &mut transport);

    assert_eq!(queue.origin(), Some("spotify:playlist:p"));
    let report = report_of(&queue, &transport, 0).expect("a local entry");
    assert_eq!(report.context.as_deref(), Some("spotify:playlist:p"));
}

#[test]
fn a_play_from_no_page_carries_no_context() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks(3), 1, None, &mut transport);

    assert_eq!(queue.origin(), None);
    let report = report_of(&queue, &transport, 0).expect("a local entry");
    assert_eq!(report.context, None);
}

#[test]
fn the_context_is_the_origin_of_an_adopted_queue_and_none_after_a_local_start() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert!(queue.adopt(&landing(Some("spotify:playlist:p")), &mut transport));

    let report = report_of(&queue, &transport, 0).expect("a landed entry");
    assert_eq!(report.context.as_deref(), Some("spotify:playlist:p"));

    queue.start(&tracks(2), 0, None, &mut transport);
    let report = report_of(&queue, &transport, 0).expect("a local entry");
    assert_eq!(report.context, None, "a local start has no context");
}

#[test]
fn an_unchanged_state_sends_nothing() {
    let (queue, transport) = started(3, 1);
    let mut reporter = Reporter::default();

    assert!(reporter.next(&queue, &transport, 0).is_some());
    assert_eq!(reporter.next(&queue, &transport, 0), None);
}

#[test]
fn a_position_tick_while_playing_sends_nothing() {
    let (queue, mut transport) = started(3, 1);
    transport.state = TransportState::Playing;
    transport.position_ms = 1_000;
    let mut reporter = Reporter::default();

    assert!(reporter.next(&queue, &transport, 1_000).is_some());
    assert_eq!(
        reporter.next(&queue, &transport, 9_000),
        None,
        "the interpolated position is not a change"
    );
}

#[test]
fn a_seek_a_pause_a_new_track_or_an_edit_of_the_upcoming_rows_sends_once() {
    let (mut queue, mut transport) = started(5, 1);
    transport.state = TransportState::Playing;
    transport.position_ms = 1_000;
    let mut reporter = Reporter::default();
    let _ = reporter.next(&queue, &transport, 1_000);

    // A seek moves the anchor the report carries.
    transport.position_ms = 30_000;
    assert!(reporter.next(&queue, &transport, 30_000).is_some());
    assert_eq!(reporter.next(&queue, &transport, 30_000), None);

    // A pause is a status change.
    transport.state = TransportState::Paused;
    assert!(reporter.next(&queue, &transport, 30_000).is_some());
    assert_eq!(reporter.next(&queue, &transport, 30_000), None);

    // A track the queue moved to is another track.
    transport.state = TransportState::Playing;
    queue.next(&mut transport);
    assert!(reporter.next(&queue, &transport, 0).is_some());
    assert_eq!(reporter.next(&queue, &transport, 0), None);

    // An edit of the upcoming rows changes what the phone draws next.
    queue.insert_at(3, vec![track("x")]);
    assert!(reporter.next(&queue, &transport, 0).is_some());
    assert_eq!(reporter.next(&queue, &transport, 0), None);
}

#[test]
fn some_then_none_sends_none_once() {
    let (queue, transport) = started(2, 0);
    let mut reporter = Reporter::default();
    assert!(send(&mut reporter, &queue, &transport).is_some());

    let empty = Queue::default();
    assert_eq!(reporter.next(&empty, &transport, 0), Some(None));
    assert_eq!(
        reporter.next(&empty, &transport, 0),
        None,
        "the clearing is sent once"
    );
}

#[test]
fn a_repeated_send_is_quiet_until_the_state_changes_again() {
    let (mut queue, transport) = started(2, 0);
    let mut reporter = Reporter::default();
    assert!(reporter.next(&queue, &transport, 0).is_some());

    std::thread::sleep(Duration::from_millis(2));
    assert_eq!(reporter.next(&queue, &transport, 0), None);

    queue.cycle_repeat();
    assert!(reporter.next(&queue, &transport, 0).is_some());
}

/// One row the phone's window carries, named the way the wire names it.
fn window(name: &str) -> String {
    format!("spotify:track:{name}")
}

#[test]
fn the_revision_is_published_back() {
    let (mut queue, transport) = started(3, 1);
    assert_eq!(
        report_of(&queue, &transport, 0)
            .expect("a current entry")
            .queue_revision,
        "",
        "a queue the phone never pushed a window to has no revision"
    );

    assert_eq!(
        queue.replace_window(&[window("p")], &[window("n")], "rev-7".to_owned()),
        Ok(())
    );

    assert_eq!(
        report_of(&queue, &transport, 0)
            .expect("a current entry")
            .queue_revision,
        "rev-7",
        "the revision the window came from is the one that travels back"
    );
}

#[test]
fn a_new_revision_alone_is_a_change() {
    let (mut queue, transport) = started(3, 1);
    let prev = [window("t0")];
    let next = [window("t2")];
    assert_eq!(
        queue.replace_window(&prev, &next, "rev-1".to_owned()),
        Ok(())
    );
    let mut reporter = Reporter::default();
    assert!(reporter.next(&queue, &transport, 0).is_some());
    assert_eq!(
        reporter.next(&queue, &transport, 0),
        None,
        "the same rows and the same revision are the same state"
    );

    // The same window again, so nothing but the revision differs.
    assert_eq!(
        queue.replace_window(&prev, &next, "rev-2".to_owned()),
        Ok(())
    );

    assert!(
        reporter.next(&queue, &transport, 0).is_some(),
        "a revision the phone has not been told back is a change of its own"
    );
}

#[test]
fn an_order_that_changes_the_queue_triggers_a_report() {
    let (mut queue, mut transport) = started(3, 0);
    let mut reporter = Reporter::default();
    assert!(reporter.next(&queue, &transport, 0).is_some());
    assert_eq!(reporter.next(&queue, &transport, 0), None);

    let reaction = react(
        &Order::AddToQueue(Some(window("x"))),
        &mut queue,
        &mut transport,
        0,
    );
    assert!(reaction.ok, "a named row is added");

    assert!(
        reporter.next(&queue, &transport, 0).is_some(),
        "the row the phone queued has to reach the account"
    );
}
