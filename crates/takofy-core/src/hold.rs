//! Taking the account's sound and giving it back.
//!
//! The phone is answered by what the server accepted, so a put comes before the
//! answer and an activation before the hold is reported: the order here is what
//! keeps a device from being told it holds the sound when it does not.

use std::time::Instant;

use librespot::protocol::connect::Cluster;
use protobuf::Message as _;
use takofy_model::{Landing, RequestId};

use crate::connect::{Announcement, ClusterEvent, ClusterView, ConnectError, view_of};
use crate::loss::Baseline;
use crate::playstate::ActiveState;
use crate::presence::Owed;
use crate::registry::Verdict;
use crate::transfer::{self, Resolution, TransferPlan};
use crate::watch::{Gate, Shared, Timing, Wire, bounded, give_back, put_and_view};

/// The log target of this module, so the binary can let the activation line
/// through the app's `Warn` floor without raising it for everything.
pub const HOLD_LOG_TARGET: &str = "takofy_core::hold";

/// Takes the sound for one request, and answers the phone at the end of it.
///
/// Every refusal answers `Failure` and sends nothing, so the phone's own
/// playback is untouched by a transfer this window cannot take. The hold
/// reaches the presence only after the server took the put: a failed or cut-off
/// put gives back whatever it may have applied and says so.
pub(crate) async fn accept<W: Wire>(
    wire: &W,
    gate: &Gate,
    shared: &Shared,
    id: RequestId,
    plan: &TransferPlan,
    resolution: Resolution,
    timing: Timing,
) -> Result<Landing, ConnectError> {
    // The budget may have answered the phone already, in which case there is
    // nothing left to take.
    let Some(ack) = shared.registry.ack_of(id) else {
        said(&ConnectError::Expired);
        return Err(ConnectError::Expired);
    };
    // A device that cannot play must never take the sound, whatever its caller
    // believes about the player.
    if !shared.presence.playable() {
        said(&ConnectError::NotPlayable);
        shared.registry.refuse(id);
        return Err(ConnectError::NotPlayable);
    }
    let _guard = match tokio::time::timeout(timing.gate_wait, gate.take()).await {
        Ok(guard) => guard,
        Err(_) => {
            said(&ConnectError::Busy);
            shared.registry.refuse(id);
            return Err(ConnectError::Busy);
        }
    };
    if gate.withdrawn() {
        said(&ConnectError::Withdrawn);
        shared.registry.refuse(id);
        return Err(ConnectError::Withdrawn);
    }
    if wire.connection_id().is_empty() {
        said(&ConnectError::NoConnectionId);
        shared.registry.refuse(id);
        return Err(ConnectError::NoConnectionId);
    }

    let now_ms = wire.now_ms();
    let started_at_ms = u64::try_from(now_ms).unwrap_or(0);
    let mut landing = transfer::land(plan, resolution, now_ms);
    let Some(state) = ActiveState::new(landing.report(), 0) else {
        said(&ConnectError::NoTrack);
        shared.registry.refuse(id);
        return Err(ConnectError::NoTrack);
    };
    let announcement = Announcement::active(
        &wire.device_id(),
        &wire.client_id(),
        &wire.session_id(),
        &state,
        Some(ack),
        started_at_ms,
    );

    // A hold taken before this request is remembered, so an arm that gives back
    // a put it could not keep can end that hold too: the phone was told the
    // hand-over failed and plays on, so playing here as well would put two
    // devices on one account. A refusal that never reached the wire leaves the
    // hold alone, because the account's own picture is unchanged.
    let held_before = shared.presence.active();
    // The answer to this put can be dropped with this future, so the account
    // may take the sound with no activation recorded here.
    shared.presence.put_issued();
    log::info!(target: HOLD_LOG_TARGET, "accept putting active");
    let reply = match bounded(gate, timing.put, wire.put(&announcement)).await {
        Ok(reply) => reply,
        Err(error) => {
            said(&error);
            // The server may have applied a put whose answer never came, so the
            // sound is given back before the phone is told it failed.
            refuse_after_the_put(wire, shared, id, timing, held_before).await;
            return Err(error);
        }
    };

    // The budget can answer during the put. When it did, the phone was told
    // `Failure` and the hold is not ours to keep.
    if shared.registry.ack_of(id).is_none() {
        said(&ConnectError::Expired);
        refuse_after_the_put(wire, shared, id, timing, held_before).await;
        return Err(ConnectError::Expired);
    }

    // The player can die while the put is on the wire, and the phone would then
    // be told this device took the sound and cannot play it.
    if !shared.presence.playable() {
        said(&ConnectError::NotPlayable);
        refuse_after_the_put(wire, shared, id, timing, held_before).await;
        return Err(ConnectError::NotPlayable);
    }

    let own = wire.device_id();
    let view = Cluster::parse_from_bytes(&reply)
        .map(|cluster| view_of(&cluster, &own))
        .ok();
    let (baseline, held_here, stamped) = baseline_of(view.as_ref(), timing);
    log::info!(
        target: HOLD_LOG_TARGET,
        "activation reply: held_here={held_here} stamped={stamped} baseline={}",
        match baseline {
            Baseline::After(_) => "after",
            Baseline::Until(_) => "grace",
        }
    );

    shared.hold(state, started_at_ms);
    let epoch = shared.presence.activate(baseline);
    landing.epoch = epoch;
    // The last answer can find the entry gone too, in the instant between the
    // peek above and this call; the hold then ends as it did for a lost put.
    if !shared.registry.answer(id, Verdict::Success) {
        end_the_hold(shared);
        shared.presence.gave_back(give_back(wire, timing).await);
        return Err(ConnectError::Expired);
    }
    Ok(landing)
}

