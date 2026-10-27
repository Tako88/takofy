//! Taking the account's sound and giving it back.
//!
//! The phone is answered by what the server accepted, so a put comes before the
//! answer and an activation before the hold is reported: the order here is what
//! keeps a device from being told it holds the sound when it does not.

use std::time::Instant;

use librespot::protocol::connect::Cluster;
use protobuf::Message as _;
use takofy_model::{Landing, RequestId, Status};

use crate::connect::{Announcement, ClusterEvent, ClusterView, ConnectError, view_of};
use crate::loss::Baseline;
use crate::playstate::ActiveState;
use crate::presence::Owed;
use crate::registry::Verdict;
use crate::transfer::{self, Resolution, TransferPlan};
use crate::transfer_out;
use crate::watch::{Gate, Shared, Timing, Wire, bounded, give_back, hand_over_path, put_and_view};

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

    shared.latest.set(Some(landing.report()));
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

/// The same line for a pick, which has no phone request to answer.
fn refused(error: ConnectError) -> ConnectError {
    log::info!(target: HOLD_LOG_TARGET, "pick refused: {}", error.word());
    error
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

/// Moves the sound to another device, taking it first when it is not ours.
///
/// A hand-over that does not come from the sound's owner is acked and never
/// completes, so a device that is not holding the sound takes it before it
/// gives it away. The put is bounded and the hold is recorded only once the
/// server took it; a report-less or paused stage is refused with nothing sent,
/// because there is nothing to move.
///
/// The move itself is bounded too: it holds the gate, which every other write
/// and the exit path wait on, and the client under it retries a degraded wire
/// for far longer than a request may take.
///
/// A hand-over that fails leaves the hold and the local transport as they were:
/// the account never named the target, so this device still owns the sound. The
/// window pauses when the cluster names the target, which is the same loss rule
/// a push uses.
///
/// A hand-over already on the wire is refused. Its activation stays live until
/// the cluster names the target, so this device reads as the owner meanwhile;
/// handing over again from there is acked and never completes.
pub(crate) async fn pick<W: Wire>(
    wire: &W,
    gate: &Gate,
    shared: &Shared,
    to: &str,
    timing: Timing,
) -> Result<(), ConnectError> {
    let own = wire.device_id();
    // A target that would add a path segment, or is this device itself, is
    // refused before the account is touched at all.
    hand_over_path(&own, to)?;
    let _guard = tokio::time::timeout(timing.gate_wait, gate.take())
        .await
        .map_err(|_| ConnectError::Busy)?;
    if gate.withdrawn() {
        return Err(ConnectError::Withdrawn);
    }
    // The same two refusals `accept` makes, for the same reasons: a device the
    // dealer never named cannot answer the account's cluster, and a device that
    // cannot play must not take the sound even to pass it on.
    if wire.connection_id().is_empty() {
        return Err(refused(ConnectError::NoConnectionId));
    }
    if !shared.presence.playable() {
        return Err(refused(ConnectError::NotPlayable));
    }
    if shared.presence.picking() {
        return Err(refused(ConnectError::HandOverPending));
    }

    // Only what is moving can be moved on: a paused or report-less stage has
    // nothing to carry, and taking the sound for it would silence the account.
    let (report, age) = match shared.latest.report() {
        Some((report, age)) if matches!(report.status, Status::Playing | Status::Loading) => {
            (report, age)
        }
        _ => {
            log::info!(target: HOLD_LOG_TARGET, "pick refused: nothing playing");
            return Err(ConnectError::NotPlaying);
        }
    };
    // The target takes this state rather than resolving the context itself, so
    // the move names the rows — and names them by the account's own uids, which
    // is the only way it can find the row the sound is on. A context that cannot
    // be read is not a reason to refuse: the near rows still play, at whatever
    // position the target derives.
    let resolved = match report.context.as_deref() {
        Some(uri) if !uri.is_empty() => match wire.context_rows(uri).await {
            Ok(rows) => rows,
            Err(error) => {
                log::info!(
                    target: HOLD_LOG_TARGET,
                    "pick moving without the context rows: {}",
                    error.log_line()
                );
                Vec::new()
            }
        },
        _ => Vec::new(),
    };
    // A stage with no page is refused before the put, so the account is never
    // taken for nothing.
    let body = match transfer_out::body(&report, &own, wire.now_ms(), &resolved) {
        Ok(body) => body,
        Err(transfer_out::MoveError::NoTrack) => {
            log::info!(target: HOLD_LOG_TARGET, "pick refused: no track");
            return Err(ConnectError::NoTrack);
        }
        Err(transfer_out::MoveError::NoContext) => {
            log::info!(target: HOLD_LOG_TARGET, "pick refused: no context");
            return Err(ConnectError::NoContext);
        }
    };

    if !shared.presence.active() {
        let age_ms = i64::try_from(age.as_millis()).unwrap_or(i64::MAX);
        let Some(state) = ActiveState::new(report, age_ms) else {
            log::info!(target: HOLD_LOG_TARGET, "pick refused: no track");
            return Err(ConnectError::NoTrack);
        };
        let announcement = Announcement::active(
            &own,
            &wire.client_id(),
            &wire.session_id(),
            &state,
            shared.registry.last_ack(),
            u64::try_from(wire.now_ms()).unwrap_or(0),
        );
        // The answer to this put can be dropped with this future, so the
        // account may take the sound with no activation recorded here.
        shared.presence.put_issued();
        log::info!(target: HOLD_LOG_TARGET, "pick putting active");
        let reply = match bounded(gate, timing.put, wire.put(&announcement)).await {
            Ok(reply) => reply,
            Err(error) => {
                // The server may have applied a put whose answer never came.
                shared.presence.gave_back(give_back(wire, timing).await);
                log::info!(target: HOLD_LOG_TARGET, "pick put failed: {}", error.word());
                return Err(error);
            }
        };
        let view = Cluster::parse_from_bytes(&reply)
            .map(|cluster| view_of(&cluster, &own))
            .ok();
        let (baseline, _, _) = baseline_of(view.as_ref(), timing);
        shared.presence.activate(baseline);
    }

    log::info!(target: HOLD_LOG_TARGET, "pick handing over");
    // The hold outlives the move, so a second pick is refused until the cluster
    // names the target: a view reporting the loss, a release, or a new
    // connection id ends it, and so does a move that comes back a failure.
    shared.presence.pick_issued();
    match bounded(gate, timing.hand_over, wire.hand_over(to, &body)).await {
        Ok(()) => Ok(()),
        Err(error) => {
            // Nothing is in flight any more, and the sound never left.
            shared.presence.pick_settled();
            log::info!(target: HOLD_LOG_TARGET, "pick hand-over failed: {}", error.log_line());
            Err(match error {
                ConnectError::PutTimedOut => ConnectError::HandOverTimedOut,
                other => other,
            })
        }
    }
}

#[cfg(test)]
mod tests;
