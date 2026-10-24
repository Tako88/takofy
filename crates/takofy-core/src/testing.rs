//! Fakes shared by the modules that speak to the wire or answer requests.
//!
//! Every fake writes to one ordered log, so a test can assert what happened in
//! what order instead of sleeping and hoping an absence means something.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use librespot::core::dealer::manager::BoxedStream;
use librespot::protocol::connect::{Capabilities, Cluster, ClusterUpdate, DeviceInfo};
use librespot::protocol::devices::DeviceType;
use protobuf::{EnumOrUnknown, Message as _, MessageField};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::connect::{Announcement, ClusterEvent, ConnectError};
use crate::presence::Presence;
use crate::publish;
use crate::registry::{Registry, Responder, Verdict};
use crate::watch::{Gate, Shared, Timing, Wire, pump};

pub(crate) const DEVICE: &str = "device-1";
pub(crate) const CLIENT: &str = "client-1";
pub(crate) const SESSION: &str = "session-1";

/// What every fake did, in the order it happened.
#[derive(Debug, Clone, Default)]
pub(crate) struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    pub(crate) fn push(&self, entry: impl Into<String>) {
        self.0.lock().expect("lock").push(entry.into());
    }

    pub(crate) fn entries(&self) -> Vec<String> {
        self.0.lock().expect("lock").clone()
    }
}

/// What one put answers.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Reply {
    #[default]
    Cluster,
    Garbage,
    /// Fails as a put with no connection id does, before anything is sent.
    Fail,
    /// Fails as a put that had an id and did not land.
    Refused,
    /// Waits for the test's release before answering.
    Held,
}

