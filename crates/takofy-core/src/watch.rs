//! Watching the account's devices: the one seam that speaks the wire.
//!
//! One gate orders every announce, refresh, steer and withdraw, so a connection
//! id that arrives during a withdraw cannot re-create the device, and a skip's
//! single attempt cannot overlap another request. The pump is the only
//! long-lived reader; every other call is one request.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use http::{HeaderMap, HeaderValue, Method, header};
use librespot::core::Session;
use librespot::core::dealer::manager::BoxedStreamResult;
use librespot::core::dealer::protocol::Message;
use librespot::core::spclient::{RequestStrategy, SpClient};
use librespot::protocol::connect::{Cluster, ClusterUpdate};
use protobuf::Message as _;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::connect::{
    Announcement, ClusterEvent, ClusterView, ConnectError, Steer, WITHDRAW_TIMEOUT, repeatable,
    steer_body, view_of, view_of_update,
};

/// The stream the dealer names its connection id on.
const CONNECTION_URI: &str = "hm://pusher/v1/connections/";

/// The stream the cluster pushes on.
const CLUSTER_URI: &str = "hm://connect-state/v1/cluster";

/// The header the pusher names the connection id in. The put carries the same
/// id under `x-spotify-connection-id`; both spellings are the wire's.
const CONNECTION_HEADER: &str = "Spotify-Connection-Id";

/// How long the pump waits for a connection id before saying so.
pub(crate) const CONNECTION_ID_WAIT: Duration = Duration::from_secs(10);

/// The longest a skip waits for the wire to be free.
pub(crate) const GATE_WAIT: Duration = Duration::from_secs(2);

/// The slices of time the watch depends on, so a test can shorten them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    pub id_wait: Duration,
    pub withdraw: Duration,
    pub gate_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            id_wait: CONNECTION_ID_WAIT,
            withdraw: WITHDRAW_TIMEOUT,
            gate_wait: GATE_WAIT,
        }
    }
}

/// Orders the calls that write the account's device state.
///
/// `withdrawn` is set before the gate is taken, so a withdraw that never gets in
/// still stops every announce that comes after it.
#[derive(Default)]
pub(crate) struct Gate {
    lock: TokioMutex<()>,
    withdrawn: AtomicBool,
}

impl Gate {
    async fn take(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.lock.lock().await
    }

    fn mark_withdrawn(&self) {
        self.withdrawn.store(true, Ordering::SeqCst);
    }

    fn withdrawn(&self) -> bool {
        self.withdrawn.load(Ordering::SeqCst)
    }
}

/// The wire the watch needs, behind a seam a test can answer for.
pub(crate) trait Wire: Send + Sync + 'static {
    fn device_id(&self) -> String;
    fn client_id(&self) -> String;
    /// Empty until the dealer names one.
    fn connection_id(&self) -> String;
    fn set_connection_id(&self, id: &str);
    fn put(
        &self,
        announcement: &Announcement,
    ) -> impl Future<Output = Result<Vec<u8>, ConnectError>> + Send;
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

