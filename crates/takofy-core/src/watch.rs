//! Watching the account's devices: the one seam that speaks the wire.
//!
//! One gate orders every announce, refresh, steer, accept and withdraw, so a
//! connection id that arrives during a withdraw cannot re-create the device, and
//! a skip's single attempt cannot overlap another request. The pump is the only
//! long-lived reader of the dealer's streams, so a lost hold is reported while a
//! put is still on the wire; a put is never unbounded, because the gate is what
//! the next writer waits on.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use http::{HeaderMap, HeaderValue, Method, header};
use librespot::core::Session;
use librespot::core::dealer::manager::BoxedStreamResult;
use librespot::core::dealer::protocol::Message;
use librespot::core::spclient::{RequestStrategy, SpClient};
use librespot::protocol::connect::{Cluster, ClusterUpdate};
use protobuf::Message as _;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, WeakUnboundedSender, unbounded_channel,
};
use tokio::sync::watch as signal;

use crate::connect::{
    Announcement, ClusterEvent, ClusterView, ConnectError, Steer, WITHDRAW_TIMEOUT, repeatable,
    steer_body, view_of, view_of_update,
};
use crate::latest::Latest;
use crate::playstate::ActiveState;
use crate::presence::{Observation, Presence, Stage};
use crate::publish;
use crate::receive::{self, Inbound};
use crate::registry::Registry;

/// The stream the dealer names its connection id on.
const CONNECTION_URI: &str = "hm://pusher/v1/connections/";

/// The stream the cluster pushes on.
const CLUSTER_URI: &str = "hm://connect-state/v1/cluster";

/// The stream the account sends its commands on.
const COMMAND_URI: &str = "hm://connect-state/v1/player/command";

/// The header the pusher names the connection id in. The put carries the same
/// id under `x-spotify-connection-id`; both spellings are the wire's.
const CONNECTION_HEADER: &str = "Spotify-Connection-Id";

/// How long the pump waits for a connection id before saying so.
pub(crate) const CONNECTION_ID_WAIT: Duration = Duration::from_secs(10);

/// The longest a skip waits for the wire to be free.
pub(crate) const GATE_WAIT: Duration = Duration::from_secs(2);

/// How long a request may wait for an answer before the phone is told it failed.
///
/// Under the phone's own retry and over the longest accept path, so a slow
/// transfer is never cut off and a stuck one is never left hanging.
pub(crate) const ANSWER_BUDGET: Duration = Duration::from_secs(10);

/// How much of an answer budget an order leaves unspent.
///
/// The window drops an order it receives this close to the budget running out:
/// once the phone has been told the order failed, carrying it out would move
/// the transport behind a phone that has already moved on.
pub const ORDER_MARGIN: Duration = Duration::from_millis(500);

/// How long a context walk may take before it gives up.
pub(crate) const RESOLVE_BUDGET: Duration = Duration::from_secs(3);

/// The bound on every state put, in place of the client's own retries.
pub(crate) const PUT_BUDGET: Duration = Duration::from_secs(3);

/// The bound on the inactive put that gives the sound back.
pub(crate) const RELEASE_BUDGET: Duration = Duration::from_secs(1);

/// The bound on the hand-over that moves the sound away.
///
/// Sized so a pick — the wait for the gate, the active put and the move itself —
/// fits inside one answer budget: the command loop awaits inline, so everything
/// queued behind a pick waits no longer than a request does.
pub(crate) const HAND_OVER_BUDGET: Duration = Duration::from_secs(5);

/// The window a loss rule falls back to when an activation reply carries no
/// usable time of its own.
pub(crate) const ACTIVATION_GRACE: Duration = Duration::from_secs(2);

/// How long a burst of flips settles before it becomes one put.
pub(crate) const STATE_DELAY: Duration = Duration::from_millis(200);

/// The slices of time the watch depends on, so a test can shorten them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    pub id_wait: Duration,
    pub withdraw: Duration,
    pub gate_wait: Duration,
    pub answer: Duration,
    pub put: Duration,
    /// The bound on a hand-over, which holds the gate like any other write.
    pub hand_over: Duration,
    pub debounce: Duration,
    // A transfer's context walk is bounded by this; nothing on the watch path
    // walks a context, so only the hand-over reads it.
    pub resolve: Duration,
    pub release: Duration,
    pub grace: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            id_wait: CONNECTION_ID_WAIT,
            withdraw: WITHDRAW_TIMEOUT,
            gate_wait: GATE_WAIT,
            answer: ANSWER_BUDGET,
            resolve: RESOLVE_BUDGET,
            put: PUT_BUDGET,
            hand_over: HAND_OVER_BUDGET,
            release: RELEASE_BUDGET,
            grace: ACTIVATION_GRACE,
            debounce: STATE_DELAY,
        }
    }
}

/// Orders the calls that write the account's device state.
///
/// `withdrawn` is set before the gate is taken, so a withdraw that never gets in
/// still stops every announce that comes after it; the signal is what cuts a put
/// that is already waiting on the wire.
pub(crate) struct Gate {
    lock: TokioMutex<()>,
    withdrawn: AtomicBool,
    signal: signal::Sender<bool>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            lock: TokioMutex::new(()),
            withdrawn: AtomicBool::new(false),
            signal: signal::channel(false).0,
        }
    }
}

impl Gate {
    pub(crate) async fn take(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.lock.lock().await
    }

    fn mark_withdrawn(&self) {
        self.withdrawn.store(true, Ordering::SeqCst);
        // `send_replace` rather than `send`: the flag must be readable by the
        // next subscriber even when nobody is listening right now.
        self.signal.send_replace(true);
    }

    pub(crate) fn withdrawn(&self) -> bool {
        self.withdrawn.load(Ordering::SeqCst)
    }

    /// The flag, for a put that must stop the moment the device goes.
    fn withdrawn_signal(&self) -> signal::Receiver<bool> {
        self.signal.subscribe()
    }
}