pub(crate) fn device_info(name: &str) -> DeviceInfo {
    DeviceInfo {
        can_play: true,
        name: name.to_owned(),
        device_type: EnumOrUnknown::new(DeviceType::SMARTPHONE),
        capabilities: MessageField::some(Capabilities {
            can_be_player: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(crate) fn cluster() -> Cluster {
    cluster_of("phone", 0)
}

/// A cluster naming `holder` as the device with the sound.
pub(crate) fn cluster_of(holder: &str, changed_ms: i64) -> Cluster {
    let mut device = HashMap::new();
    device.insert("phone".to_owned(), device_info("Phone"));
    Cluster {
        device,
        active_device_id: holder.to_owned(),
        changed_timestamp_ms: changed_ms,
        ..Default::default()
    }
}

/// A pushed update carrying one cluster.
pub(crate) fn pushed(cluster: Cluster) -> ClusterUpdate {
    ClusterUpdate {
        cluster: MessageField::some(cluster),
        ..Default::default()
    }
}

/// The word the log uses for the body a put carries.
fn kind_of(announcement: &Announcement) -> &'static str {
    let message = announcement.message();
    if message.is_active {
        return "active";
    }
    let can_play = message
        .device
        .as_ref()
        .and_then(|device| device.device_info.as_ref())
        .is_some_and(|info| info.can_play);
    if can_play { "ready" } else { "controller" }
}

#[derive(Default)]
pub(crate) struct FakeWire {
    pub(crate) log: Log,
    connection_id: Mutex<String>,
    puts: Mutex<Vec<Announcement>>,
    commands: Mutex<Vec<(String, String, bool)>>,
    pub(crate) deletes: AtomicUsize,
    pub(crate) closes: AtomicUsize,
    pub(crate) exit_calls: Mutex<Vec<&'static str>>,
    reply: Mutex<Reply>,
    queued: Mutex<Vec<Reply>>,
    /// The cluster a put answers with, when a test wants one that is not the
    /// default: naming this device, or stamped.
    answer_cluster: Mutex<Option<Cluster>>,
    started: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    pub(crate) delete_hangs: AtomicBool,
    inactive_hangs: AtomicBool,
    inactive_fails: AtomicBool,
    now: Mutex<i64>,
    /// Every hand-over the wire was asked to make, in order.
    #[allow(dead_code)] // Nothing hands the sound over yet.
    pub(crate) hand_overs: Mutex<Vec<(String, String)>>,
}

impl FakeWire {
    pub(crate) fn reply(&self, reply: Reply) {
        *self.reply.lock().expect("lock") = reply;
    }

    /// Lines up one reply per put, so a test can fail one and pass the next.
    pub(crate) fn replies(&self, replies: impl IntoIterator<Item = Reply>) {
        *self.queued.lock().expect("lock") = replies.into_iter().collect();
    }

    /// The reply the next put gets; the queue runs out before `reply` is read.
    fn next_reply(&self) -> Reply {
        let mut queued = self.queued.lock().expect("lock");
        if queued.is_empty() {
            *self.reply.lock().expect("lock")
        } else {
            queued.remove(0)
        }
    }

    /// Holds the next put: `started` fires when it is in flight, and the
    /// put answers only once the returned sender fires.
    pub(crate) fn hold(&self) -> (oneshot::Sender<()>, oneshot::Receiver<()>) {
        self.reply(Reply::Held);
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        *self.started.lock().expect("lock") = Some(started_tx);
        *self.release.lock().expect("lock") = Some(release_rx);
        (release_tx, started_rx)
    }

    pub(crate) fn puts(&self) -> Vec<Announcement> {
        self.puts.lock().expect("lock").clone()
    }

    /// The cluster the next put answers with.
    pub(crate) fn answer_with(&self, cluster: Cluster) {
        *self.answer_cluster.lock().expect("lock") = Some(cluster);
    }

    /// The instant the fake clock reads, which a landing is computed at.
    pub(crate) fn set_now(&self, now_ms: i64) {
        *self.now.lock().expect("lock") = now_ms;
    }

    /// Makes the inactive put hang, so only its budget can end it.
    pub(crate) fn hang_inactive(&self) {
        self.inactive_hangs.store(true, Ordering::SeqCst);
    }

    /// Makes the inactive put fail.
    pub(crate) fn refuse_inactive(&self) {
        self.inactive_fails.store(true, Ordering::SeqCst);
    }

    pub(crate) fn commands(&self) -> Vec<(String, String, bool)> {
        self.commands.lock().expect("lock").clone()
    }
}

impl Wire for FakeWire {
    fn device_id(&self) -> String {
        DEVICE.to_owned()
    }

    fn client_id(&self) -> String {
        CLIENT.to_owned()
    }

    fn connection_id(&self) -> String {
        self.connection_id.lock().expect("lock").clone()
    }

    fn set_connection_id(&self, id: &str) {
        id.clone_into(&mut self.connection_id.lock().expect("lock"));
    }

    fn session_id(&self) -> String {
        SESSION.to_owned()
    }

    fn now_ms(&self) -> i64 {
        *self.now.lock().expect("lock")
    }

    async fn hand_over(&self, from: &str, to: &str) -> Result<(), ConnectError> {
        self.hand_overs
            .lock()
            .expect("lock")
            .push((from.to_owned(), to.to_owned()));
        Ok(())
    }

    async fn put(&self, announcement: &Announcement) -> Result<Vec<u8>, ConnectError> {
        let kind = kind_of(announcement);
        let cluster = self
            .answer_cluster
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(cluster);
        self.puts.lock().expect("lock").push(announcement.clone());
        self.log.push(format!("put:{kind}:start"));
        if let Some(started) = self.started.lock().expect("lock").take() {
            let _ = started.send(());
        }
        let held = *self.reply.lock().expect("lock") == Reply::Held;
        let release = self.release.lock().expect("lock").take();
        if held && let Some(release) = release {
            let _ = release.await;
        }
        let answer = match self.next_reply() {
            Reply::Cluster | Reply::Held => Ok(cluster.write_to_bytes().expect("bytes")),
            // A length-delimited field shorter than it claims is not a
            // cluster, whatever the parser is willing to skip.
            Reply::Garbage => Ok(vec![0x0a, 0x05, 0x00]),
            // Any failure will do: the pump reports the same either way.
            Reply::Fail => Err(ConnectError::NoConnectionId),
            Reply::Refused => Err(ConnectError::PutTimedOut),
        };
        self.log.push(format!("put:{kind}:done"));
        answer
    }

    async fn put_inactive(&self) -> Result<(), ConnectError> {
        self.log.push("put:inactive:start");
        self.exit_calls.lock().expect("lock").push("inactive");
        if self.inactive_hangs.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if self.inactive_fails.load(Ordering::SeqCst) {
            return Err(ConnectError::PutTimedOut);
        }
        self.log.push("put:inactive:done");
        Ok(())
    }

    async fn delete(&self) -> Result<(), ConnectError> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.exit_calls.lock().expect("lock").push("delete");
        if self.delete_hangs.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Ok(())
    }

    async fn command(&self, target: &str, body: &str, once: bool) -> Result<(), ConnectError> {
        self.commands
            .lock()
            .expect("lock")
            .push((target.to_owned(), body.to_owned(), once));
        Ok(())
    }

    async fn close(&self) {
        self.closes.fetch_add(1, Ordering::SeqCst);
        self.exit_calls.lock().expect("lock").push("close");
    }
}

/// One item of a dealer stream, as the fake feeds it.
pub(crate) type Node<T> = Result<T, librespot::core::Error>;

/// The connection id stream's sending end.
pub(crate) type IdFeed = UnboundedSender<Node<String>>;

/// The cluster push stream's sending end.
pub(crate) type PushFeed = UnboundedSender<Node<ClusterUpdate>>;

/// A stream the test feeds, so an item can arrive after the loop started.
pub(crate) fn fed<T: Send + 'static>(rx: UnboundedReceiver<T>) -> BoxedStream<T> {
    Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }))
}

