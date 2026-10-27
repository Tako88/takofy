//! What this window is playing, as the account's phone needs it.
//!
//! A report is a snapshot of the queue and the transport, and the reporter is
//! the change detector over that snapshot: a frame's interpolated position is
//! not a change, but a seek, a pause, another track or an edited queue is.

use takofy_model::{MAX_NEXT_TRACKS, MAX_PREV_TRACKS, Options, Report, Status};

use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::repeat::Repeat;

/// Remembers the last report sent, so a frame that changed nothing sends
/// nothing.
#[derive(Debug, Clone, Default)]
pub struct Reporter {
    last: Option<Report>,
}

impl Reporter {
    /// The report to send, or `None` when the state is unchanged.
    ///
    /// The outer `Some` means a report is due; the inner value is the report
    /// itself, which may be `None` when the queue emptied.
    pub fn next(
        &mut self,
        queue: &Queue,
        transport: &Transport,
        position_ms: u32,
    ) -> Option<Option<Report>> {
        let report = report_of(queue, transport, position_ms);
        if report == self.last {
            return None;
        }
        self.last = report.clone();
        Some(report)
    }
}

/// The state this window would publish, or `None` when it has nothing true to
/// say.
pub fn report_of(queue: &Queue, transport: &Transport, position_ms: u32) -> Option<Report> {
    let track = queue.current()?.clone();
    let status = status_of(transport.state)?;
    // While playing, the wire advances the position by the report's age. The
    // frame's interpolated position would be advanced twice, so the anchor is
    // what travels; a seek moves the anchor and is a change.
    let position_ms = match transport.state {
        TransportState::Playing => transport.position_ms,
        _ => position_ms,
    };
    Some(Report {
        status,
        track: Some(track),
        prev: queue.history(MAX_PREV_TRACKS).cloned().collect(),
        next: queue
            .upcoming()
            .take(MAX_NEXT_TRACKS)
            .map(|(_, entry)| entry.clone())
            .collect(),
        context: queue.origin().map(str::to_owned),
        // The phone's own window carries a revision, and echoing it is how the
        // phone learns its push was taken rather than dropped.
        queue_revision: queue.queue_revision().to_owned(),
        options: Some(Options {
            shuffle: queue.shuffle(),
            repeat_context: queue.repeat() == Repeat::All,
            repeat_track: queue.repeat() == Repeat::One,
        }),
        position_ms,
        duration_ms: transport.duration_ms,
    })
}

/// The status the phone reads, or `None` when the transport cannot name one.
fn status_of(state: TransportState) -> Option<Status> {
    match state {
        TransportState::Playing => Some(Status::Playing),
        TransportState::Loading | TransportState::Reconnecting => Some(Status::Loading),
        TransportState::Paused
        | TransportState::Stopped
        | TransportState::Unavailable
        | TransportState::Idle => Some(Status::Paused),
        // A failed transport is not a state the account should copy: the phone
        // would show a track this window never played.
        TransportState::Failed(_) => None,
    }
}

#[cfg(test)]
mod tests;