/// The wire the watch needs, behind a seam a test can answer for.
pub(crate) trait Wire: Send + Sync + 'static {
    fn device_id(&self) -> String;
    fn client_id(&self) -> String;
    /// Empty until the dealer names one.
    fn connection_id(&self) -> String;
    fn set_connection_id(&self, id: &str);
    /// The session the account knows this device by, carried by a body that can
    /// play.
    fn session_id(&self) -> String;
    /// The wall clock, in unix milliseconds, which a landing is computed at.
    fn now_ms(&self) -> i64;
    fn put(
        &self,
        announcement: &Announcement,
    ) -> impl Future<Output = Result<Vec<u8>, ConnectError>> + Send;
    /// Tells the account this device no longer holds the sound.
    fn put_inactive(&self) -> impl Future<Output = Result<(), ConnectError>> + Send;
    /// Moves the sound to `to`, away from `from`.
    fn hand_over(
        &self,
        from: &str,
        to: &str,
    ) -> impl Future<Output = Result<(), ConnectError>> + Send;
    fn delete(&self) -> impl Future<Output = Result<(), ConnectError>> + Send;
    /// Sends one command body to `target`, in one attempt when `once`.
    fn command(
        &self,
        target: &str,
        body: &str,
        once: bool,
    ) -> impl Future<Output = Result<(), ConnectError>> + Send;
    fn close(&self) -> impl Future<Output = ()> + Send;
}

/// One segment of a device path, refusing an id that would add or change one.
fn segment(value: &str) -> Result<&str, ConnectError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.chars().any(|character| {
            matches!(character, '/' | '\\' | '?' | '#' | '%')
                || character.is_control()
                || character.is_whitespace()
        })
    {
        return Err(ConnectError::InvalidTarget);
    }
    Ok(value)
}

/// Builds the command path, refusing an id that would add or change a segment.
fn command_endpoint(own_id: &str, target: &str) -> Result<String, ConnectError> {
    Ok(format!(
        "/connect-state/v1/player/command/from/{}/to/{}",
        segment(own_id)?,
        segment(target)?
    ))
}

/// Refuses the ids of a hand-over that could add or change a path segment, so
/// no device id can widen the endpoint the client builds from them.
///
/// A move to this device's own id is refused too: the server reads it as
/// something other than "the sound is already here".
pub(crate) fn hand_over_path(from: &str, to: &str) -> Result<(), ConnectError> {
    segment(from)?;
    segment(to)?;
    if from == to {
        return Err(ConnectError::InvalidTarget);
    }
    Ok(())
}

/// The production wire: the only code that names the device endpoints.
pub(crate) struct SessionWire(Session);

impl SessionWire {
    pub(crate) fn new(session: &Session) -> Self {
        Self(session.clone())
    }
}

impl Wire for SessionWire {
    fn device_id(&self) -> String {
        self.0.device_id().to_owned()
    }

    fn client_id(&self) -> String {
        self.0.client_id()
    }

    fn connection_id(&self) -> String {
        self.0.connection_id()
    }

    fn set_connection_id(&self, id: &str) {
        self.0.set_connection_id(id);
    }

    fn session_id(&self) -> String {
        self.0.session_id()
    }

    fn now_ms(&self) -> i64 {
        receive::now_ms()
    }

    async fn put(&self, announcement: &Announcement) -> Result<Vec<u8>, ConnectError> {
        let client = self.0.spclient();
        Ok(client
            .put_connect_state_request(announcement.message())
            .await?
            .to_vec())
    }

    async fn put_inactive(&self) -> Result<(), ConnectError> {
        self.0.spclient().put_connect_state_inactive(false).await?;
        Ok(())
    }

    async fn hand_over(&self, from: &str, to: &str) -> Result<(), ConnectError> {
        // The path a hand-over will take is checked before anything is sent,
        // so no device id can widen it into another endpoint.
        hand_over_path(from, to)?;
        self.0.spclient().transfer(from, to, None).await?;
        Ok(())
    }

    async fn delete(&self) -> Result<(), ConnectError> {
        self.0.spclient().delete_connect_state_request().await?;
        Ok(())
    }

    async fn command(&self, target: &str, body: &str, once: bool) -> Result<(), ConnectError> {
        let client = self.0.spclient();
        // The restore runs inside this call, so it is already done before the
        // gate frees: the next caller never reads a one-attempt budget.
        let _restore = once.then(|| {
            client.set_strategy(RequestStrategy::TryTimes(1));
            Restore(client)
        });
        let endpoint = command_endpoint(&self.device_id(), target)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        client
            .request(
                &Method::POST,
                &endpoint,
                Some(headers),
                Some(body.as_bytes()),
            )
            .await?;
        Ok(())
    }

    async fn close(&self) {
        self.0.dealer().close().await;
    }
}

/// Puts the shared client's retry budget back however the request ends.
struct Restore<'a>(&'a SpClient);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.0.set_strategy(RequestStrategy::TryTimes(10));
    }
}

/// What every announcement reads: the stage this device is in, the newest
/// report, the requests in flight and the stream the window reads.
///
/// The active body is built from the report the window last published, aged by
/// the time since it was taken. A landing's report is published the same way,
/// so there is one source of truth for what this device is playing.
///
/// The events sender is weak because the pump holds the only strong one: when
/// its streams end the stream closes, and the window reads that as a dead watch.
pub(crate) struct Shared {
    pub(crate) presence: Arc<Presence>,
    pub(crate) latest: Arc<Latest>,
    pub(crate) registry: Arc<Registry>,
    pub(crate) events: WeakUnboundedSender<ClusterEvent>,
}

impl Shared {
    pub(crate) fn new(
        presence: Arc<Presence>,
        latest: Arc<Latest>,
        registry: Arc<Registry>,
        events: WeakUnboundedSender<ClusterEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            presence,
            latest,
            registry,
            events,
        })
    }

    /// A bundle with no window behind it, for a call made outside the pump.
    pub(crate) fn detached(
        presence: Arc<Presence>,
        latest: Arc<Latest>,
        registry: Arc<Registry>,
    ) -> Arc<Self> {
        let (events, _) = unbounded_channel();
        Self::new(presence, latest, registry, events.downgrade())
    }

    /// Sends to the window, or says that nothing is reading any more.
    pub(crate) fn emit(&self, event: ClusterEvent) -> bool {
        match self.events.upgrade() {
            Some(events) => events.send(event).is_ok(),
            None => false,
        }
    }
}