/// One line per refusal, because an arm that answers the phone and says nothing
/// here is a hand-over nobody can debug from the outside.
fn said(error: &ConnectError) {
    log::info!(target: HOLD_LOG_TARGET, "accept refused: {}", error.word());
}

/// Gives back what a put may have taken and answers the phone `Failure`.
///
/// A hand-over the phone was told failed ends the hold this window already had:
/// the phone keeps or resumes the sound, so nothing may play on here as well.
async fn refuse_after_the_put<W: Wire>(
    wire: &W,
    shared: &Shared,
    id: RequestId,
    timing: Timing,
    held_before: bool,
) {
    // The give-back answers the debt unless the wire refused it, in which case
    // the marker stays for the exit path to try again.
    let confirmed = give_back(wire, timing).await;
    shared.presence.gave_back(confirmed);
    if held_before {
        end_the_hold(shared);
    }
    shared.registry.answer(id, Verdict::Failure);
}

/// Ends the activation that holds the sound, telling the window it is over.
fn end_the_hold(shared: &Shared) {
    if let Owed::Held { epoch } = shared.presence.release() {
        shared.emit(ClusterEvent::Lost { epoch });
    }
}

/// Which pushes this activation may ignore, and the two words the log records.
///
/// The reply is the only voice that can say where the hold starts. Without a
/// usable stamp of its own — an unreadable reply, a reply naming another
/// device, or no time at all — the activation is trusted for a short grace
/// instead, because a spurious pause is recoverable and playing over the owner
/// is not.
fn baseline_of(view: Option<&ClusterView>, timing: Timing) -> (Baseline, bool, bool) {
    let (held_here, changed_ms) = view.map_or((false, 0), |view| (view.held_here, view.changed_ms));
    let stamped = changed_ms != 0;
    let baseline = if held_here && changed_ms > 0 {
        Baseline::After(changed_ms)
    } else {
        Baseline::Until(Instant::now() + timing.grace)
    };
    (baseline, held_here, stamped)
}