pub(crate) fn empty<T: Send + 'static>() -> BoxedStream<T> {
    Box::pin(futures_util::stream::empty())
}

/// Records what the registry told the dealer, in the shared log.
pub(crate) struct FakeResponder {
    log: Log,
    answered: bool,
}

impl FakeResponder {
    pub(crate) fn new(log: Log) -> Self {
        Self {
            log,
            answered: false,
        }
    }
}

impl Responder for FakeResponder {
    fn answer(&mut self, verdict: Verdict) {
        self.answered = true;
        self.log.push(match verdict {
            Verdict::Success => "reply:success",
            Verdict::Failure => "reply:failure",
        });
    }
}

impl Drop for FakeResponder {
    fn drop(&mut self) {
        if !self.answered {
            self.log.push("dropped");
        }
    }
}

/// The next event the pump sent, failing the test rather than hanging on one
/// that never comes.
pub(crate) async fn recv(rx: &mut UnboundedReceiver<ClusterEvent>) -> ClusterEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("the pump must answer")
        .expect("the pump must still be alive")
}

/// The view one event carries.
pub(crate) fn view_of_event(event: ClusterEvent) -> crate::connect::ClusterView {
    match event {
        ClusterEvent::View(view) => *view,
        other => panic!("expected a view, got {other:?}"),
    }
}

/// Waits for a held put to reach the wire, so a missing one fails the test
/// instead of hanging it.
pub(crate) async fn in_flight(started: oneshot::Receiver<()>) {
    tokio::time::timeout(std::time::Duration::from_secs(2), started)
        .await
        .expect("the put must reach the wire")
        .expect("the wire must say the put started");
}

/// Waits until `check` holds, so no test has to guess at a duration.
pub(crate) async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let waited = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !check() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "timed out waiting for {what}");
}

/// Collects the events the pump emits until it goes quiet for `wait`.
pub(crate) async fn quiet(
    rx: &mut UnboundedReceiver<ClusterEvent>,
    wait: std::time::Duration,
) -> Vec<ClusterEvent> {
    let mut events = Vec::new();
    while let Ok(event) = tokio::time::timeout(wait, rx.recv()).await {
        events.push(event.expect("the pump must still be alive"));
    }
    events
}

/// A bundle with no pump behind it, for a call the test makes directly.
pub(crate) fn bundle(timing: Timing) -> Arc<Shared> {
    Shared::detached(
        Arc::new(Presence::default()),
        Arc::new(Registry::new(timing)),
    )
}

/// A running pump and the ends a test needs to drive and read it.
pub(crate) struct Pump {
    pub(crate) ids: IdFeed,
    pub(crate) pushes: PushFeed,
    pub(crate) events: UnboundedReceiver<ClusterEvent>,
    pub(crate) shared: Arc<Shared>,
    pub(crate) task: JoinHandle<()>,
}

/// Starts a pump whose two streams the test feeds.
///
/// The pump task holds the events channel's only strong sender, so the stream
/// closes the moment the task ends.
pub(crate) fn start_pump(wire: Arc<FakeWire>, gate: Arc<Gate>, timing: Timing) -> Pump {
    let (ids, id_rx) = unbounded_channel();
    let (pushes, push_rx) = unbounded_channel();
    let (events, event_rx) = unbounded_channel();
    let shared = Shared::new(
        Arc::new(Presence::default()),
        Arc::new(Registry::new(timing)),
        events.downgrade(),
    );
    let task = tokio::spawn(pump(
        wire,
        gate,
        fed(id_rx),
        fed(push_rx),
        Arc::clone(&shared),
        events,
        timing,
    ));
    Pump {
        ids,
        pushes,
        events: event_rx,
        shared,
        task,
    }
}

/// Runs the publisher beside a pump, so a flip reaches the wire.
pub(crate) fn start_publish(
    wire: Arc<FakeWire>,
    gate: Arc<Gate>,
    pump: &Pump,
    timing: Timing,
) -> JoinHandle<()> {
    tokio::spawn(publish::run(wire, gate, Arc::clone(&pump.shared), timing))
}