/// The one place an announcement is built, so a put always says what the stage
/// says.
///
/// The active body is built from the newest report and its age. A report whose
/// track cannot be published falls back to the ready body, so the account is
/// never told this device holds a track it cannot name.
fn body<W: Wire>(wire: &W, shared: &Shared) -> Announcement {
    match shared.presence.stage(shared.latest.has_report()) {
        Stage::Controller => Announcement::controller(&wire.device_id(), &wire.client_id()),
        Stage::Ready => {
            Announcement::ready(&wire.device_id(), &wire.client_id(), &wire.session_id())
        }
        Stage::Active => match shared.latest.report() {
            Some((report, age)) => {
                let age_ms = i64::try_from(age.as_millis()).unwrap_or(i64::MAX);
                match ActiveState::new(report, age_ms) {
                    Some(state) => {
                        let now_ms = wire.now_ms();
                        Announcement::active(
                            &wire.device_id(),
                            &wire.client_id(),
                            &wire.session_id(),
                            &state,
                            shared.registry.last_ack(),
                            u64::try_from(now_ms).unwrap_or(0),
                        )
                    }
                    None => Announcement::ready(
                        &wire.device_id(),
                        &wire.client_id(),
                        &wire.session_id(),
                    ),
                }
            }
            None => Announcement::ready(&wire.device_id(), &wire.client_id(), &wire.session_id()),
        },
    }
}

/// Runs one write on the wire under a budget, cut short when the device goes.
pub(crate) async fn bounded<T>(
    gate: &Gate,
    budget: Duration,
    work: impl Future<Output = Result<T, ConnectError>>,
) -> Result<T, ConnectError> {
    let mut withdrawn = gate.withdrawn_signal();
    if *withdrawn.borrow_and_update() {
        return Err(ConnectError::Withdrawn);
    }
    let raced = async {
        tokio::select! {
            biased;
            // The gate outlives the call that borrows it, so a closed sender
            // means the same thing the flag does.
            _ = withdrawn.changed() => Err(ConnectError::Withdrawn),
            result = work => result,
        }
    };
    match tokio::time::timeout(budget, raced).await {
        Ok(result) => result,
        Err(_) => Err(ConnectError::PutTimedOut),
    }
}

/// Gives the sound back, best effort, inside its own bound, and says whether
/// the account was told.
///
/// A put whose answer never came may still have been applied, so the caller
/// sends this whether the put looked successful or not. Only a give-back the
/// wire took settles the debt it answers.
pub(crate) async fn give_back<W: Wire>(wire: &W, timing: Timing) -> bool {
    matches!(
        tokio::time::timeout(timing.release, wire.put_inactive()).await,
        Ok(Ok(()))
    )
}

/// Reports the loss a view names, before the view itself reaches the window.
///
/// The clear happens inside the presence's own lock, so a second view of the
/// same loss finds nothing left to report.
pub(crate) fn report_loss(shared: &Shared, view: &ClusterView) {
    if let Observation::Lost { epoch } = shared.presence.observe(view, Instant::now()) {
        // A hold that ends is the one thing the window cannot show while it is
        // reading, so the log carries it: an unexpected loss looks exactly like
        // audio that never started.
        log::info!(
            target: crate::hold::HOLD_LOG_TARGET,
            "lost the sound: epoch {epoch} held_here={} changed_ms={}",
            view.held_here,
            view.changed_ms
        );
        shared.emit(ClusterEvent::Lost { epoch });
    }
}

/// What `watch` hands its caller: the events the window reads, the requests it
/// has to answer, and the bundle later calls announce through.
pub struct Watching {
    pub events: UnboundedReceiver<ClusterEvent>,
    pub inbound: UnboundedReceiver<Inbound>,
    pub(crate) shared: Arc<Shared>,
}

/// Starts listening for the cluster and returns the streams the window reads.
///
/// A second call on one session cannot work: `DealerManager::start` consumes the
/// builder it would subscribe on, so the caller keeps its own guard.
pub(crate) async fn watch(
    session: &Session,
    gate: Arc<Gate>,
    presence: Arc<Presence>,
    latest: Arc<Latest>,
    registry: Arc<Registry>,
) -> Result<Watching, ConnectError> {
    let dealer = session.dealer();
    let ids = dealer
        .listen_for(CONNECTION_URI, |message| {
            connection_id_of(&message).map_err(librespot::core::Error::failed_precondition)
        })
        .map_err(|_| ConnectError::DealerSpent)?;
    let pushes = dealer
        .listen_for(CLUSTER_URI, Message::from_raw::<ClusterUpdate>)
        .map_err(|_| ConnectError::DealerSpent)?;
    let requests = dealer
        .handle_for(COMMAND_URI)
        .map_err(|_| ConnectError::DealerSpent)?;
    dealer
        .start()
        .await
        .map_err(|_| ConnectError::DealerSpent)?;

    let (events, event_rx) = unbounded_channel();
    let (inbound_tx, inbound_rx) = unbounded_channel();
    let shared = Shared::new(presence, latest, registry, events.downgrade());
    let wire = Arc::new(SessionWire::new(session));
    let timing = Timing::default();

    tokio::spawn(receive::serve(
        requests,
        inbound_tx,
        Arc::clone(&shared.registry),
    ));
    tokio::spawn(publish::run(
        Arc::clone(&wire),
        Arc::clone(&gate),
        Arc::clone(&shared),
        timing,
    ));
    tokio::spawn(pump(
        wire,
        gate,
        ids,
        pushes,
        Arc::clone(&shared),
        events,
        timing,
    ));

    Ok(Watching {
        events: event_rx,
        inbound: inbound_rx,
        shared,
    })
}