/// Builds the command path, refusing an id that would add or change a segment.
fn command_endpoint(own_id: &str, target: &str) -> Result<String, ConnectError> {
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
    Ok(format!(
        "/connect-state/v1/player/command/from/{}/to/{}",
        segment(own_id)?,
        segment(target)?
    ))
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

    async fn put(&self, announcement: &Announcement) -> Result<Vec<u8>, ConnectError> {
        let client = self.0.spclient();
        Ok(client
            .put_connect_state_request(announcement.message())
            .await?
            .to_vec())
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

/// Starts listening for the cluster and returns the stream the window reads.
///
/// A second call on one session cannot work: `DealerManager::start` consumes the
/// builder it would subscribe on, so the caller keeps its own guard.
pub(crate) async fn watch(
    session: &Session,
    gate: Arc<Gate>,
) -> Result<UnboundedReceiver<ClusterEvent>, ConnectError> {
    let dealer = session.dealer();
    let ids = dealer
        .listen_for(CONNECTION_URI, |message| {
            connection_id_of(&message).map_err(librespot::core::Error::failed_precondition)
        })
        .map_err(|_| ConnectError::DealerSpent)?;
    let pushes = dealer
        .listen_for(CLUSTER_URI, Message::from_raw::<ClusterUpdate>)
        .map_err(|_| ConnectError::DealerSpent)?;
    dealer
        .start()
        .await
        .map_err(|_| ConnectError::DealerSpent)?;
    let (tx, rx) = unbounded_channel();
    tokio::spawn(pump(
        Arc::new(SessionWire::new(session)),
        gate,
        ids,
        pushes,
        tx,
        Timing::default(),
    ));
    Ok(rx)
}

/// Reads the dealer's two streams until they end or the window goes.
///
/// One `Failed` per failure and no exit: a silence of the account is the state
/// the window draws, not the end of the watch.
pub(crate) async fn pump<W: Wire>(
    wire: Arc<W>,
    gate: Arc<Gate>,
    mut ids: BoxedStreamResult<String>,
    mut pushes: BoxedStreamResult<ClusterUpdate>,
    tx: UnboundedSender<ClusterEvent>,
    timing: Timing,
) {
    let wait = tokio::time::sleep(timing.id_wait);
    tokio::pin!(wait);
    let mut awaiting_id = true;

    loop {
        let event = tokio::select! {
            biased;
            _ = tx.closed() => break,
            _ = &mut wait, if awaiting_id => {
                awaiting_id = false;
                ClusterEvent::Failed
            }
            received = ids.next() => match received {
                Some(Ok(id)) => {
                    awaiting_id = false;
                    wire.set_connection_id(&id);
                    match announce(&*wire, &gate).await {
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
                    Some(view) => ClusterEvent::View(Box::new(view)),
                    None => continue,
                },
                Some(Err(_)) => ClusterEvent::Failed,
                None => break,
            },
        };
        if tx.send(event).is_err() {
            break;
        }
    }
}

/// Announces the device and reads the reply, unless a withdraw got there first.
async fn announce<W: Wire>(wire: &W, gate: &Gate) -> Option<Result<ClusterView, ConnectError>> {
    let _guard = gate.take().await;
    if gate.withdrawn() {
        return None;
    }
    Some(put_and_view(wire).await)
}

/// Sends the controller body and reads the cluster it answers with.
async fn put_and_view<W: Wire>(wire: &W) -> Result<ClusterView, ConnectError> {
    let announcement = Announcement::controller(&wire.device_id(), &wire.client_id());
    let reply = wire.put(&announcement).await?;
    let own = wire.device_id();
    Cluster::parse_from_bytes(&reply)
        .map(|cluster| view_of(&cluster, &own))
        .map_err(ConnectError::from)
}

/// Asks the cluster for the roster again, answering what it says.
pub(crate) async fn refresh<W: Wire>(
    wire: &W,
    gate: &Gate,
    timing: Timing,
) -> Result<ClusterView, ConnectError> {
    if wire.connection_id().is_empty() {
        return Err(ConnectError::NoConnectionId);
    }
    let _guard = tokio::time::timeout(timing.gate_wait, gate.take())
        .await
        .map_err(|_| ConnectError::Busy)?;
    if gate.withdrawn() {
        return Err(ConnectError::Withdrawn);
    }
    put_and_view(wire).await
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
    let body = steer_body(steer);
    if repeatable(steer) {
        return wire.command(target, &body, false).await;
    }
    let _guard = tokio::time::timeout(timing.gate_wait, gate.take())
        .await
        .map_err(|_| ConnectError::Busy)?;
    wire.command(target, &body, true).await
}

/// Deletes the device, then closes the dealer, inside one bound.
///
/// The flag goes up first so an announce already in flight that wins the gate
/// afterwards sends nothing.
pub(crate) async fn withdraw<W: Wire>(
    wire: &W,
    gate: &Gate,
    timing: Timing,
) -> Result<(), ConnectError> {
    gate.mark_withdrawn();
    let work = async {
        let _guard = gate.take().await;
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
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use librespot::core::dealer::protocol::PayloadValue;
    use librespot::protocol::connect::{Capabilities, DeviceInfo};
    use librespot::protocol::devices::DeviceType;
    use protobuf::{EnumOrUnknown, MessageField};
    use tokio::sync::mpsc::UnboundedSender;
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;

    use super::*;

    const DEVICE: &str = "device-1";
    const CLIENT: &str = "client-1";

    /// What one put answers.
    #[derive(Clone, Copy, Default, PartialEq, Eq)]
    enum Reply {
        #[default]
        Cluster,
        Garbage,
        Fail,
        /// Waits for the test's release before answering.
        Held,
    }

    fn device_info(name: &str) -> DeviceInfo {
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

    fn cluster() -> Cluster {
        let mut device = HashMap::new();
        device.insert("phone".to_owned(), device_info("Phone"));
        Cluster {
            device,
            active_device_id: "phone".to_owned(),
            ..Default::default()
        }
    }

    #[derive(Default)]
    struct FakeWire {
        connection_id: Mutex<String>,
        puts: Mutex<Vec<Announcement>>,
        commands: Mutex<Vec<(String, String, bool)>>,
        deletes: AtomicUsize,
        closes: AtomicUsize,
        exit_calls: Mutex<Vec<&'static str>>,
        reply: Mutex<Reply>,
        started: Mutex<Option<oneshot::Sender<()>>>,
        release: Mutex<Option<oneshot::Receiver<()>>>,
        delete_hangs: AtomicBool,
    }

    impl FakeWire {
        fn reply(&self, reply: Reply) {
            *self.reply.lock().expect("lock") = reply;
        }

        /// Holds the next put: `started` fires when it is in flight, and the
        /// put answers only once the returned sender fires.
        fn hold(&self) -> (oneshot::Sender<()>, oneshot::Receiver<()>) {
            self.reply(Reply::Held);
            let (started_tx, started_rx) = oneshot::channel();
            let (release_tx, release_rx) = oneshot::channel();
            *self.started.lock().expect("lock") = Some(started_tx);
            *self.release.lock().expect("lock") = Some(release_rx);
            (release_tx, started_rx)
        }

        fn puts(&self) -> Vec<Announcement> {
            self.puts.lock().expect("lock").clone()
        }

        fn commands(&self) -> Vec<(String, String, bool)> {
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

        async fn put(&self, announcement: &Announcement) -> Result<Vec<u8>, ConnectError> {
            self.puts.lock().expect("lock").push(announcement.clone());
            if let Some(started) = self.started.lock().expect("lock").take() {
                let _ = started.send(());
            }
            let held = *self.reply.lock().expect("lock") == Reply::Held;
            let release = self.release.lock().expect("lock").take();
            if held && let Some(release) = release {
                let _ = release.await;
            }
            match *self.reply.lock().expect("lock") {
                Reply::Cluster | Reply::Held => Ok(cluster().write_to_bytes().expect("bytes")),
                // A length-delimited field shorter than it claims is not a
                // cluster, whatever the parser is willing to skip.
                Reply::Garbage => Ok(vec![0x0a, 0x05, 0x00]),
                // Any failure will do: the pump reports the same either way.
                Reply::Fail => Err(ConnectError::NoConnectionId),
            }
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
    type Node<T> = Result<T, librespot::core::Error>;

    /// The connection id stream's sending end.
    type IdFeed = UnboundedSender<Node<String>>;

    /// The cluster push stream's sending end.
    type PushFeed = UnboundedSender<Node<ClusterUpdate>>;

    /// A stream the test feeds, so an item can arrive after the pump started.
    fn fed<T: Send + 'static>(rx: UnboundedReceiver<Node<T>>) -> BoxedStreamResult<T> {
        Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        }))
    }

    fn empty<T: Send + 'static>() -> BoxedStreamResult<T> {
        Box::pin(futures_util::stream::empty())
    }

    /// Starts a pump whose two streams the test feeds.
    fn start_pump(
        wire: Arc<FakeWire>,
        gate: Arc<Gate>,
        timing: Timing,
    ) -> (
        IdFeed,
        PushFeed,
        UnboundedReceiver<ClusterEvent>,
        JoinHandle<()>,
    ) {
        let (id_tx, id_rx) = unbounded_channel();
        let (push_tx, push_rx) = unbounded_channel();
        let (tx, rx) = unbounded_channel();
        let task = tokio::spawn(pump(wire, gate, fed(id_rx), fed(push_rx), tx, timing));
        (id_tx, push_tx, rx, task)
    }

    fn timing() -> Timing {
        Timing {
            id_wait: Duration::from_millis(20),
            withdraw: Duration::from_millis(50),
            gate_wait: Duration::from_millis(30),
        }
    }

    fn view_of_event(event: ClusterEvent) -> ClusterView {
        match event {
            ClusterEvent::View(view) => *view,
            other => panic!("expected a view, got {other:?}"),
        }
    }

    async fn recv(rx: &mut UnboundedReceiver<ClusterEvent>) -> ClusterEvent {
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("the pump must answer")
            .expect("the pump must still be alive")
    }

    #[tokio::test]
    async fn the_first_connection_id_pins_and_sends_the_controller_body() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (ids, _pushes, mut rx, _task) = start_pump(Arc::clone(&wire), gate, timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut rx).await;

        assert_eq!(wire.connection_id(), "connection-1");
        // `connect`'s own test pins this body against the probed literal, so
        // equality with its only constructor is equality with the wire's bytes.
        assert_eq!(wire.puts(), vec![Announcement::controller(DEVICE, CLIENT)]);
    }

    #[tokio::test]
    async fn a_reply_becomes_a_view_and_a_garbage_one_only_fails() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (ids, _pushes, mut rx, _task) = start_pump(Arc::clone(&wire), gate, timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        let view = view_of_event(recv(&mut rx).await);
        assert_eq!(
            view.devices
                .expect("a roster")
                .first()
                .map(|d| d.id.as_str()),
            Some("phone")
        );
        assert_eq!(view.holder.as_deref(), Some("phone"));

        wire.reply(Reply::Garbage);
        ids.send(Ok("connection-2".to_owned())).expect("id");
        assert_eq!(recv(&mut rx).await, ClusterEvent::Failed);
        assert_eq!(
            wire.puts().len(),
            2,
            "the pump keeps announcing after a bad reply"
        );

        wire.reply(Reply::Fail);
        ids.send(Ok("connection-3".to_owned())).expect("id");
        assert_eq!(
            recv(&mut rx).await,
            ClusterEvent::Failed,
            "a failed announce is the same silence as an unreadable one"
        );

        wire.reply(Reply::Cluster);
        ids.send(Ok("connection-4".to_owned())).expect("id");
        assert!(matches!(recv(&mut rx).await, ClusterEvent::View(_)));
    }

    #[tokio::test]
    async fn a_missing_connection_id_fails_once_and_a_late_one_still_announces() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (ids, _pushes, mut rx, _task) = start_pump(Arc::clone(&wire), gate, timing());

        assert_eq!(recv(&mut rx).await, ClusterEvent::Failed);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            rx.try_recv().is_err(),
            "the one silence is reported once, not on a loop"
        );

        ids.send(Ok("connection-1".to_owned())).expect("id");
        assert!(matches!(recv(&mut rx).await, ClusterEvent::View(_)));
        assert_eq!(wire.puts().len(), 1);
    }

    #[tokio::test]
    async fn a_second_connection_id_re_pins_and_announces_again() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (ids, _pushes, mut rx, _task) = start_pump(Arc::clone(&wire), gate, timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut rx).await;
        ids.send(Ok("connection-2".to_owned())).expect("id");
        let _ = recv(&mut rx).await;

        assert_eq!(wire.connection_id(), "connection-2");
        assert_eq!(wire.puts().len(), 2);
    }

    #[tokio::test]
    async fn withdraw_deletes_then_closes_and_no_later_announce_sends() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (ids, _pushes, mut rx, _task) =
            start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut rx).await;

        withdraw(&*wire, &gate, timing())
            .await
            .expect("a withdraw on a watched device completes");
        assert_eq!(wire.deletes.load(Ordering::SeqCst), 1);
        assert_eq!(wire.closes.load(Ordering::SeqCst), 1);
        assert_eq!(
            &*wire.exit_calls.lock().expect("lock"),
            &["delete", "close"],
            "the device is deleted before the dealer closes"
        );

        ids.send(Ok("connection-2".to_owned())).expect("id");
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
        let result = withdraw(&*wire, &gate, timing()).await;
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
        let (release, started) = wire.hold();
        let (ids, _pushes, mut rx, _task) =
            start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        started.await.expect("the announce is in flight");
        assert_eq!(wire.puts().len(), 1);

        let result = withdraw(&*wire, &gate, timing()).await;
        assert!(
            matches!(result, Err(ConnectError::WithdrawTimedOut)),
            "the blocked announce holds the gate past the bound"
        );
        assert_eq!(wire.deletes.load(Ordering::SeqCst), 0);

        let _ = release.send(());
        let _ = recv(&mut rx).await;
        ids.send(Ok("connection-2".to_owned())).expect("id");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            wire.puts().len(),
            1,
            "a withdraw that timed out still marks the device gone"
        );
    }

    #[tokio::test]
    async fn refresh_without_a_connection_id_sends_nothing() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());

        assert!(matches!(
            refresh(&*wire, &gate, timing()).await,
            Err(ConnectError::NoConnectionId)
        ));
        assert!(wire.puts().is_empty());

        wire.set_connection_id("connection-1");
        let view = refresh(&*wire, &gate, timing())
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
        let (release, started) = wire.hold();

        let first_wire = Arc::clone(&wire);
        let first_gate = Arc::clone(&gate);
        let first = tokio::spawn(async move { refresh(&*first_wire, &first_gate, timing()).await });
        started.await.expect("the announce is in flight");

        assert!(matches!(
            refresh(&*wire, &gate, timing()).await,
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
        let (ids, pushes, mut rx, _task) = start_pump(Arc::clone(&wire), gate, timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut rx).await;

        let update = ClusterUpdate {
            cluster: MessageField::some(cluster()),
            ..Default::default()
        };
        pushes.send(Ok(update)).expect("push");
        let view = view_of_event(recv(&mut rx).await);
        assert_eq!(view.holder.as_deref(), Some("phone"));

        pushes
            .send(Err(librespot::core::Error::failed_precondition(
                std::io::Error::other("garbage"),
            )))
            .expect("push");
        assert_eq!(recv(&mut rx).await, ClusterEvent::Failed);

        let update = ClusterUpdate {
            cluster: MessageField::some(cluster()),
            ..Default::default()
        };
        pushes.send(Ok(update)).expect("push");
        assert!(
            matches!(recv(&mut rx).await, ClusterEvent::View(_)),
            "the pump keeps reading after a bad push"
        );
        assert_eq!(wire.puts().len(), 1, "a push needs no announce");
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

    #[tokio::test]
    async fn the_pump_ends_when_the_window_goes() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (ids, _pushes, mut rx, task) = start_pump(wire, gate, timing());

        ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = recv(&mut rx).await;
        drop(rx);

        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("the pump must end with the window")
            .expect("the pump must not panic");
    }

    #[tokio::test]
    async fn the_pump_ends_when_the_streams_end() {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let (tx, mut rx) = unbounded_channel();

        let task = tokio::spawn(pump(
            Arc::clone(&wire),
            gate,
            empty(),
            empty(),
            tx,
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
}
