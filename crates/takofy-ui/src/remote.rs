//! What the window does when the phone sends a transport action.
//!
//! The phone is the account's sound's owner no more: it stopped playing and
//! sends `pause`, `resume`, `seek_to`, `skip_next` and `skip_prev` instead. Each
//! is a decision about the queue and the transport, and an answer the phone is
//! waiting for, so the decision is a pure function the frame's own stores are
//! handed to.
//!
//! Every effect goes through the queue's own move, so a phone skip and a click
//! on the bar cannot land on two different rows.

use takofy_model::Order;

use crate::message::Command;
use crate::player::{Transport, TransportState};
use crate::queue::Queue;

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
    }
}

/// Pauses a transport that is moving, and leaves a still one where it is.
fn pause(transport: &Transport) -> Reaction {
    if transport.now_playing.is_none() {
        return Reaction::refused();
    }
    match transport.state {
        TransportState::Playing | TransportState::Loading => {
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
        TransportState::Playing | TransportState::Loading => Reaction::carried_out(Vec::new()),
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

#[cfg(test)]
mod tests {
    use takofy_model::TrackEntry;

    use super::*;
    use crate::player::RESTART_AFTER_MS;

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

    /// A queue started on `index`, with the transport the player reported.
    fn started(tracks: &[&str], index: usize) -> (Queue, Transport) {
        let rows: Vec<TrackEntry> = tracks.iter().map(|uri| track(uri)).collect();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let _ = queue.start(&rows, index, &mut transport);
        transport.state = TransportState::Playing;
        transport.duration_ms = Some(200_000);
        (queue, transport)
    }

    fn play(uri: &str) -> Command {
        Command::Play {
            uri: uri.to_owned(),
        }
    }

    #[test]
    fn pause_while_playing_sends_pause_and_is_ok() {
        let (_, mut transport) = started(&["a", "b"], 0);

        let reaction = react(&Order::Pause, &mut Queue::default(), &mut transport, 0);

        assert_eq!(reaction.commands, vec![Command::Pause]);
        assert!(reaction.ok);
    }

    #[test]
    fn pause_while_paused_is_ok_with_no_command() {
        let (_, mut transport) = started(&["a", "b"], 0);
        transport.state = TransportState::Paused;

        let reaction = react(&Order::Pause, &mut Queue::default(), &mut transport, 0);

        assert!(reaction.commands.is_empty(), "nothing is left to pause");
        assert!(reaction.ok);
    }

    #[test]
    fn pause_with_nothing_loaded_fails() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let reaction = react(&Order::Pause, &mut queue, &mut transport, 0);

        assert!(reaction.commands.is_empty());
        assert!(!reaction.ok, "the phone is told nothing could be paused");
    }

    #[test]
    fn resume_while_paused_sends_resume() {
        let (_, mut transport) = started(&["a", "b"], 0);
        transport.state = TransportState::Paused;

        let reaction = react(&Order::Resume, &mut Queue::default(), &mut transport, 0);

        assert_eq!(reaction.commands, vec![Command::Resume]);
        assert!(reaction.ok);
    }

    #[test]
    fn resume_while_stopped_replays_the_current_row() {
        let (mut queue, mut transport) = started(&["a", "b"], 1);
        transport.state = TransportState::Stopped;

        let reaction = react(&Order::Resume, &mut queue, &mut transport, 0);

        assert_eq!(reaction.commands, vec![play("b")], "the current row plays");
        assert!(reaction.ok);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("b"));
    }

    #[test]
    fn resume_with_no_current_row_fails() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        let reaction = react(&Order::Resume, &mut queue, &mut transport, 0);

        assert!(reaction.commands.is_empty());
        assert!(!reaction.ok, "there is nothing to start");
    }

    #[test]
    fn seek_when_seekable_sends_seek_and_otherwise_fails() {
        let (mut queue, mut transport) = started(&["a"], 0);

        let reaction = react(&Order::Seek(30_000), &mut queue, &mut transport, 0);
        assert_eq!(
            reaction.commands,
            vec![Command::Seek {
                position_ms: 30_000
            }]
        );
        assert!(reaction.ok);

        // A length the player never reported is nothing to jump in.
        transport.duration_ms = None;
        let reaction = react(&Order::Seek(30_000), &mut queue, &mut transport, 0);
        assert!(reaction.commands.is_empty());
        assert!(!reaction.ok);

        // Nor is a track that is not loaded at all.
        let mut idle = Transport::default();
        let reaction = react(&Order::Seek(1_000), &mut Queue::default(), &mut idle, 0);
        assert!(!reaction.ok);
    }

    #[test]
    fn skip_next_plays_the_next_row_and_fails_at_the_end() {
        let (mut queue, mut transport) = started(&["a", "b", "c"], 0);

        let reaction = react(&Order::Next(None), &mut queue, &mut transport, 0);
        assert_eq!(reaction.commands, vec![play("b")]);
        assert!(reaction.ok);

        // The last row has nothing after it: the queue is left where it was.
        queue.jump(2, &mut transport);
        let reaction = react(&Order::Next(None), &mut queue, &mut transport, 0);
        assert!(reaction.commands.is_empty());
        assert!(!reaction.ok);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("c"));
    }

    #[test]
    fn skip_next_to_a_named_track_jumps_to_that_upcoming_row_and_fails_if_absent() {
        let (mut queue, mut transport) = started(&["a", "b", "c"], 0);

        let reaction = react(
            &Order::Next(Some("c".to_owned())),
            &mut queue,
            &mut transport,
            0,
        );
        assert_eq!(reaction.commands, vec![play("c")], "the named row plays");
        assert!(reaction.ok);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("c"));

        let (mut queue, mut transport) = started(&["a", "b", "c"], 0);
        for name in ["gone", "a"] {
            let reaction = react(
                &Order::Next(Some(name.to_owned())),
                &mut queue,
                &mut transport,
                0,
            );
            assert!(reaction.commands.is_empty(), "{name} is not upcoming");
            assert!(!reaction.ok);
        }
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("a"),
            "a refused skip moves nothing"
        );
    }

    #[test]
    fn skip_prev_restarts_past_the_threshold_and_steps_back_otherwise() {
        let (mut queue, mut transport) = started(&["a", "b"], 1);
        transport.position_ms = RESTART_AFTER_MS + 1;

        let reaction = react(
            &Order::Prev,
            &mut queue,
            &mut transport,
            RESTART_AFTER_MS + 1,
        );
        assert_eq!(
            reaction.commands,
            vec![Command::Seek { position_ms: 0 }],
            "a play this far in restarts instead of leaving the row"
        );
        assert!(reaction.ok);

        let reaction = react(&Order::Prev, &mut queue, &mut transport, 0);
        assert_eq!(reaction.commands, vec![play("a")], "the one before plays");
        assert!(reaction.ok);

        // The first row has nowhere before it, so it restarts.
        let reaction = react(&Order::Prev, &mut queue, &mut transport, 0);
        assert!(reaction.ok);
        assert!(!reaction.commands.is_empty());
    }

    #[test]
    fn a_remote_skip_then_a_local_next_advance_twice_in_order() {
        let (mut queue, mut transport) = started(&["a", "b", "c"], 0);

        let remote = react(&Order::Next(None), &mut queue, &mut transport, 0);
        assert_eq!(remote.commands, vec![play("b")]);
        let local = queue.next(&mut transport);

        assert_eq!(local, Some(play("c")), "the two drivers take turns");
        assert!(remote.ok);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("c"));
    }
}
