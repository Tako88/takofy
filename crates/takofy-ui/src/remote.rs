//! What the window does when the phone sends a transport action.
//!
//! The phone is the account's sound's owner no more: it stopped playing and
//! sends `pause`, `resume`, `seek_to`, `skip_next` and `skip_prev` instead, and
//! when it edits the queue or the modes it sends those too. Each is a decision
//! about the queue and the transport, and an answer the phone is waiting for, so
//! the decision is a pure function the frame's own stores are handed to.
//!
//! Every effect goes through the queue's own move, so a phone skip and a click
//! on the bar cannot land on two different rows.

use takofy_model::Order;

use crate::message::Command;
use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::repeat::Repeat;

/// The order path's own log target, so the binary can let the one line that says
/// why a phone's order was not carried out through the app's floor without
/// raising it for everything.
pub const REMOTE_LOG_TARGET: &str = "takofy_ui::remote";

/// What one order asked this window to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reaction {
    /// The commands that carry the order out, in the order they are sent.
    pub commands: Vec<Command>,
    /// Whether the order was carried out. `false` means the phone is told
    /// `Failure` and nothing changed.
    pub ok: bool,
}

impl Reaction {
    fn carried_out(commands: Vec<Command>) -> Self {
        Self { commands, ok: true }
    }

    /// An order this window cannot carry out, which is answered as it is.
    fn refused() -> Self {
        Self::default()
    }
}

/// Carries out one order, at the position the transport shows.
pub fn react(
    order: &Order,
    queue: &mut Queue,
    transport: &mut Transport,
    position_ms: u32,
) -> Reaction {
    match order {
        Order::Pause => pause(transport),
        Order::Resume => resume(queue, transport),
        Order::Seek(position_ms) => seek(transport, *position_ms),
        Order::Next(named) => next(queue, transport, named.as_deref()),
        Order::Prev => previous(queue, transport, position_ms),
        Order::SetShuffle(on) => {
            set_shuffle(queue, *on);
            Reaction::carried_out(Vec::new())
        }
        Order::SetOptions {
            shuffling,
            repeating_context,
            repeating_track,
        } => options(queue, *shuffling, *repeating_context, *repeating_track),
        Order::AddToQueue(Some(uri)) => add_to_queue(queue, uri),
        // A queue addition with no track names nothing to put next, and a row
        // invented for it would be a row the phone never sent.
        Order::AddToQueue(None) => Reaction::refused(),
        Order::SetQueue {
            prev,
            next,
            revision,
        } => set_queue(queue, prev, next, revision),
    }
}

/// Pauses a transport that is moving, and leaves a still one where it is.
fn pause(transport: &Transport) -> Reaction {
    if transport.now_playing.is_none() {
        return Reaction::refused();
    }
    match transport.state {
        TransportState::Playing | TransportState::Loading | TransportState::Reconnecting => {
            Reaction::carried_out(vec![Command::Pause])
        }
        // Asking to pause what is already still names a state, not a step, so a
        // repeated order is obeyed rather than answered as a failure.
        TransportState::Paused => Reaction::carried_out(Vec::new()),
        // Starting something new is `play`'s business, and a phone that asked
        // to pause must not be given a different track.
        TransportState::Idle
        | TransportState::Stopped
        | TransportState::Unavailable
        | TransportState::Failed(_) => Reaction::refused(),
    }
}

/// Starts a paused transport, and replays the current row when nothing runs.
fn resume(queue: &mut Queue, transport: &mut Transport) -> Reaction {
    match transport.state {
        TransportState::Paused => Reaction::carried_out(vec![Command::Resume]),
        TransportState::Playing | TransportState::Loading | TransportState::Reconnecting => {
            Reaction::carried_out(Vec::new())
        }
        // Nothing is loaded to start, so the one row the queue can still name is
        // played; with no row there is nothing the phone could mean.
        TransportState::Idle
        | TransportState::Stopped
        | TransportState::Unavailable
        | TransportState::Failed(_) => {
            if queue.current().is_none() {
                return Reaction::refused();
            }
            queue
                .toggle(transport)
                .map_or_else(Reaction::refused, |command| {
                    Reaction::carried_out(vec![command])
                })
        }
    }
}

/// Jumps within the loaded track, when there is one to jump in.
fn seek(transport: &Transport, position_ms: u32) -> Reaction {
    if !transport.seekable() {
        return Reaction::refused();
    }
    Reaction::carried_out(vec![Command::Seek { position_ms }])
}

/// Steps forward, landing on the row the phone named when it named one.
fn next(queue: &mut Queue, transport: &mut Transport, named: Option<&str>) -> Reaction {
    let moved = match named {
        // A named row has to be one the queue would reach next, never one it has
        // already passed: skipping back is `skip_prev`'s business.
        Some(uri) => {
            let row = queue
                .upcoming()
                .find(|(_, entry)| entry.uri == uri)
                .map(|(row, _)| row);
            row.and_then(|row| queue.jump(row, transport))
        }
        None => queue.next(transport),
    };
    moved.map_or_else(Reaction::refused, |command| {
        Reaction::carried_out(vec![command])
    })
}

/// Restarts the current row past the threshold, and steps back before it.
fn previous(queue: &mut Queue, transport: &mut Transport, position_ms: u32) -> Reaction {
    queue
        .previous(position_ms, transport)
        .map_or_else(Reaction::refused, |command| {
            Reaction::carried_out(vec![command])
        })
}

/// Turns shuffle on or off, drawing the coming rows again only when the mode
/// really changes.
///
/// The phone's switch names a state, so an order that repeats the state the
/// queue is in is obeyed and redraws nothing: the rows someone is looking at
/// must not be permuted by an order that asked for what already holds.
fn set_shuffle(queue: &mut Queue, on: bool) {
    if queue.shuffle() != on {
        queue.toggle_shuffle();
    }
}

/// Applies the options the phone named, and only those.
///
/// Each field goes through the same path as the endpoint that names it alone, so
/// a set of options and a run of single orders cannot land differently. Repeat is
/// one three-state switch, so a half the phone left out keeps the mode it is in.
fn options(
    queue: &mut Queue,
    shuffling: Option<bool>,
    repeating_context: Option<bool>,
    repeating_track: Option<bool>,
) -> Reaction {
    if let Some(on) = shuffling {
        set_shuffle(queue, on);
    }
    let (context, track) = queue.repeat().flags();
    queue.set_repeat(Repeat::from_flags(
        repeating_context.unwrap_or(context),
        repeating_track.unwrap_or(track),
    ));
    Reaction::carried_out(Vec::new())
}

/// Puts the track the phone named next in the queue.
fn add_to_queue(queue: &mut Queue, uri: &str) -> Reaction {
    if queue.add_next_uri(uri).is_none() {
        return Reaction::refused();
    }
    // The row plays after the current one, so nothing is sent yet.
    Reaction::carried_out(Vec::new())
}

/// Replaces the queue with the window the phone published around its current
/// row.
fn set_queue(queue: &mut Queue, prev: &[String], next: &[String], revision: &str) -> Reaction {
    match queue.replace_window(prev, next, revision.to_owned()) {
        Ok(()) => Reaction::carried_out(Vec::new()),
        Err(refusal) => {
            // The kind only: the rows are the user's data and the phone's.
            log::info!(
                target: REMOTE_LOG_TARGET,
                "queue window refused: {}",
                refusal.word()
            );
            Reaction::refused()
        }
    }
}

#[cfg(test)]
mod tests;
