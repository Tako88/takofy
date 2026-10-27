use std::time::{Duration, Instant};

use takofy_model::{Report, RequestId, Status, TrackEntry};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::*;
use crate::latest::Latest;
use crate::loss::Baseline;
use crate::presence::Presence;
use crate::registry::{Ack, Registry, Verdict};
use crate::testing::{FakeResponder, FakeWire, Log, until};

/// The short debounce a test can wait out.
fn timing() -> Timing {
    Timing {
        debounce: Duration::from_millis(10),
        ..Timing::default()
    }
}

const MESSAGE_ID: u32 = 42;

/// Long enough that no scheduler stall inside a test reaches it.
const GRACE: Duration = Duration::from_secs(5);

/// Longer than the fixture's debounce, so a put a wake asked for would have
/// landed by the time it has passed.
const SETTLE: Duration = Duration::from_millis(50);

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

fn report() -> Report {
    report_with("spotify:track:4uLU6hMCjMI75M1A2tKUQC", 0)
}

fn report_with(uri: &str, position_ms: u32) -> Report {
    Report {
        status: Status::Playing,
        track: Some(track(uri)),
        prev: Vec::new(),
        next: Vec::new(),
        context: None,
        queue_revision: String::new(),
        options: None,
        position_ms,
        duration_ms: Some(200_000),
    }
}

/// The ends of the publisher's event stream. A closed stream reads as
/// nobody left to tell, so a test keeps both alive to keep it running.
type Watched = (
    UnboundedSender<ClusterEvent>,
    UnboundedReceiver<ClusterEvent>,
);

/// A wire, a gate and a bundle whose device already holds the sound.
fn holding() -> (Arc<FakeWire>, Arc<Gate>, Arc<Shared>, Watched) {
    let wire = Arc::new(FakeWire::default());
    wire.set_connection_id("connection-1");
    let gate = Arc::new(Gate::default());
    let events = unbounded_channel();
    let presence = Arc::new(Presence::default());
    presence.set_playable(true);
    let latest = Arc::new(Latest::default());
    latest.set(Some(report()));
    let shared = Shared::new(
        Arc::clone(&presence),
        latest,
        Arc::new(Registry::new(timing())),
        events.0.downgrade(),
    );
    // Every reply inside the test names another device, which only the
    // grace baseline reads as a push that raced the activation rather than
    // one that ends it.
    presence.activate(Baseline::Until(Instant::now() + GRACE));
    (wire, gate, shared, events)
}

/// Starts the publisher and waits out the announce the first flip asks for,
/// so every put a test reads is one the test itself caused.
async fn publish(wire: &Arc<FakeWire>, gate: &Arc<Gate>, shared: &Arc<Shared>) -> JoinHandle<()> {
    let task = tokio::spawn(run(
        Arc::clone(wire),
        Arc::clone(gate),
        Arc::clone(shared),
        timing(),
    ));
    let before = wire.puts().len();
    until("the first announce", || wire.puts().len() > before).await;
    task
}

/// Answers one request the way the bridge answers an order.
fn answer_order(shared: &Shared, id: RequestId) {
    let log = Log::default();
    shared.registry.insert(
        id,
        Ack {
            message_id: MESSAGE_ID,
            sender: "phone".to_owned(),
        },
        Box::new(FakeResponder::new(log)),
    );
    assert!(shared.registry.answer_order(id, Verdict::Success));
}

/// Registers one request and refuses it, the way an endpoint this device
/// cannot take is answered.
fn refuse(shared: &Shared, id: RequestId) -> Log {
    let log = Log::default();
    shared.registry.insert(
        id,
        Ack {
            message_id: MESSAGE_ID,
            sender: "phone".to_owned(),
        },
        Box::new(FakeResponder::new(log.clone())),
    );
    shared.registry.refuse(id);
    log
}

/// Waits out the debounce and asserts no put was made, so a wake that should
/// not have happened is caught rather than missed.
async fn quiet(wire: &FakeWire, what: &str) {
    let puts = wire.puts().len();
    tokio::time::sleep(SETTLE).await;
    assert_eq!(wire.puts().len(), puts, "{what} asks for no put");
}

#[tokio::test]
async fn an_answered_order_forces_a_put_carrying_its_message_id_and_sender() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;
    let before = wire.puts().len();

    answer_order(&shared, RequestId(1));

    until("the answer's put", || wire.puts().len() > before).await;
    let body = wire.puts()[before].message().clone();
    assert!(body.is_active, "the answer rides in the active body");
    assert_eq!(body.last_command_message_id, MESSAGE_ID);
    assert_eq!(body.last_command_sent_by_device_id, "phone");
}

#[tokio::test]
async fn later_puts_keep_the_last_command_ids() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;

    answer_order(&shared, RequestId(2));
    until("the answer's put", || {
        wire.puts()
            .iter()
            .any(|put| put.message().last_command_message_id == MESSAGE_ID)
    })
    .await;

    // Another write of the same state, which a refresh makes here, still
    // names the command: the ack is the last order answered, not one the put
    // that carried it used up.
    let before = wire.puts().len();
    crate::watch::refresh(&*wire, &gate, &shared, timing())
        .await
        .expect("the refresh puts");

    let puts = wire.puts();
    assert_eq!(puts.len(), before + 1, "the refresh is one more put");
    for put in &puts[before..] {
        assert_eq!(put.message().last_command_message_id, MESSAGE_ID);
        assert_eq!(put.message().last_command_sent_by_device_id, "phone");
    }
}