/// Reads the dealer's two streams until they end or the window goes.
///
/// One `Failed` per failure and no exit: a silence of the account is the state
/// the window draws, not the end of the watch. `events` is the only strong
/// sender, so this task ending is what closes the window's stream.
pub(crate) async fn pump<W: Wire>(
    wire: Arc<W>,
    gate: Arc<Gate>,
    mut ids: BoxedStreamResult<String>,
    mut pushes: BoxedStreamResult<ClusterUpdate>,
    shared: Arc<Shared>,
    events: UnboundedSender<ClusterEvent>,
    timing: Timing,
) {
    let wait = tokio::time::sleep(timing.id_wait);
    tokio::pin!(wait);
    let mut awaiting_id = true;

    loop {
        let event = tokio::select! {
            biased;
            _ = events.closed() => break,
            _ = &mut wait, if awaiting_id => {
                awaiting_id = false;
                ClusterEvent::Failed
            }
            received = ids.next() => match received {
                Some(Ok(id)) => {
                    awaiting_id = false;
                    // A registration that lapsed may have handed the sound to
                    // whoever picked it up, so the hold ends before the new id
                    // is announced: the put that follows says what is left, and
                    // it is outside the gate, so a publisher waiting on it sees
                    // the cleared stage.
                    if let Some(epoch) = shared.presence.release_for_new_connection()
                        && !shared.emit(ClusterEvent::Lost { epoch })
                    {
                        break;
                    }
                    wire.set_connection_id(&id);
                    match announce(&*wire, &gate, &shared, timing).await {
                        Some(Ok(view)) => ClusterEvent::View(Box::new(view)),
                        Some(Err(_)) => ClusterEvent::Failed,
                        None => continue,
                    }
                }
                Some(Err(_)) => ClusterEvent::Failed,
                None => break,
            },
            pushed = pushes.next() => match pushed {
                Some(Ok(update)) => match view_of_update(&update, &wire.device_id()) {
                    Some(view) => {
                        report_loss(&shared, &view);
                        ClusterEvent::View(Box::new(view))
                    }
                    None => continue,
                },
                Some(Err(_)) => ClusterEvent::Failed,
                None => break,
            },
        };
        if events.send(event).is_err() {
            break;
        }
    }
}

/// Announces the device and reads the reply, unless a withdraw got there first.
async fn announce<W: Wire>(
    wire: &W,
    gate: &Gate,
    shared: &Shared,
    timing: Timing,
) -> Option<Result<ClusterView, ConnectError>> {
    let _guard = gate.take().await;
    if gate.withdrawn() {
        return None;
    }
    Some(put_and_view(wire, gate, shared, timing).await)
}

/// Sends the body the stage calls for and reads the cluster it answers with.
///
/// The caller holds the gate; the put itself is bounded, so a wire that never
/// answers cannot keep it.
pub(crate) async fn put_and_view<W: Wire>(
    wire: &W,
    gate: &Gate,
    shared: &Shared,
    timing: Timing,
) -> Result<ClusterView, ConnectError> {
    if wire.connection_id().is_empty() {
        return Err(ConnectError::NoConnectionId);
    }
    let announcement = body(wire, shared);
    let reply = bounded(gate, timing.put, wire.put(&announcement)).await?;
    let own = wire.device_id();
    let view = Cluster::parse_from_bytes(&reply)
        .map(|cluster| view_of(&cluster, &own))
        .map_err(ConnectError::from)?;
    // A reply names the holder as plainly as a push does.
    report_loss(shared, &view);
    Ok(view)
}

/// Asks the cluster for the roster again, answering what it says.
pub(crate) async fn refresh<W: Wire>(
    wire: &W,
    gate: &Gate,
    shared: &Shared,
    timing: Timing,
) -> Result<ClusterView, ConnectError> {
    let _guard = tokio::time::timeout(timing.gate_wait, gate.take())
        .await
        .map_err(|_| ConnectError::Busy)?;
    if gate.withdrawn() {
        return Err(ConnectError::Withdrawn);
    }
    put_and_view(wire, gate, shared, timing).await
}

/// Sends one transport action to the device that holds the sound.
///
/// A skip is relative, so it may be sent only once and must not share the wire
/// with another request; it waits for the gate a bounded time and then gives up.
/// The rest name an absolute state, so they repeat safely and never wait.
pub(crate) async fn steer<W: Wire>(
    wire: &W,
    gate: &Gate,
    target: &str,
    steer: &Steer,
    timing: Timing,
) -> Result<(), ConnectError> {
    let payload = steer_body(steer);
    if repeatable(steer) {
        return wire.command(target, &payload, false).await;
    }
    let _guard = tokio::time::timeout(timing.gate_wait, gate.take())
        .await
        .map_err(|_| ConnectError::Busy)?;
    wire.command(target, &payload, true).await
}

/// Deletes the device, then closes the dealer, inside one bound.
///
/// The flag goes up first, so a put already on the wire is cut short and an
/// announce that wins the gate afterwards sends nothing. A device that holds
/// the sound gives it back before it goes, inside a bound of its own, so the
/// delete always has time to run.
pub(crate) async fn withdraw<W: Wire>(
    wire: &W,
    gate: &Gate,
    presence: &Presence,
    timing: Timing,
) -> Result<(), ConnectError> {
    gate.mark_withdrawn();
    let work = async {
        let _guard = gate.take().await;
        // A device that may have taken the sound has something to give back,
        // even when no activation was recorded: a quit can drop an accept
        // between the put and the hold it would have marked.
        if presence.release().any() {
            presence.gave_back(give_back(wire, timing).await);
        }
        let deleted = wire.delete().await;
        wire.close().await;
        deleted
    };
    match tokio::time::timeout(timing.withdraw, work).await {
        Ok(result) => result,
        Err(_) => Err(ConnectError::WithdrawTimedOut),
    }
}

