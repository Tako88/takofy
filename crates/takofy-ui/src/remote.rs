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

    /// Every row the queue holds, in the order it has them.
    fn uris(queue: &Queue) -> Vec<String> {
        queue
            .history(usize::MAX)
            .chain(queue.current())
            .chain(queue.upcoming().map(|(_, entry)| entry))
            .map(|entry| entry.uri.clone())
            .collect()
    }

    /// One row the phone's window carries, named the way the wire names it.
    fn window(name: &str) -> String {
        format!("spotify:track:{name}")
    }

    /// The target the binary has to name to let the refusal line through, so the
    /// name is derived from the module rather than typed twice.
    #[test]
    fn the_log_target_is_the_module_that_logs() {
        assert_eq!(
            module_path!().strip_suffix("::tests"),
            Some(REMOTE_LOG_TARGET),
            "the refusal's target must be the module that logs it"
        );
    }

    #[test]
    fn set_shuffle_toggles_only_when_it_differs() {
        let (mut queue, mut transport) = started(&["a", "b", "c", "d"], 0);
        assert!(!queue.shuffle());

        let reaction = react(&Order::SetShuffle(true), &mut queue, &mut transport, 0);
        assert!(reaction.ok, "turning a mode on is always obeyed");
        assert!(reaction.commands.is_empty(), "nothing is loaded by a mode");
        assert!(queue.shuffle());
        let drawn = uris(&queue);
        assert_eq!(drawn.first().map(String::as_str), Some("a"));

        let reaction = react(&Order::SetShuffle(true), &mut queue, &mut transport, 0);
        assert!(reaction.ok);
        assert_eq!(
            uris(&queue),
            drawn,
            "an order asking for the mode already held redraws nothing"
        );

        let reaction = react(&Order::SetShuffle(false), &mut queue, &mut transport, 0);
        assert!(reaction.ok);
        assert!(!queue.shuffle());
        assert_eq!(
            uris(&queue),
            vec![
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "d".to_owned()
            ],
            "turning it off puts the list's own order back"
        );
    }

    #[test]
    fn every_pair_of_repeat_flags_names_a_mode() {
        let (mut queue, mut transport) = started(&["a", "b"], 0);
        let table = [
            ((Some(false), Some(false)), Repeat::Off),
            ((Some(true), Some(false)), Repeat::All),
            ((Some(false), Some(true)), Repeat::One),
            ((Some(true), Some(true)), Repeat::One),
        ];

        for ((context, track), expected) in table {
            let reaction = react(
                &Order::SetOptions {
                    shuffling: None,
                    repeating_context: context,
                    repeating_track: track,
                },
                &mut queue,
                &mut transport,
                0,
            );
            assert!(reaction.ok, "context {context:?}, track {track:?}");
            assert!(reaction.commands.is_empty(), "a mode loads nothing");
            assert_eq!(
                queue.repeat(),
                expected,
                "context {context:?}, track {track:?}: the track loop wins"
            );
        }
    }

    #[test]
    fn turning_context_repeat_off_leaves_track_repeat_alone() {
        let (mut queue, mut transport) = started(&["a", "b"], 0);
        queue.set_repeat(Repeat::One);

        // What `set_repeating_context(false)` decodes to: one half named.
        let reaction = react(
            &Order::SetOptions {
                shuffling: None,
                repeating_context: Some(false),
                repeating_track: None,
            },
            &mut queue,
            &mut transport,
            0,
        );

        assert!(reaction.ok);
        assert_eq!(
            queue.repeat(),
            Repeat::One,
            "turning the context loop off does not turn the track loop off too"
        );
    }

    #[test]
    fn turning_track_repeat_off_leaves_context_repeat_alone() {
        let (mut queue, mut transport) = started(&["a", "b"], 0);
        queue.set_repeat(Repeat::All);

        // What `set_repeating_track(false)` decodes to: the other half named.
        let reaction = react(
            &Order::SetOptions {
                shuffling: None,
                repeating_context: None,
                repeating_track: Some(false),
            },
            &mut queue,
            &mut transport,
            0,
        );

        assert!(reaction.ok);
        assert_eq!(
            queue.repeat(),
            Repeat::All,
            "turning the track loop off does not turn the context loop off too"
        );
    }

    #[test]
    fn set_options_applies_only_the_fields_present() {
        let (mut queue, mut transport) = started(&["a", "b", "c"], 0);
        queue.set_repeat(Repeat::All);

        // Only shuffle: the repeat mode the phone said nothing about stays.
        let reaction = react(
            &Order::SetOptions {
                shuffling: Some(true),
                repeating_context: None,
                repeating_track: None,
            },
            &mut queue,
            &mut transport,
            0,
        );
        assert!(reaction.ok);
        assert!(reaction.commands.is_empty());
        assert!(queue.shuffle());
        assert_eq!(queue.repeat(), Repeat::All, "an absent field is untouched");

        // Both repeat fields, the track loop last: it wins.
        let reaction = react(
            &Order::SetOptions {
                shuffling: None,
                repeating_context: Some(true),
                repeating_track: Some(true),
            },
            &mut queue,
            &mut transport,
            0,
        );
        assert!(reaction.ok);
        assert_eq!(queue.repeat(), Repeat::One);
        assert!(queue.shuffle(), "an absent shuffle is untouched too");

        // Clearing the context loop leaves the track loop alone.
        let reaction = react(
            &Order::SetOptions {
                shuffling: None,
                repeating_context: Some(false),
                repeating_track: None,
            },
            &mut queue,
            &mut transport,
            0,
        );
        assert!(reaction.ok);
        assert_eq!(queue.repeat(), Repeat::One);

        // Nothing named is nothing to apply, and it is still an answer.
        let reaction = react(
            &Order::SetOptions {
                shuffling: None,
                repeating_context: None,
                repeating_track: None,
            },
            &mut queue,
            &mut transport,
            0,
        );
        assert!(reaction.ok);
        assert!(queue.shuffle());
        assert_eq!(queue.repeat(), Repeat::One);
    }

    #[test]
    fn add_to_queue_inserts_after_the_current_row_and_after_earlier_adds() {
        let (mut queue, mut transport) = started(&["a", "b", "c"], 0);

        let reaction = react(
            &Order::AddToQueue(Some(window("x"))),
            &mut queue,
            &mut transport,
            0,
        );
        assert!(reaction.ok);
        assert!(
            reaction.commands.is_empty(),
            "a queued row waits for the current one to end"
        );

        let reaction = react(
            &Order::AddToQueue(Some(window("y"))),
            &mut queue,
            &mut transport,
            0,
        );
        assert!(reaction.ok);

        assert_eq!(
            uris(&queue),
            vec![
                "a".to_owned(),
                window("x"),
                window("y"),
                "b".to_owned(),
                "c".to_owned()
            ],
            "each add goes behind the earlier ones"
        );
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("a"),
            "the row that plays does not move"
        );
    }

    #[test]
    fn add_to_queue_without_a_track_fails() {
        let (mut queue, mut transport) = started(&["a", "b"], 0);
        let before = uris(&queue);

        let reaction = react(&Order::AddToQueue(None), &mut queue, &mut transport, 0);

        assert!(reaction.commands.is_empty());
        assert!(!reaction.ok, "there is nothing to add");
        assert_eq!(uris(&queue), before, "and no row is invented for it");
    }

    #[test]
    fn add_to_queue_with_an_unparsable_track_fails() {
        let (mut queue, mut transport) = started(&["a", "b"], 0);
        let before = uris(&queue);

        for uri in [
            "https://example.test/row",
            "spotify:",
            "spotify::x",
            "spotify:foo:bar",
            "spotify:track:",
        ] {
            let reaction = react(
                &Order::AddToQueue(Some(uri.to_owned())),
                &mut queue,
                &mut transport,
                0,
            );

            assert!(reaction.commands.is_empty(), "{uri}");
            assert!(!reaction.ok, "{uri} names no row to play");
            assert_eq!(uris(&queue), before, "{uri} invented a row");
        }
    }

    #[test]
    fn add_to_queue_with_nothing_playing_appends() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.append(vec![track("a"), track("b")]);
        assert!(queue.armed(), "the rows wait for a first play");

        let reaction = react(
            &Order::AddToQueue(Some(window("x"))),
            &mut queue,
            &mut transport,
            0,
        );

        assert!(reaction.ok);
        assert_eq!(
            uris(&queue),
            vec!["a".to_owned(), "b".to_owned(), window("x")],
            "an add with no current row joins the end"
        );
        assert!(queue.armed(), "the queue still waits for its first play");
    }

    #[test]
    fn set_queue_applies_the_window_and_is_ok() {
        let (mut queue, mut transport) = started(&["a", "b", "c"], 1);

        let reaction = react(
            &Order::SetQueue {
                prev: vec![window("p1"), window("p2")],
                next: vec![window("n1")],
                revision: "rev-2".to_owned(),
            },
            &mut queue,
            &mut transport,
            0,
        );

        assert!(reaction.ok);
        assert!(reaction.commands.is_empty(), "the window loads nothing");
        assert_eq!(
            uris(&queue),
            vec![window("p1"), window("p2"), "b".to_owned(), window("n1")],
            "the window sits around the row that is playing"
        );
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("b"));
        assert_eq!(queue.queue_revision(), "rev-2");
    }

    #[test]
    fn a_refused_window_fails_and_changes_nothing() {
        let (mut queue, mut transport) = started(&["a", "b"], 1);
        let before = uris(&queue);

        let reaction = react(
            &Order::SetQueue {
                prev: vec!["https://example.test/row".to_owned()],
                next: Vec::new(),
                revision: "rev-9".to_owned(),
            },
            &mut queue,
            &mut transport,
            0,
        );

        assert!(reaction.commands.is_empty());
        assert!(!reaction.ok, "a row that names nothing is not a window");
        assert_eq!(uris(&queue), before, "the rows are left as they were");
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("b"),
            "the row that plays does not move"
        );
        assert_eq!(queue.queue_revision(), "", "a refusal stores no revision");
    }
}