#[tokio::test]
async fn a_refused_request_asks_for_no_put_and_names_no_command() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;

    let log = refuse(&shared, RequestId(3));

    assert_eq!(log.entries(), vec!["reply:failure"]);
    assert!(
        shared.registry.last_ack().is_none(),
        "a request this device did not carry out names no command"
    );
    quiet(&wire, "a refusal").await;
}

#[tokio::test]
async fn a_burst_of_reports_is_one_put_with_the_latest() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;
    let before = wire.puts().len();

    shared.latest.set(Some(report_with("first", 0)));
    shared.latest.set(Some(report_with("second", 0)));
    shared.latest.set(Some(report_with("third", 0)));

    until("the burst's one put", || wire.puts().len() > before).await;
    quiet(&wire, "the burst after its put").await;

    let puts = wire.puts();
    assert_eq!(puts.len(), before + 1, "a burst is one put");
    let body = puts[before].message();
    assert_eq!(
        body.device
            .as_ref()
            .and_then(|device| device.player_state.as_ref())
            .and_then(|state| state.track.as_ref())
            .map(|track| track.uri.as_str()),
        Some("third"),
        "the put carries the newest report"
    );
}

#[tokio::test]
async fn nothing_active_is_put_while_not_active() {
    let wire = Arc::new(FakeWire::default());
    wire.set_connection_id("connection-1");
    let gate = Arc::new(Gate::default());
    let events = unbounded_channel();
    let presence = Arc::new(Presence::default());
    presence.set_playable(true);
    let shared = Shared::new(
        Arc::clone(&presence),
        Arc::new(Latest::default()),
        Arc::new(Registry::new(timing())),
        events.0.downgrade(),
    );
    let _publisher = publish(&wire, &gate, &shared).await;

    shared.latest.set(Some(report_with("not-active", 0)));

    quiet(&wire, "a report while the device is not active").await;
    assert!(wire.puts().iter().all(|put| !put.message().is_active));
}

#[tokio::test]
async fn a_report_after_withdraw_sends_nothing() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;

    crate::watch::withdraw(&*wire, &gate, &shared.presence, timing())
        .await
        .expect("the withdraw completes");
    let before = wire.puts().len();

    shared.latest.set(Some(report_with("after-withdraw", 0)));

    quiet(&wire, "a report after the device went").await;
    assert_eq!(wire.puts().len(), before);
}

#[tokio::test]
async fn the_put_waits_for_a_held_gate_and_goes_after() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;
    let before = wire.puts().len();
    let held = gate.take().await;

    shared.latest.set(Some(report_with("gated", 0)));
    tokio::time::sleep(SETTLE).await;
    assert_eq!(
        wire.puts().len(),
        before,
        "the put waits behind the held gate"
    );

    drop(held);
    until("the held put to go", || wire.puts().len() > before).await;
    let puts = wire.puts();
    let body = puts.last().expect("the waiting put").message();
    assert_eq!(
        body.device
            .as_ref()
            .and_then(|device| device.player_state.as_ref())
            .and_then(|state| state.track.as_ref())
            .map(|track| track.uri.as_str()),
        Some("gated")
    );
}

#[tokio::test]
async fn the_put_carries_a_fresh_timestamp_and_the_aged_position() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;
    let now_ms = 1_700_000_000_000;
    wire.set_now(now_ms);
    let before = wire.puts().len();

    // The gate is held across the report so the body is built from a report
    // that is measurably old. The age is read in whole milliseconds, and a
    // put built in the millisecond the report was taken would carry a
    // position that never aged — true, but it says nothing about the age.
    let held = gate.take().await;
    shared.latest.set(Some(report_with("aged", 5_000)));
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(held);

    until("the aged put", || wire.puts().len() > before).await;
    let puts = wire.puts();
    let body = puts.last().expect("the aged put").message();
    let state = body
        .device
        .as_ref()
        .and_then(|device| device.player_state.as_ref())
        .expect("a player state");

    assert_eq!(state.timestamp, now_ms, "the put is stamped now");
    assert_eq!(
        body.started_playing_at,
        u64::try_from(now_ms).expect("an instant fits a u64")
    );
    assert!(
        state.position_as_of_timestamp > 5_000,
        "the position is advanced by the report's age: {}",
        state.position_as_of_timestamp
    );
}

#[tokio::test]
async fn none_while_active_releases_and_announces_the_controller() {
    let (wire, gate, shared, _watched) = holding();
    let _publisher = publish(&wire, &gate, &shared).await;

    assert!(shared.presence.active());
    shared.latest.set(None);
    assert!(shared.presence.set_playable(false));

    until("the controller after the release", || {
        wire.puts()
            .last()
            .is_some_and(|put| !put.message().is_active)
    })
    .await;
    assert!(
        wire.log.entries().contains(&"put:inactive:done".to_owned()),
        "the sound was given back before the device was announced: {:?}",
        wire.log.entries()
    );
    assert!(
        !shared.presence.active(),
        "a device with nothing to play holds no sound"
    );
    let puts = wire.puts();
    let body = puts.last().expect("the controller body").message();
    let can_play = body
        .device
        .as_ref()
        .and_then(|device| device.device_info.as_ref())
        .is_some_and(|info| info.can_play);
    assert!(!can_play, "an empty device is a controller");
}