/// The connection id a pusher message carries.
pub(crate) fn connection_id_of(message: &Message) -> Result<String, ConnectError> {
    message
        .headers
        .get(CONNECTION_HEADER)
        .cloned()
        .ok_or(ConnectError::NoConnectionId)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use librespot::core::dealer::protocol::PayloadValue;
    use protobuf::MessageField;

    use super::*;
    use crate::loss::Baseline;
    use crate::testing::{
        CLIENT, DEVICE, FakeWire, Reply, SESSION, bundle, cluster, empty, in_flight, quiet, recv,
        start_publish, start_pump, until, view_of_event,
    };

    fn timing() -> Timing {
        Timing {
            id_wait: Duration::from_millis(20),
            withdraw: Duration::from_millis(50),
            gate_wait: Duration::from_millis(30),
            answer: Duration::from_millis(50),
            resolve: Duration::from_millis(20),
            put: Duration::from_millis(50),
            hand_over: Duration::from_millis(50),
            release: Duration::from_millis(20),
            grace: Duration::from_millis(50),
            debounce: Duration::from_millis(20),
        }
    }

    /// How many puts of one kind the wire has seen.
    fn starts(wire: &FakeWire, kind: &str) -> usize {
        let wanted = format!("put:{kind}:start");
        wire.log
            .entries()
            .iter()
            .filter(|entry| entry.as_str() == wanted)
            .count()
    }

    /// Pins the safety invariant on what the pump, the refresh and the publisher
    /// actually sent.
    fn assert_never_active(wire: &FakeWire) {
        assert!(
            wire.puts().iter().all(|put| !put.message().is_active),
            "an announcement that takes the sound reached the wire"
        );
        let entries = wire.log.entries();
        assert!(
            !entries.iter().any(|entry| entry.contains(":active:")),
            "an active body reached the wire: {entries:?}"
        );
    }

    #[tokio::test]
    async fn the_first_connection_id_pins_and_sends_the_controller_body() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), gate, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut pump.events).await;

        assert_eq!(wire.connection_id(), "connection-1");
        // `connect`'s own test pins this body against the probed literal, so
        // equality with its only constructor is equality with the wire's bytes.
        assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
    }

    #[tokio::test]
    async fn a_reply_becomes_a_view_and_a_garbage_one_only_fails() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), gate, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let view = view_of_event(recv(&mut pump.events).await);
        assert_eq!(
            view.devices
                .expect("a roster")
                .first()
                .map(|d| d.id.as_str()),
            Some("phone")
        );
        assert_eq!(view.holder.as_deref(), Some("phone"));

        wire.reply(Reply::Garbage);
        pump.ids.send(Ok("connection-2".to_owned())).expect("id");
        assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);
        assert_eq!(
            wire.puts().len(),
            2,
            "the pump keeps announcing after a bad reply"
        );

        wire.reply(Reply::Fail);
        pump.ids.send(Ok("connection-3".to_owned())).expect("id");
        assert_eq!(
            recv(&mut pump.events).await,
            ClusterEvent::Failed,
            "a failed announce is the same silence as an unreadable one"
        );

        wire.reply(Reply::Cluster);
        pump.ids.send(Ok("connection-4".to_owned())).expect("id");
        assert!(matches!(
            recv(&mut pump.events).await,
            ClusterEvent::View(_)
        ));
    }

    #[tokio::test]
    async fn a_missing_connection_id_fails_once_and_a_late_one_still_announces() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), gate, timing());

        assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            pump.events.try_recv().is_err(),
            "the one silence is reported once, not on a loop"
        );

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        assert!(matches!(
            recv(&mut pump.events).await,
            ClusterEvent::View(_)
        ));
        assert_eq!(wire.puts().len(), 1);
    }

    #[tokio::test]
    async fn a_second_connection_id_re_pins_and_announces_again() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), gate, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut pump.events).await;
        pump.ids.send(Ok("connection-2".to_owned())).expect("id");
        let _ = recv(&mut pump.events).await;

        assert_eq!(wire.connection_id(), "connection-2");
        assert_eq!(wire.puts().len(), 2);
    }

    #[tokio::test]
    async fn withdraw_deletes_then_closes_and_no_later_announce_sends() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut pump.events).await;

        withdraw(&*wire, &gate, &pump.shared.presence, timing())
            .await
            .expect("a withdraw on a watched device completes");
        assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
        assert_eq!(wire.closes.load(Ordering::SeqCst), 1);
        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["delete", "close"],
            "the device is deleted before the dealer closes"
        );

        pump.ids.send(Ok("connection-2".to_owned())).expect("id");
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            wire.puts().len(),
            1,
            "a connection id after the delete must not re-create the device"
        );
    }

    #[tokio::test]
    async fn a_delete_that_never_answers_releases_within_the_bound() {
        let wire = Arc::new(FakeWire::default());
        wire.delete_hangs.store(true, Ordering::SeqCst);
        let gate = Arc::new(Gate::default());

        let started = std::time::Instant::now();
        let result = withdraw(&*wire, &gate, &Presence::default(), timing()).await;
        let waited = started.elapsed();

        assert!(matches!(result, Err(ConnectError::WithdrawTimedOut)));
        assert!(waited >= timing().withdraw, "the bound is the wait");
        assert!(waited < Duration::from_secs(1), "the exit cannot hang");
        assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
        assert_eq!(
            wire.closes.load(Ordering::SeqCst),
            0,
            "a delete that never answers cannot reach the close"
        );
    }

    #[tokio::test]
    async fn a_withdraw_that_cannot_take_the_gate_still_blocks_later_announces() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        // Held by something a withdraw cannot cut, so the bound is what ends it.
        let held = gate.take().await;

        let result = withdraw(&*wire, &gate, &Presence::default(), timing()).await;
        assert!(
            matches!(result, Err(ConnectError::WithdrawTimedOut)),
            "the held gate is past the bound"
        );
        assert_eq!(wire.deletes.load(Ordering::SeqCst), 0);
        drop(held);

        let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            wire.puts().len(),
            0,
            "a withdraw that timed out still marks the device gone"
        );
    }

    #[tokio::test]
    async fn refresh_without_a_connection_id_sends_nothing() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());

        assert!(matches!(
            refresh(&*wire, &gate, &shared, timing()).await,
            Err(ConnectError::NoConnectionId)
        ));
        assert!(wire.puts().is_empty());

        wire.set_connection_id("connection-1");
        let view = refresh(&*wire, &gate, &shared, timing())
            .await
            .expect("a refresh answers");
        assert_eq!(view.holder.as_deref(), Some("phone"));
        assert_eq!(wire.puts().len(), 1);
    }

    #[tokio::test]
    async fn a_refresh_behind_a_blocked_announce_gives_up_as_busy() {
        let wire = Arc::new(FakeWire::default());
        wire.set_connection_id("connection-1");
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());
        let patient = Timing {
            put: Duration::from_secs(2),
            ..timing()
        };
        let (release, started) = wire.hold();

        let first_wire = Arc::clone(&wire);
        let first_gate = Arc::clone(&gate);
        let first_shared = Arc::clone(&shared);
        let first = tokio::spawn(async move {
            refresh(&*first_wire, &first_gate, &first_shared, patient).await
        });
        in_flight(started).await;

        assert!(matches!(
            refresh(&*wire, &gate, &shared, timing()).await,
            Err(ConnectError::Busy)
        ));
        assert_eq!(wire.puts().len(), 1, "the busy refresh sends nothing");

        release.send(()).expect("release");
        first
            .await
            .expect("the first refresh ends")
            .expect("the held announce answers");
    }

    #[tokio::test]
    async fn a_skip_waits_for_the_gate_and_the_rest_close_no_strategy() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let held = gate.take().await;

        assert!(
            matches!(
                steer(&*wire, &gate, "phone", &Steer::SkipNext, timing()).await,
                Err(ConnectError::Busy)
            ),
            "a skip gives up rather than wait on a busy wire"
        );
        assert!(
            wire.commands().is_empty(),
            "a skipped steer never reaches the strategy"
        );

        steer(&*wire, &gate, "phone", &Steer::Pause, timing())
            .await
            .expect("a pause names an absolute state, so it never waits");
        assert_eq!(
            wire.commands(),
            vec![(
                "phone".to_owned(),
                r#"{"command":{"endpoint":"pause"}}"#.to_owned(),
                false
            )]
        );
        drop(held);

        for (action, once) in [
            (Steer::Pause, false),
            (Steer::Resume, false),
            (Steer::SeekTo(60_000), false),
            (Steer::SkipNext, true),
            (Steer::SkipPrev, true),
        ] {
            steer(&*wire, &gate, "phone", &action, timing())
                .await
                .expect("a free wire answers");
            assert_eq!(
                wire.commands().last().map(|c| c.2),
                Some(once),
                "{action:?} carries the wrong attempt budget"
            );
        }
    }

    #[tokio::test]
    async fn a_push_becomes_a_view_and_a_garbage_one_only_fails() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), gate, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut pump.events).await;

        let update = ClusterUpdate {
            cluster: MessageField::some(cluster()),
            ..Default::default()
        };
        pump.pushes.send(Ok(update)).expect("push");
        let view = view_of_event(recv(&mut pump.events).await);
        assert_eq!(view.holder.as_deref(), Some("phone"));

        pump.pushes
            .send(Err(librespot::core::Error::failed_precondition(
                std::io::Error::other("garbage"),
            )))
            .expect("push");
        assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);

        let update = ClusterUpdate {
            cluster: MessageField::some(cluster()),
            ..Default::default()
        };
        pump.pushes.send(Ok(update)).expect("push");
        assert!(
            matches!(recv(&mut pump.events).await, ClusterEvent::View(_)),
            "the pump keeps reading after a bad push"
        );
        assert_eq!(wire.puts().len(), 1, "a push needs no announce");
    }

    #[tokio::test]
    async fn withdraw_while_active_releases_before_it_deletes() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());
        shared.presence.activate(Baseline::After(1));

        withdraw(&*wire, &gate, &shared.presence, timing())
            .await
            .expect("the withdraw completes");

        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["inactive", "delete", "close"],
            "the sound is given back before the device goes"
        );
        assert_eq!(
            wire.log.entries(),
            ["put:inactive:start", "put:inactive:done"],
            "the give-back is one bounded put"
        );
    }

    #[tokio::test]
    async fn withdraw_when_idle_does_not_release() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());

        withdraw(&*wire, &gate, &shared.presence, timing())
            .await
            .expect("the withdraw completes");

        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["delete", "close"],
            "a device that never held the sound has nothing to give back"
        );
        assert!(wire.log.entries().is_empty());
    }

    #[tokio::test]
    async fn a_failed_release_still_deletes() {
        let wire = Arc::new(FakeWire::default());
        wire.refuse_inactive();
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());
        shared.presence.activate(Baseline::After(1));

        withdraw(&*wire, &gate, &shared.presence, timing())
            .await
            .expect("the withdraw completes");

        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["inactive", "delete", "close"],
            "a give-back that failed does not cost the device its delete"
        );
        assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
        assert_eq!(wire.closes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_hung_release_is_cut_at_its_budget_and_the_delete_still_runs() {
        let wire = Arc::new(FakeWire::default());
        wire.hang_inactive();
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());
        shared.presence.activate(Baseline::After(1));
        let exit = Timing {
            release: Duration::from_millis(20),
            withdraw: Duration::from_millis(200),
            ..timing()
        };

        let started = std::time::Instant::now();
        withdraw(&*wire, &gate, &shared.presence, exit)
            .await
            .expect("the give-back's own bound frees the exit");
        let waited = started.elapsed();

        assert!(waited >= exit.release, "the give-back is bounded");
        assert!(waited < exit.withdraw, "and it is a sub-budget of the exit");
        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["inactive", "delete", "close"],
            "a give-back that never answers does not cost the device its delete"
        );
    }

    #[tokio::test]
    async fn a_withdraw_that_times_out_during_release_still_blocks_announces() {
        let wire = Arc::new(FakeWire::default());
        wire.hang_inactive();
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());
        shared.presence.activate(Baseline::After(1));
        let exit = Timing {
            release: Duration::from_millis(100),
            withdraw: Duration::from_millis(20),
            ..timing()
        };

        assert!(matches!(
            withdraw(&*wire, &gate, &shared.presence, exit).await,
            Err(ConnectError::WithdrawTimedOut)
        ));
        assert_eq!(
            wire.deletes.load(Ordering::SeqCst),
            0,
            "a give-back that ate the exit leaves no time to delete"
        );

        let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            wire.puts().is_empty(),
            "a withdraw that timed out still marks the device gone"
        );
    }

    #[test]
    fn a_target_id_cannot_change_the_command_path() {
        assert_eq!(
            command_endpoint("own", "phone").expect("a plain id is one segment"),
            "/connect-state/v1/player/command/from/own/to/phone"
        );
        assert!(matches!(
            command_endpoint("own", "phone/../other"),
            Err(ConnectError::InvalidTarget)
        ));
        assert!(matches!(
            command_endpoint("own", "phone?query"),
            Err(ConnectError::InvalidTarget)
        ));
    }

    #[test]
    fn a_target_cannot_change_the_path() {
        hand_over_path("own", "phone").expect("a plain id is one segment");
        assert!(
            matches!(
                hand_over_path("own", "own"),
                Err(ConnectError::InvalidTarget)
            ),
            "a move to this device's own id is not a hand-over"
        );
        for target in [
            "",
            ".",
            "..",
            "phone/../other",
            "phone/other",
            "phone?query",
            "phone#fragment",
            "phone%2Fother",
            "ph one",
            "phone\\other",
            "phone\u{0}",
        ] {
            assert!(
                matches!(
                    hand_over_path("own", target),
                    Err(ConnectError::InvalidTarget)
                ),
                "{target:?} must not widen the path"
            );
        }
        for source in ["", "own/other", "own?query"] {
            assert!(
                matches!(
                    hand_over_path(source, "phone"),
                    Err(ConnectError::InvalidTarget)
                ),
                "{source:?} must not widen the path"
            );
        }
    }

    #[tokio::test]
    async fn the_pump_ends_when_the_window_goes() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(wire, gate, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut pump.events).await;
        drop(pump.events);

        tokio::time::timeout(Duration::from_secs(2), pump.task)
            .await
            .expect("the pump must end with the window")
            .expect("the pump must not panic");
    }

    #[tokio::test]
    async fn the_pump_ends_when_the_streams_end() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (events, mut rx) = unbounded_channel();
        let shared = Shared::new(
            Arc::new(Presence::default()),
            Arc::new(Latest::default()),
            Arc::new(Registry::new(timing())),
            events.downgrade(),
        );

        let task = tokio::spawn(pump(
            Arc::clone(&wire),
            gate,
            empty(),
            empty(),
            shared,
            events,
            timing(),
        ));
        assert!(
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("the pump must end")
                .is_none()
        );
        task.await.expect("the pump must not panic");
    }

    #[test]
    fn the_connection_id_is_the_pusher_header() {
        let mut headers = HashMap::new();
        headers.insert(CONNECTION_HEADER.to_owned(), "connection-1".to_owned());
        let message = Message {
            headers,
            payload: PayloadValue::Empty,
            uri: CONNECTION_URI.to_owned(),
        };
        assert_eq!(
            connection_id_of(&message).expect("the header names the id"),
            "connection-1"
        );

        let bare = Message {
            headers: HashMap::new(),
            payload: PayloadValue::Empty,
            uri: CONNECTION_URI.to_owned(),
        };
        assert!(matches!(
            connection_id_of(&bare),
            Err(ConnectError::NoConnectionId)
        ));
    }

    #[tokio::test]
    async fn the_first_announce_is_a_controller_until_something_can_play() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), gate, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
        assert!(!wire.puts()[0].message().is_active);
    }

    #[tokio::test]
    async fn becoming_playable_re_announces_ready_once() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        assert!(pump.shared.presence.set_playable(true));
        until("the ready announce", || wire.puts().len() == 2).await;
        assert_eq!(wire.puts()[1], Announcement::ready(DEVICE, CLIENT, SESSION));

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            wire.puts().len(),
            2,
            "one flip is one announce, not one per check"
        );
        assert_never_active(&wire);
    }

    #[tokio::test]
    async fn becoming_unplayable_re_announces_the_controller() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        assert!(pump.shared.presence.set_playable(true));
        until("the ready announce", || wire.puts().len() == 2).await;

        assert!(pump.shared.presence.set_playable(false));
        until("the controller announce", || wire.puts().len() == 3).await;
        assert_eq!(wire.puts()[2], Announcement::controller(DEVICE, CLIENT));
    }

    #[tokio::test]
    async fn a_flip_before_the_connection_id_sends_nothing_until_the_id_arrives() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let patient = Timing {
            id_wait: Duration::from_secs(2),
            ..timing()
        };
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, patient);

        assert!(pump.shared.presence.set_playable(true));
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            wire.puts().is_empty(),
            "a put has no connection id to carry yet"
        );

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);
        assert_eq!(
            wire.puts(),
            vec![Announcement::ready(DEVICE, CLIENT, SESSION)],
            "the id announces the stage the device is in"
        );
    }

    #[tokio::test]
    async fn a_flip_after_withdraw_sends_nothing() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        withdraw(&*wire, &gate, &pump.shared.presence, timing())
            .await
            .expect("the withdraw completes");

        assert!(pump.shared.presence.set_playable(true));
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            wire.puts().len(),
            1,
            "a device that is gone announces nothing"
        );
    }

    #[tokio::test]
    async fn a_flip_while_an_announce_is_held_waits_for_the_gate() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let patient = Timing {
            put: Duration::from_secs(2),
            gate_wait: Duration::from_secs(2),
            ..timing()
        };
        let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, patient);

        let (release, started) = wire.hold();
        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        in_flight(started).await;

        assert!(pump.shared.presence.set_playable(true));
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            wire.puts().len(),
            1,
            "the flip waits behind the announce that holds the gate"
        );

        let _ = release.send(());
        until("the ready announce", || wire.puts().len() == 2).await;
        assert_eq!(wire.puts()[1], Announcement::ready(DEVICE, CLIENT, SESSION));
    }

    #[tokio::test]
    async fn a_refresh_announces_the_current_stage() {
        let wire = Arc::new(FakeWire::default());
        wire.set_connection_id("connection-1");
        let gate = Arc::new(Gate::default());
        let shared = bundle(timing());

        let _ = refresh(&*wire, &gate, &shared, timing())
            .await
            .expect("a controller refresh answers");
        assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
        assert_never_active(&wire);

        assert!(shared.presence.set_playable(true));
        let _ = refresh(&*wire, &gate, &shared, timing())
            .await
            .expect("a ready refresh answers");
        assert_eq!(wire.puts()[1], Announcement::ready(DEVICE, CLIENT, SESSION));
        assert_never_active(&wire);
    }

    #[tokio::test]
    async fn a_put_that_never_answers_is_cut_off_at_the_budget_and_reported_failed() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let short = Timing {
            put: Duration::from_millis(40),
            ..timing()
        };
        let (release, started) = wire.hold();
        let mut pump = start_pump(Arc::clone(&wire), gate, short);

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        in_flight(started).await;

        assert_eq!(recv(&mut pump.events).await, ClusterEvent::Failed);
        assert_eq!(
            wire.log.entries(),
            vec!["put:controller:start"],
            "a cut put never reached its answer"
        );
        let _ = release.send(());
    }

    #[tokio::test]
    async fn a_withdraw_cuts_an_in_flight_put_short_and_still_deletes() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let patient = Timing {
            put: Duration::from_secs(2),
            ..timing()
        };
        let (release, started) = wire.hold();
        let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        in_flight(started).await;

        let patient_exit = Timing {
            withdraw: Duration::from_millis(500),
            ..timing()
        };
        withdraw(&*wire, &gate, &pump.shared.presence, patient_exit)
            .await
            .expect("the cut announce frees the gate, so the exit finishes");
        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["delete", "close"],
            "a cut put does not cost the device its delete"
        );
        assert_eq!(
            wire.log.entries(),
            vec!["put:controller:start"],
            "the in-flight put was cut, not answered"
        );
        let _ = release.send(());
    }

    #[tokio::test]
    async fn a_slow_publisher_put_does_not_delay_a_cluster_push() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let patient = Timing {
            put: Duration::from_secs(2),
            gate_wait: Duration::from_secs(2),
            ..timing()
        };
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), patient);
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, patient);

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        let (release, started) = wire.hold();
        assert!(pump.shared.presence.set_playable(true));
        in_flight(started).await;

        let update = ClusterUpdate {
            cluster: MessageField::some(cluster()),
            ..Default::default()
        };
        pump.pushes.send(Ok(update)).expect("push");
        let view = view_of_event(recv(&mut pump.events).await);
        assert_eq!(view.holder.as_deref(), Some("phone"));

        let entries = wire.log.entries();
        assert!(
            entries.contains(&"put:ready:start".to_owned()),
            "the held put is the publisher's: {entries:?}"
        );
        assert!(
            !entries.contains(&"put:ready:done".to_owned()),
            "the push was read while the put was still on the wire: {entries:?}"
        );

        let _ = release.send(());
        until("the held put to answer", || {
            wire.log.entries().contains(&"put:ready:done".to_owned())
        })
        .await;
    }

    #[tokio::test]
    async fn a_pump_that_ends_closes_the_stream_the_window_reads() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        // Both dealer streams end, as they do when the session dies.
        drop(pump.ids);
        drop(pump.pushes);

        let closed = tokio::time::timeout(Duration::from_secs(2), async {
            while pump.events.recv().await.is_some() {}
        })
        .await;
        assert!(
            closed.is_ok(),
            "the stream must close when its pump ends, or the window never learns the devices are off"
        );
    }

    #[tokio::test]
    async fn a_failed_flip_put_is_retried_up_to_its_limit() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        wire.reply(Reply::Refused);
        assert!(pump.shared.presence.set_playable(true));
        until("the run of failed puts to end", || {
            starts(&wire, "ready") == usize::from(publish::PUT_ATTEMPTS)
        })
        .await;

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            starts(&wire, "ready"),
            usize::from(publish::PUT_ATTEMPTS),
            "a failed put is retried, and the retries are bounded"
        );
    }

    #[tokio::test]
    async fn a_failed_flip_put_reports_failed_once_per_run() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        wire.reply(Reply::Refused);
        assert!(pump.shared.presence.set_playable(true));
        until("the run of failed puts to end", || {
            starts(&wire, "ready") == usize::from(publish::PUT_ATTEMPTS)
        })
        .await;

        assert_eq!(
            quiet(&mut pump.events, Duration::from_millis(150)).await,
            vec![ClusterEvent::Failed],
            "one Failed per run of failures, not one per attempt"
        );
    }

    #[tokio::test]
    async fn a_successful_retry_clears_the_failure_run() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _publisher = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, timing());

        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);

        wire.replies([Reply::Refused, Reply::Cluster]);
        assert!(pump.shared.presence.set_playable(true));
        until("the retry to land", || {
            wire.log
                .entries()
                .iter()
                .filter(|entry| entry.as_str() == "put:ready:done")
                .count()
                == 2
        })
        .await;
        assert_eq!(starts(&wire, "ready"), 2, "the failure cost one retry");

        let events = quiet(&mut pump.events, Duration::from_millis(150)).await;
        assert!(
            matches!(
                events.as_slice(),
                [ClusterEvent::Failed, ClusterEvent::View(_)]
            ),
            "one Failed for the run, then the view the landed put answered with: {events:?}"
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            starts(&wire, "ready"),
            2,
            "a put that landed is not retried"
        );
    }
}
