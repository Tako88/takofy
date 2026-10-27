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
use crate::transfer_out::ContextRow;

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
    /// Asks `to` to take the sound, carrying the state in `body`.
    fn hand_over(
        &self,
        to: &str,
        body: &str,
    ) -> impl Future<Output = Result<(), ConnectError>> + Send;
    /// The account's own rows for a context, so a move names a row by the name
    /// the target knows rather than one this client invented.
    fn context_rows(
        &self,
        uri: &str,
    ) -> impl Future<Output = Result<Vec<ContextRow>, ConnectError>> + Send;
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

    async fn hand_over(&self, to: &str, body: &str) -> Result<(), ConnectError> {
        // Not the REST transfer endpoint: that one only lets a device claim the
        // sound for itself, and refuses a push to another device. The move is
        // the dealer command the phone itself sends here, so the target claims
        // the sound with the state this body carries.
        self.command(to, body, false).await
    }

    async fn context_rows(&self, uri: &str) -> Result<Vec<ContextRow>, ConnectError> {
        let context = self.0.spclient().get_context(uri).await?;
        let mut rows = Vec::new();
        for page in &context.pages {
            for track in &page.tracks {
                let Some(track_uri) = track.uri.clone() else {
                    continue;
                };
                let Some(uid) = track.uid.clone() else {
                    continue;
                };
                if track_uri.is_empty() || uid.is_empty() {
                    continue;
                }
                rows.push(ContextRow {
                    uri: track_uri,
                    uid,
                });
            }
        }
        Ok(rows)
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
mod tests;