/// Gives the sound back, then announces the stage that is left.
///
/// The give-back comes first, so the account stops naming this device as the
/// holder before it is told what the device is now. It runs under the gate, so
/// no put of the hold it is ending can land after it.
pub(crate) async fn release<W: Wire>(
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
    if shared.presence.release().any() {
        shared.presence.gave_back(give_back(wire, timing).await);
    }
    put_and_view(wire, gate, shared, timing).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use takofy_model::RequestId;

    use super::*;
    use crate::connect::{ClusterEvent, ClusterView};
    use crate::presence::{Observation, Presence};
    use crate::registry::{Ack, Registry};
    use crate::testing::{
        CLIENT, DEVICE, FakeResponder, FakeWire, Pump, Reply, SESSION, cluster_of, in_flight,
        pushed, quiet, recv, start_publish, start_pump, until, view_of_event,
    };
    use crate::transfer::ContextSource;
    use crate::watch::{Gate, Shared, refresh, withdraw};

    const TRACK_1: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";
    const TRACK_2: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";
    const TRACK_3: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ3";
    const PLAYLIST: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";

    /// The phone's snapshot: five seconds in, two seconds old when it lands.
    const POSITION_MS: u32 = 5_000;
    const SNAPSHOT_MS: i64 = 1_000_000;
    const NOW_MS: i64 = SNAPSHOT_MS + 2_000;

    /// The instant the account put on the accept's own reply.
    const REPLY_MS: i64 = 5_000;

    fn timing() -> Timing {
        Timing {
            id_wait: Duration::from_millis(20),
            withdraw: Duration::from_millis(50),
            gate_wait: Duration::from_millis(30),
            answer: Duration::from_millis(50),
            resolve: Duration::from_millis(20),
            put: Duration::from_millis(50),
            release: Duration::from_millis(20),
            grace: Duration::from_millis(50),
            debounce: Duration::from_millis(20),
        }
    }

    /// A wire that never cuts a held put, and a request that never expires.
    fn patient() -> Timing {
        Timing {
            put: Duration::from_secs(2),
            gate_wait: Duration::from_secs(2),
            answer: Duration::from_secs(30),
            ..timing()
        }
    }

    fn plan() -> TransferPlan {
        TransferPlan {
            current: TRACK_2.to_owned(),
            from_queue: false,
            queue: Vec::new(),
            associated: None,
            pages: vec![TRACK_1.to_owned(), TRACK_2.to_owned(), TRACK_3.to_owned()],
            context: Some(PLAYLIST.to_owned()),
            source: ContextSource::Pages,
            position_ms: Some(POSITION_MS),
            timestamp_ms: Some(SNAPSHOT_MS),
            paused: false,
            shuffle: false,
            repeat_context: false,
            repeat_track: false,
        }
    }

    fn resolved() -> Resolution {
        Resolution::Resolved(vec![
            TRACK_1.to_owned(),
            TRACK_2.to_owned(),
            TRACK_3.to_owned(),
        ])
    }

    fn rows() -> Vec<String> {
        vec![TRACK_1.to_owned(), TRACK_2.to_owned(), TRACK_3.to_owned()]
    }

    fn ack() -> Ack {
        Ack {
            message_id: 7,
            sender: "phone".to_owned(),
        }
    }

    /// Registers one request whose answer lands in the wire's own ordered log.
    fn pending(shared: &Shared, wire: &FakeWire, id: RequestId) {
        shared
            .registry
            .insert(id, ack(), Box::new(FakeResponder::new(wire.log.clone())));
    }

    fn view(held_here: bool, changed_ms: i64) -> ClusterView {
        ClusterView {
            devices: None,
            holder: (!held_here).then(|| "phone".to_owned()),
            held_here,
            changed_ms,
            playback: None,
        }
    }

    /// One accept path's world: the wire, the gate holding it, and the bundle
    /// every call reads.
    struct World {
        wire: Arc<FakeWire>,
        gate: Arc<Gate>,
        shared: Arc<Shared>,
    }

    impl World {
        fn new(timing: Timing) -> Self {
            let wire = Arc::new(FakeWire::default());
            wire.set_connection_id("connection-1");
            wire.set_now(NOW_MS);
            let presence = Arc::new(Presence::default());
            // Only a device that can play is ever handed the sound.
            presence.set_playable(true);
            let shared = Shared::detached(presence, Arc::new(Registry::new(timing)));
            Self {
                wire,
                gate: Arc::default(),
                shared,
            }
        }

        fn pending(&self, id: RequestId) {
            pending(&self.shared, &self.wire, id);
        }

        async fn accept(&self, id: RequestId, timing: Timing) -> Result<Landing, ConnectError> {
            accept(
                &*self.wire,
                &self.gate,
                &self.shared,
                id,
                &plan(),
                resolved(),
                timing,
            )
            .await
        }
    }

    /// The position one instant of the fake clock lands the snapshot at.
    fn position_at(now_ms: i64) -> u32 {
        transfer::landing_position(Some(POSITION_MS), Some(SNAPSHOT_MS), now_ms, false)
    }

    #[tokio::test]
    async fn accept_puts_active_then_answers_success() {
        let world = World::new(timing());
        let id = RequestId(1);
        world.pending(id);

        let landing = world.accept(id, timing()).await.expect("the accept lands");

        assert_eq!(landing.rows, rows());
        assert_eq!(landing.index, 1, "the phone's current track is the row");
        assert_eq!(landing.epoch, 1);
        assert_eq!(
            world.wire.log.entries(),
            ["put:active:start", "put:active:done", "reply:success"],
            "the put comes before the answer, and nothing else is sent"
        );
        assert!(world.shared.presence.active());
    }

    #[tokio::test]
    async fn the_reply_follows_the_done_of_the_put() {
        let world = World::new(timing());
        let id = RequestId(1);
        world.pending(id);

        world.accept(id, timing()).await.expect("the accept lands");

        let entries = world.wire.log.entries();
        let done = entries
            .iter()
            .position(|entry| entry == "put:active:done")
            .expect("the put finished");
        let reply = entries
            .iter()
            .position(|entry| entry == "reply:success")
            .expect("the phone was answered");
        assert!(
            done < reply,
            "the answer must follow the put that landed: {entries:?}"
        );
    }

    #[tokio::test]
    async fn accept_does_not_answer_while_the_put_is_in_flight() {
        let world = World::new(patient());
        let id = RequestId(1);
        world.pending(id);
        let (release, started) = world.wire.hold();
        let wire = Arc::clone(&world.wire);
        let gate = Arc::clone(&world.gate);
        let shared = Arc::clone(&world.shared);
        let task = tokio::spawn(async move {
            accept(&*wire, &gate, &shared, id, &plan(), resolved(), patient()).await
        });
        in_flight(started).await;

        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            world.wire.log.entries(),
            ["put:active:start"],
            "nothing is answered while the put is still on the wire"
        );

        release.send(()).expect("release");
        task.await
            .expect("the accept ends")
            .expect("the held accept lands");
        let entries = world.wire.log.entries();
        let done = entries
            .iter()
            .position(|entry| entry == "put:active:done")
            .expect("the released put finished");
        let reply = entries
            .iter()
            .position(|entry| entry == "reply:success")
            .expect("the phone was answered");
        assert!(done < reply, "{entries:?}");
    }

    #[tokio::test]
    async fn a_failed_put_answers_failure_and_never_activates() {
        let world = World::new(timing());
        world.wire.reply(Reply::Refused);
        let id = RequestId(1);
        world.pending(id);

        let result = world.accept(id, timing()).await;

        assert!(matches!(result, Err(ConnectError::PutTimedOut)));
        assert_eq!(
            world.wire.log.entries(),
            [
                "put:active:start",
                "put:active:done",
                "put:inactive:start",
                "put:inactive:done",
                "reply:failure",
            ],
            "a failed put gives the sound back before it answers failure"
        );
        assert!(!world.shared.presence.active());
    }

    #[tokio::test]
    async fn accept_after_withdraw_refuses_and_sends_nothing() {
        let world = World::new(timing());
        withdraw(&*world.wire, &world.gate, &world.shared.presence, timing())
            .await
            .expect("the withdraw completes");
        let id = RequestId(1);
        world.pending(id);

        assert!(matches!(
            world.accept(id, timing()).await,
            Err(ConnectError::Withdrawn)
        ));
        assert_eq!(world.wire.log.entries(), ["reply:failure"]);
        assert!(world.wire.puts().is_empty());
    }

    #[tokio::test]
    async fn accept_gives_up_busy_when_the_gate_is_held() {
        let world = World::new(timing());
        let held = world.gate.take().await;
        let id = RequestId(1);
        world.pending(id);

        assert!(matches!(
            world.accept(id, timing()).await,
            Err(ConnectError::Busy)
        ));
        assert_eq!(world.wire.log.entries(), ["reply:failure"]);
        assert!(world.wire.puts().is_empty());
        drop(held);
    }

    #[tokio::test]
    async fn accept_without_a_connection_id_refuses() {
        let world = World::new(timing());
        world.wire.set_connection_id("");
        let id = RequestId(1);
        world.pending(id);

        assert!(matches!(
            world.accept(id, timing()).await,
            Err(ConnectError::NoConnectionId)
        ));
        assert_eq!(world.wire.log.entries(), ["reply:failure"]);
        assert!(world.wire.puts().is_empty());
    }

    #[tokio::test]
    async fn accept_refuses_when_the_device_cannot_play() {
        let world = World::new(timing());
        world.shared.presence.set_playable(false);
        let id = RequestId(1);
        world.pending(id);

        assert!(matches!(
            world.accept(id, timing()).await,
            Err(ConnectError::NotPlayable)
        ));
        assert_eq!(world.wire.log.entries(), ["reply:failure"]);
        assert!(
            world.wire.puts().is_empty(),
            "a device that cannot play never reaches the wire"
        );
    }

    #[tokio::test]
    async fn accept_with_an_expired_entry_sends_nothing() {
        let short = Timing {
            answer: Duration::from_millis(20),
            ..timing()
        };
        let world = World::new(short);
        let id = RequestId(1);
        world.pending(id);
        until("the entry to expire", || {
            world.shared.registry.ack_of(id).is_none()
        })
        .await;
        let before = world.wire.log.entries();

        assert!(matches!(
            world.accept(id, short).await,
            Err(ConnectError::Expired)
        ));
        assert_eq!(
            world.wire.log.entries(),
            before,
            "the peek answers nothing and sends nothing"
        );
        assert!(world.wire.puts().is_empty());
    }

    #[tokio::test]
    async fn accept_finding_the_entry_gone_after_the_put_releases_and_does_not_activate() {
        let timing = Timing {
            answer: Duration::from_millis(20),
            ..patient()
        };
        let world = World::new(timing);
        let id = RequestId(1);
        world.pending(id);
        let (release, started) = world.wire.hold();
        let wire = Arc::clone(&world.wire);
        let gate = Arc::clone(&world.gate);
        let shared = Arc::clone(&world.shared);
        let task = tokio::spawn(async move {
            accept(&*wire, &gate, &shared, id, &plan(), resolved(), timing).await
        });
        in_flight(started).await;
        until("the entry to expire during the put", || {
            world.shared.registry.ack_of(id).is_none()
        })
        .await;

        release.send(()).expect("release");
        let result = task.await.expect("the accept ends");

        assert!(matches!(result, Err(ConnectError::Expired)));
        assert!(!world.shared.presence.active());
        let entries = world.wire.log.entries();
        let done = entries
            .iter()
            .position(|entry| entry == "put:active:done")
            .expect("the released put finished");
        assert_eq!(
            &entries[done + 1..],
            ["put:inactive:start", "put:inactive:done"],
            "a put that landed after the phone was answered is given back: {entries:?}"
        );
    }

    #[tokio::test]
    async fn a_slow_active_put_is_cut_off_at_the_put_budget_and_releases_best_effort() {
        let timing = Timing {
            put: Duration::from_millis(40),
            ..patient()
        };
        let world = World::new(Timing {
            answer: Duration::from_secs(30),
            ..timing
        });
        let id = RequestId(1);
        world.pending(id);
        let (release, _started) = world.wire.hold();

        let result = world.accept(id, timing).await;

        // The held put was cut, so it never logged its answer.
        assert!(matches!(result, Err(ConnectError::PutTimedOut)));
        assert_eq!(
            world.wire.log.entries(),
            [
                "put:active:start",
                "put:inactive:start",
                "put:inactive:done",
                "reply:failure",
            ]
        );
        assert!(!world.shared.presence.active());
        let _ = release.send(());
    }

    #[test]
    fn the_accept_path_fits_inside_the_answer_budget() {
        let timing = Timing::default();

        assert!(
            timing.resolve + timing.gate_wait + timing.put + timing.release < timing.answer,
            "a transfer must be answered before the phone gives up on it"
        );
    }

    #[tokio::test]
    async fn a_successful_accept_marks_active_with_the_replys_changed_time() {
        let world = World::new(timing());
        world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
        let id = RequestId(1);
        world.pending(id);

        let landing = world.accept(id, timing()).await.expect("the accept lands");

        assert!(world.shared.presence.active());
        assert_eq!(
            world
                .shared
                .presence
                .observe(&view(false, REPLY_MS - 1), Instant::now()),
            Observation::Stale,
            "a push stamped before the reply predates the hold"
        );
        assert_eq!(
            world
                .shared
                .presence
                .observe(&view(false, REPLY_MS + 1), Instant::now()),
            Observation::Lost {
                epoch: landing.epoch
            },
            "a push stamped after the reply is not ours to ignore"
        );
    }

    #[tokio::test]
    async fn an_accept_whose_reply_has_no_changed_time_uses_the_grace_baseline() {
        // The grace is longer than any scheduler stall, so neither assertion
        // below can lose a race with the real clock.
        let timing = Timing {
            grace: Duration::from_secs(30),
            ..timing()
        };
        let world = World::new(timing);
        world.wire.answer_with(cluster_of(DEVICE, 0));
        let id = RequestId(1);
        world.pending(id);

        let landing = world.accept(id, timing).await.expect("the accept lands");

        // An unstamped push is treated as fresh by the `After` baseline, so
        // only the grace baseline ignores it.
        assert_eq!(
            world
                .shared
                .presence
                .observe(&view(false, 0), Instant::now()),
            Observation::Stale
        );
        assert_eq!(
            world.shared.presence.observe(
                &view(false, 0),
                Instant::now() + timing.grace + Duration::from_millis(1)
            ),
            Observation::Lost {
                epoch: landing.epoch
            }
        );
    }

    #[tokio::test]
    async fn the_active_body_is_the_landing_report_as_an_announcement() {
        let world = World::new(timing());
        world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
        let id = RequestId(1);
        world.pending(id);

        let landing = world.accept(id, timing()).await.expect("the accept lands");

        let state = ActiveState::new(landing.report(), 0).expect("a landing names a track");
        assert_eq!(
            world.wire.puts(),
            vec![Announcement::active(
                DEVICE,
                CLIENT,
                SESSION,
                &state,
                Some(ack()),
                u64::try_from(NOW_MS).expect("a positive instant"),
            )]
        );
        assert_eq!(
            landing.position_ms,
            position_at(NOW_MS),
            "the landing carries the position it was taken at"
        );
    }

    #[tokio::test]
    async fn refresh_while_active_sends_the_active_body() {
        let world = World::new(timing());
        world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
        let id = RequestId(1);
        world.pending(id);
        world.accept(id, timing()).await.expect("the accept lands");
        let held = world.wire.puts()[0].clone();
        world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));

        refresh(&*world.wire, &world.gate, &world.shared, timing())
            .await
            .expect("a refresh answers");

        let puts = world.wire.puts();
        assert_eq!(puts.len(), 2);
        assert_eq!(
            puts[1], held,
            "a refresh re-asserts the hold, not the stage below it"
        );
        assert!(puts[1].message().is_active);
        assert!(
            world.shared.presence.active(),
            "the refresh's own reply names this device"
        );
    }

    #[tokio::test]
    async fn a_publisher_flip_while_active_sends_the_active_body() {
        let wire = Arc::new(FakeWire::default());
        wire.set_connection_id("connection-1");
        wire.set_now(NOW_MS);
        let gate = Arc::new(Gate::default());
        let publisher = Timing {
            debounce: Duration::from_millis(300),
            ..timing()
        };
        let pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing());
        let _flipping = start_publish(Arc::clone(&wire), Arc::clone(&gate), &pump, publisher);
        pump.shared.presence.set_playable(true);
        let id = RequestId(1);
        pending(&pump.shared, &wire, id);
        wire.answer_with(cluster_of(DEVICE, REPLY_MS));

        // The flip wakes the publisher, and the hold lands inside its debounce,
        // so the body it puts is the stage at put time.
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

        until("the publisher's active body", || {
            wire.puts()
                .iter()
                .filter(|put| put.message().is_active)
                .count()
                == 2
        })
        .await;
        let active: Vec<Announcement> = wire
            .puts()
            .into_iter()
            .filter(|put| put.message().is_active)
            .collect();
        assert_eq!(active.len(), 2, "one accept and one flip, one body each");
        assert_eq!(
            active[0], active[1],
            "a flip while active re-announces the hold, not a waiting device"
        );
    }

    #[tokio::test]
    async fn a_failed_second_transfer_gives_up_the_hold_it_already_had() {
        let (wire, gate, mut pump) = a_held_pump(timing()).await;
        let first = RequestId(1);
        pending(&pump.shared, &wire, first);
        wire.answer_with(cluster_of(DEVICE, REPLY_MS));
        let landing = accept(
            &*wire,
            &gate,
            &pump.shared,
            first,
            &plan(),
            resolved(),
            timing(),
        )
        .await
        .expect("the first accept lands");
        assert!(pump.shared.presence.active());

        // A second hand-over the wire refuses: the phone plays on, so the hold
        // it already had here cannot stand.
        wire.reply(Reply::Refused);
        let second = RequestId(2);
        pending(&pump.shared, &wire, second);

        let result = accept(
            &*wire,
            &gate,
            &pump.shared,
            second,
            &plan(),
            resolved(),
            timing(),
        )
        .await;

        assert!(matches!(result, Err(ConnectError::PutTimedOut)));
        assert!(
            !pump.shared.presence.active(),
            "a hold the phone was told failed is not kept"
        );
        assert_eq!(
            recv(&mut pump.events).await,
            ClusterEvent::Lost {
                epoch: landing.epoch
            },
            "the window is told the hold ended, so it pauses"
        );

        wire.reply(Reply::Cluster);
        refresh(&*wire, &gate, &pump.shared, timing())
            .await
            .expect("a refresh answers");
        assert!(
            !wire.puts().last().expect("a body").message().is_active,
            "the next put does not re-assert a hold the phone was told failed"
        );
    }

    #[tokio::test]
    async fn a_quit_during_an_active_put_still_gives_the_sound_back() {
        let world = World::new(patient());
        let id = RequestId(1);
        world.pending(id);
        let (release, started) = world.wire.hold();
        let wire = Arc::clone(&world.wire);
        let gate = Arc::clone(&world.gate);
        let shared = Arc::clone(&world.shared);
        let task = tokio::spawn(async move {
            accept(&*wire, &gate, &shared, id, &plan(), resolved(), patient()).await
        });
        in_flight(started).await;

        // The window closes where it stands, so the accept is dropped mid-put
        // and never reaches the hold it would have recorded.
        task.abort();
        assert!(task.await.is_err(), "the accept was dropped, not finished");
        let _ = release.send(());

        withdraw(&*world.wire, &world.gate, &world.shared.presence, timing())
            .await
            .expect("the withdraw completes");

        assert!(!world.shared.presence.active());
        assert_eq!(
            world.wire.log.entries(),
            [
                "put:active:start",
                "put:inactive:start",
                "put:inactive:done"
            ],
            "a put nobody answered is given back on the way out"
        );
    }

    #[tokio::test]
    async fn an_accept_whose_player_dies_during_the_put_answers_failure_and_does_not_activate() {
        let world = World::new(patient());
        let id = RequestId(1);
        world.pending(id);
        let (release, started) = world.wire.hold();
        let wire = Arc::clone(&world.wire);
        let gate = Arc::clone(&world.gate);
        let shared = Arc::clone(&world.shared);
        let task = tokio::spawn(async move {
            accept(&*wire, &gate, &shared, id, &plan(), resolved(), patient()).await
        });
        in_flight(started).await;

        // The player dies while the put is on the wire.
        world.shared.presence.set_playable(false);
        release.send(()).expect("release");
        let result = task.await.expect("the accept ends");

        assert!(matches!(result, Err(ConnectError::NotPlayable)));
        assert!(
            !world.shared.presence.active(),
            "a device that cannot play never keeps the sound"
        );
        assert_eq!(
            world.wire.log.entries(),
            [
                "put:active:start",
                "put:active:done",
                "put:inactive:start",
                "put:inactive:done",
                "reply:failure",
            ],
            "the sound a landed put took is given back before the phone is told"
        );
    }

    #[tokio::test]
    async fn playable_false_while_active_releases_and_announces_the_controller() {
        let world = World::new(timing());
        world.wire.answer_with(cluster_of(DEVICE, REPLY_MS));
        let id = RequestId(1);
        world.pending(id);
        world.accept(id, timing()).await.expect("the accept lands");
        assert!(world.shared.presence.active());

        world.shared.presence.set_playable(false);
        release(&*world.wire, &world.gate, &world.shared, timing())
            .await
            .expect("the release answers");

        assert_eq!(
            world.wire.log.entries(),
            [
                "put:active:start",
                "put:active:done",
                "reply:success",
                "put:inactive:start",
                "put:inactive:done",
                "put:controller:start",
                "put:controller:done",
            ],
            "the sound is given back before the device says what it is now"
        );
        assert_eq!(
            world.wire.puts().last(),
            Some(&Announcement::controller(DEVICE, CLIENT))
        );
        assert!(!world.shared.presence.active());
    }

    /// A pump whose stage is playable, with one held request already answered.
    async fn a_held_pump(timing: Timing) -> (Arc<FakeWire>, Arc<Gate>, Pump) {
        let wire = Arc::new(FakeWire::default());
        let gate = Arc::new(Gate::default());
        let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing);
        pump.shared.presence.set_playable(true);
        pump.ids.send(Ok("connection-1".to_owned())).expect("id");
        let _ = view_of_event(recv(&mut pump.events).await);
        (wire, gate, pump)
    }

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

    #[tokio::test]
    async fn accept_computes_the_landing_at_the_put_time_not_the_receive_time() {
        let world = World::new(timing());
        let id = RequestId(1);
        world.pending(id);
        world.wire.set_now(NOW_MS);
        // Held by the test, so the accept waits where it reads the clock.
        let held = world.gate.take().await;
        let wire = Arc::clone(&world.wire);
        let gate = Arc::clone(&world.gate);
        let shared = Arc::clone(&world.shared);
        let task = tokio::spawn(async move {
            accept(&*wire, &gate, &shared, id, &plan(), resolved(), timing()).await
        });
        // The accept has peeked its ack and is parked on the gate.
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        let later = NOW_MS + 3_000;
        world.wire.set_now(later);
        drop(held);

        let landing = task
            .await
            .expect("the accept ends")
            .expect("the accept lands");

        assert_eq!(
            landing.position_ms,
            position_at(later),
            "the position is the clock's at the put, not at the receive"
        );
    }
}
