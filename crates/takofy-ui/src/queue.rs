//! The play queue: what the window is stepping through, and when it advances.
//!
//! The window owns the queue and the index, because it is the thing the bar
//! draws. The worker never learns about it: a move is an ordinary `Play`.
//!
//! Every move funnels through `goto`, so the guard and the index cannot drift
//! from each other. The guard is set only by a `Play` and cleared by the load
//! events that follow it; a restart by `Seek` never sets it, because no load
//! ever follows.

use takofy_model::TrackEntry;

use crate::message::{Command, TransportUpdate, Update};
use crate::player::{RESTART_AFTER_MS, Transport, TransportState};

/// How many tracks an unplayable run may skip before the queue stops and shows
/// the failure. A free account fails every track, and an uncapped run would
/// fire a load per entry.
pub const MAX_CONSECUTIVE_SKIPS: u32 = 5;

/// The queue and the index it is on.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    tracks: Vec<TrackEntry>,
    index: usize,
    /// Set when a `Play` is in flight; `Ended`/`Unavailable` before the load's
    /// first event are the previous track's and must be ignored.
    awaiting_start: bool,
    /// Consecutive auto-advances from `Unavailable`. A user move or a playing
    /// position clears it.
    skips: u32,
}

impl Queue {
    /// Replaces the queue with `tracks` and starts on `index`.
    pub fn start(
        &mut self,
        tracks: &[TrackEntry],
        index: usize,
        transport: &mut Transport,
    ) -> Option<Command> {
        if index >= tracks.len() {
            return None;
        }
        self.tracks = tracks.to_vec();
        self.skips = 0;
        self.goto(index, transport)
    }

    /// Steps forward one entry, if there is one.
    pub fn next(&mut self, transport: &mut Transport) -> Option<Command> {
        if !self.can_next() {
            return None;
        }
        self.skips = 0;
        self.goto(self.index + 1, transport)
    }

    /// Restarts the current entry when it is past the threshold or is the
    /// first, otherwise steps back one.
    pub fn previous(&mut self, position_ms: u32, transport: &mut Transport) -> Option<Command> {
        if self.tracks.is_empty() {
            return None;
        }
        self.skips = 0;
        if position_ms > RESTART_AFTER_MS || self.index == 0 {
            if transport.seekable() {
                // A restart is a `Seek`, and no load follows it, so the guard
                // stays clear.
                Some(Command::Seek { position_ms: 0 })
            } else {
                self.goto(self.index, transport)
            }
        } else {
            self.goto(self.index - 1, transport)
        }
    }

    /// Pauses, resumes, or replays the current entry, following the state the
    /// player reported.
    pub fn toggle(&mut self, transport: &mut Transport) -> Option<Command> {
        if self.tracks.is_empty() {
            return None;
        }
        match transport.state {
            TransportState::Playing | TransportState::Loading => Some(Command::Pause),
            TransportState::Paused => Some(Command::Resume),
            TransportState::Idle
            | TransportState::Stopped
            | TransportState::Unavailable
            | TransportState::Failed(_) => {
                self.skips = 0;
                self.goto(self.index, transport)
            }
        }
    }

    /// Folds one update in and returns the command it implies, if any.
    pub fn apply(&mut self, update: &Update, transport: &mut Transport) -> Option<Command> {
        let Update::Transport(update) = update else {
            return None;
        };
        match update {
            TransportUpdate::Loading | TransportUpdate::Duration { .. } => {
                self.awaiting_start = false;
                None
            }
            TransportUpdate::Position { paused, .. } => {
                self.awaiting_start = false;
                if !paused {
                    self.skips = 0;
                }
                None
            }
            TransportUpdate::Failed(_) => {
                self.awaiting_start = false;
                None
            }
            TransportUpdate::Ended => {
                if self.awaiting_start {
                    return None;
                }
                self.goto_next(transport)
            }
            TransportUpdate::Unavailable => {
                if self.awaiting_start {
                    return None;
                }
                self.skips = self.skips.saturating_add(1);
                if self.skips > MAX_CONSECUTIVE_SKIPS {
                    return None;
                }
                self.goto_next(transport)
            }
            TransportUpdate::Seeked { .. } | TransportUpdate::Stopped => None,
        }
    }

    /// Whether there is an entry after the current one.
    pub fn can_next(&self) -> bool {
        self.index + 1 < self.tracks.len()
    }

    /// Whether the queue has anything to step through.
    pub fn can_previous(&self) -> bool {
        !self.tracks.is_empty()
    }

    /// Every entry after the current one, with its absolute index in the queue.
    ///
    /// The index is the position in the list `start` was given, so it stays the
    /// row's identity across advances: advancing only moves the current index.
    pub fn upcoming(&self) -> impl ExactSizeIterator<Item = (usize, &TrackEntry)> + '_ {
        let first = self.index + 1;
        self.tracks.iter().enumerate().skip(first)
    }

    /// Plays the entry at `index`, leaving every other entry in place.
    ///
    /// An index past the end is refused and changes nothing at all: a click on a
    /// row that has since gone must not move the queue or disarm an in-flight
    /// load's guard.
    pub fn jump(&mut self, index: usize, transport: &mut Transport) -> Option<Command> {
        if index >= self.tracks.len() {
            return None;
        }
        self.skips = 0;
        self.goto(index, transport)
    }

    /// Whether there is an entry to pause, resume or replay.
    pub fn can_toggle(&self) -> bool {
        !self.tracks.is_empty()
    }

    fn goto_next(&mut self, transport: &mut Transport) -> Option<Command> {
        let next = self.index + 1;
        self.goto(next, transport)
    }

    /// Moves the index and asks the transport to play that entry.
    fn goto(&mut self, index: usize, transport: &mut Transport) -> Option<Command> {
        let (uri, title, artists, cover) = {
            let entry = self.tracks.get(index)?;
            (
                entry.uri.clone(),
                entry.title.clone(),
                entry.artists.clone(),
                entry.cover.clone(),
            )
        };
        self.index = index;
        self.awaiting_start = true;
        Some(transport.play(&uri, &title, &artists, cover))
    }
}

#[cfg(test)]
mod tests {
    use takofy_model::TrackEntry;

    use super::*;
    use crate::message::Failure;

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn track_by(uri: &str, artists: &str) -> TrackEntry {
        TrackEntry {
            artists: artists.to_owned(),
            ..track(uri)
        }
    }

    /// The upcoming entries as `(index, uri)` pairs, which is what every
    /// `upcoming` test asserts on.
    fn upcoming(queue: &Queue) -> Vec<(usize, String)> {
        queue
            .upcoming()
            .map(|(index, entry)| (index, entry.uri.clone()))
            .collect()
    }

    fn tracks(count: usize) -> Vec<TrackEntry> {
        (0..count).map(|i| track(&format!("t{i}"))).collect()
    }

    fn play(uri: &str) -> Command {
        Command::Play {
            uri: uri.to_owned(),
        }
    }

    fn transport_update(update: TransportUpdate) -> Update {
        Update::Transport(update)
    }

    /// The pipeline `drain` runs: the transport folds first, then the queue.
    fn feed(
        queue: &mut Queue,
        transport: &mut Transport,
        update: TransportUpdate,
    ) -> Option<Command> {
        let update = transport_update(update);
        transport.apply(&update);
        queue.apply(&update, transport)
    }

    /// The events a real load emits after a `Play`, with a failure at the end.
    fn skip(queue: &mut Queue, transport: &mut Transport) -> Option<Command> {
        feed(queue, transport, TransportUpdate::Loading);
        feed(queue, transport, TransportUpdate::Unavailable)
    }

    fn clear_guard(queue: &mut Queue, transport: &mut Transport) {
        feed(queue, transport, TransportUpdate::Loading);
    }

    fn end(queue: &mut Queue, transport: &mut Transport) -> Option<Command> {
        feed(queue, transport, TransportUpdate::Ended)
    }

    fn seekable(queue: &mut Queue, transport: &mut Transport, position_ms: u32) {
        feed(
            queue,
            transport,
            TransportUpdate::Duration {
                duration_ms: 222_000,
            },
        );
        feed(
            queue,
            transport,
            TransportUpdate::Position {
                position_ms,
                paused: false,
            },
        );
    }

    #[test]
    fn ended_advances() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    }

    #[test]
    fn ended_at_the_last_entry_is_idle_with_no_command() {
        let tracks = tracks(1);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), None);
        assert!(!queue.can_next());
    }

    #[test]
    fn unavailable_advances_and_the_chain_stops_at_the_cap() {
        let tracks = tracks(12);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        for i in 1..=MAX_CONSECUTIVE_SKIPS {
            assert_eq!(
                skip(&mut queue, &mut transport),
                Some(play(&format!("t{i}"))),
                "skip {i}"
            );
        }
        assert_eq!(
            skip(&mut queue, &mut transport),
            None,
            "the run past the cap must stop"
        );
    }

    #[test]
    fn a_non_paused_position_resets_the_skip_count() {
        let tracks = tracks(16);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        for _ in 0..3 {
            skip(&mut queue, &mut transport);
        }
        feed(
            &mut queue,
            &mut transport,
            TransportUpdate::Position {
                position_ms: 1_000,
                paused: false,
            },
        );

        for i in 4..=8 {
            assert_eq!(
                skip(&mut queue, &mut transport),
                Some(play(&format!("t{i}"))),
                "a playing position must reset the streak"
            );
        }
        assert_eq!(skip(&mut queue, &mut transport), None);
    }

    #[test]
    fn a_stale_ended_before_loading_is_ignored() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "an end before the new load's first event is the old track's"
        );
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    }

    #[test]
    fn failed_clears_the_guard_and_never_advances() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        let failed = transport_update(TransportUpdate::Failed(Failure::Other));
        assert_eq!(queue.apply(&failed, &mut transport), None);
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t1")),
            "the failure must have cleared the guard"
        );
    }

    #[test]
    fn previous_goes_back_within_the_threshold() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));

        assert_eq!(queue.previous(4_000, &mut transport), Some(play("t0")));
    }

    #[test]
    fn previous_restarts_past_the_threshold() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 1, &mut transport), Some(play("t1")));
        seekable(&mut queue, &mut transport, 4_001);

        assert_eq!(
            queue.previous(4_001, &mut transport),
            Some(Command::Seek { position_ms: 0 })
        );
    }

    #[test]
    fn previous_on_the_first_entry_restarts() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));
        seekable(&mut queue, &mut transport, 1_000);

        assert_eq!(
            queue.previous(1_000, &mut transport),
            Some(Command::Seek { position_ms: 0 })
        );
    }

    #[test]
    fn previous_replays_the_first_entry_when_not_seekable() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));
        clear_guard(&mut queue, &mut transport);

        assert_eq!(queue.previous(1_000, &mut transport), Some(play("t0")));
    }

    #[test]
    fn a_seek_restart_leaves_the_guard_unset() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));
        seekable(&mut queue, &mut transport, 4_001);

        assert_eq!(
            queue.previous(4_001, &mut transport),
            Some(Command::Seek { position_ms: 0 })
        );
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t1")),
            "a restart emits no load, so no guard may be left behind"
        );
    }

    #[test]
    fn a_position_clears_the_guard() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        feed(
            &mut queue,
            &mut transport,
            TransportUpdate::Position {
                position_ms: 1,
                paused: false,
            },
        );
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t1")),
            "the load that emits no other event must clear the guard too"
        );
    }

    #[test]
    fn a_second_loading_does_not_count_as_a_second_load() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        clear_guard(&mut queue, &mut transport);
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
        assert_eq!(end(&mut queue, &mut transport), None, "exactly one advance");
    }

    #[test]
    fn next_at_the_end_is_none() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 1, &mut transport), Some(play("t1")));

        assert!(!queue.can_next());
        assert_eq!(queue.next(&mut transport), None);
        assert_eq!(
            queue.next(&mut transport),
            None,
            "a refused next must not move the index"
        );
    }

    #[test]
    fn a_manual_next_after_a_cap_stop_gets_a_full_fresh_streak() {
        let tracks = tracks(16);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 0, &mut transport), Some(play("t0")));

        for _ in 0..=MAX_CONSECUTIVE_SKIPS {
            skip(&mut queue, &mut transport);
        }
        assert_eq!(skip(&mut queue, &mut transport), None, "capped");

        assert_eq!(queue.next(&mut transport), Some(play("t6")));
        for i in 7..=11 {
            assert_eq!(
                skip(&mut queue, &mut transport),
                Some(play(&format!("t{i}"))),
                "the user's next must have reset the streak"
            );
        }
        assert_eq!(skip(&mut queue, &mut transport), None);
    }

    #[test]
    fn toggle_follows_the_reported_state() {
        let cases = [
            (TransportState::Idle, Some(play("t0"))),
            (TransportState::Loading, Some(Command::Pause)),
            (TransportState::Playing, Some(Command::Pause)),
            (TransportState::Paused, Some(Command::Resume)),
            (TransportState::Unavailable, Some(play("t0"))),
            (TransportState::Stopped, Some(play("t0"))),
            (TransportState::Failed(Failure::Other), Some(play("t0"))),
        ];
        for (state, expected) in cases {
            let tracks = tracks(2);
            let mut queue = Queue::default();
            let mut transport = Transport::default();
            queue.start(&tracks, 0, &mut transport);
            transport.state = state;
            assert_eq!(queue.toggle(&mut transport), expected, "{state:?}");
        }
    }

    #[test]
    fn replacing_the_queue_mid_play_leaves_no_stale_guard() {
        let first = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&first, 0, &mut transport), Some(play("t0")));
        clear_guard(&mut queue, &mut transport);

        let second = vec![track("x"), track("y")];
        assert_eq!(queue.start(&second, 0, &mut transport), Some(play("x")));
        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "the old track's end must not advance the new queue"
        );
    }

    #[test]
    fn the_queue_is_unaffected_by_the_library_changing() {
        use takofy_model::PlaylistEntry;

        let all = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&all, 0, &mut transport), Some(play("t0")));
        clear_guard(&mut queue, &mut transport);

        let playlists = Update::Playlists(vec![PlaylistEntry {
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
        }]);
        assert_eq!(queue.apply(&playlists, &mut transport), None);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    }

    #[test]
    fn duplicate_adjacent_uris_behave_like_any_other_pair() {
        let duplicates = vec![track("same"), track("same")];
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&duplicates, 0, &mut transport),
            Some(play("same"))
        );
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("same")));

        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), None);
    }

    #[test]
    fn start_with_an_out_of_range_index_is_none() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.start(&tracks, 2, &mut transport), None);
        assert_eq!(queue.start(&[], 0, &mut transport), None);
        assert!(!queue.can_toggle());
    }

    #[test]
    fn an_empty_queue_has_nothing_to_step_through() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert!(!queue.can_next());
        assert!(!queue.can_previous());
        assert!(!queue.can_toggle());
        assert_eq!(queue.next(&mut transport), None);
        assert_eq!(queue.previous(1_000, &mut transport), None);
        assert_eq!(queue.toggle(&mut transport), None);
    }

    #[test]
    fn a_populated_queue_can_step_either_way() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);
        assert!(queue.can_next());
        assert!(queue.can_previous());
        assert!(queue.can_toggle());
    }

    #[test]
    fn start_over_an_episode_entry_names_it_in_now_playing() {
        let episode = takofy_model::Episode {
            uri: "spotify:episode:abc".to_owned(),
            title: "210: A dry labyrinth".to_owned(),
            show: "Lateral with Tom Scott".to_owned(),
            show_uri: Some("spotify:show:lateral".to_owned()),
            description: None,
            published: None,
            duration_ms: Some(2_773_461),
            cover: None,
            named: true,
        };
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let command = queue.start(&[episode.entry()], 0, &mut transport);

        assert_eq!(
            command,
            Some(crate::message::Command::Play {
                uri: "spotify:episode:abc".to_owned()
            })
        );
        let now = transport.now_playing.expect("the episode is playing");
        assert_eq!(now.title, "210: A dry labyrinth");
        assert_eq!(now.artists, "Lateral with Tom Scott");
    }

    #[test]
    fn goto_passes_the_entrys_cover_into_now_playing() {
        let cover =
            takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        let mut entry = track("a");
        entry.cover = Some(cover.clone());
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        let _ = queue.start(&[entry], 0, &mut transport);

        assert_eq!(
            transport.now_playing.expect("a named track").cover,
            Some(cover)
        );
    }

    #[test]
    fn goto_passes_the_entrys_artists_into_now_playing() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        let _ = queue.start(&[track_by("a", "A Band")], 0, &mut transport);

        assert_eq!(
            transport.now_playing.expect("a named track").artists,
            "A Band"
        );
    }

    #[test]
    fn an_empty_queue_has_nothing_upcoming() {
        let queue = Queue::default();
        assert_eq!(upcoming(&queue), Vec::new());
        assert_eq!(queue.upcoming().len(), 0);
    }

    #[test]
    fn the_last_entry_has_nothing_upcoming() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 2, &mut transport);

        assert_eq!(upcoming(&queue), Vec::new());
        assert!(!queue.can_next(), "nothing upcoming is exactly `!can_next`");
    }

    #[test]
    fn upcoming_lists_what_follows_the_current_entry_with_absolute_indices() {
        let tracks = tracks(5);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 1, &mut transport);

        assert_eq!(
            upcoming(&queue),
            vec![
                (2, "t2".to_owned()),
                (3, "t3".to_owned()),
                (4, "t4".to_owned()),
            ],
            "the current entry is never listed, and the indices are absolute"
        );
    }

    #[test]
    fn upcoming_shrinks_as_the_queue_advances() {
        let tracks = tracks(4);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(queue.upcoming().len(), 3);
        for (step, remaining) in [(1, 2), (2, 1), (3, 0)] {
            assert_eq!(queue.next(&mut transport), Some(play(&format!("t{step}"))));
            assert_eq!(
                queue.upcoming().len(),
                remaining,
                "after advancing to t{step}"
            );
        }
    }

    #[test]
    fn upcoming_reports_an_exact_length() {
        let tracks = tracks(6);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 2, &mut transport);

        let mut iter = queue.upcoming();
        assert_eq!(iter.len(), 3);
        let _ = iter.next();
        assert_eq!(iter.len(), 2, "`ExactSizeIterator` counts what is left");
        let _ = iter.next();
        let _ = iter.next();
        assert_eq!(iter.len(), 0);
        assert!(iter.next().is_none());
    }

    #[test]
    fn duplicate_entries_get_distinct_upcoming_indices() {
        let duplicates = vec![track("same"), track("same"), track("same")];
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&duplicates, 0, &mut transport);

        assert_eq!(
            upcoming(&queue),
            vec![(1, "same".to_owned()), (2, "same".to_owned())],
            "identical uris are two entries, not one"
        );
    }

    #[test]
    fn jump_plays_that_entry_and_keeps_the_rest_of_the_list() {
        let tracks = tracks(4);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t2")
        );
        assert_eq!(
            upcoming(&queue),
            vec![(3, "t3".to_owned())],
            "the entries after the target stay reachable"
        );
        assert_eq!(
            queue.previous(1_000, &mut transport),
            Some(play("t1")),
            "the entries before the target stay reachable"
        );
    }

    #[test]
    fn jump_sets_the_guard_so_a_stale_end_is_ignored() {
        // The guard covers an event arriving before *any* event of the new load,
        // not only its first: `Loading`, `Duration`, `Position` and `Failed` all
        // clear it, so after one of them a trailing event is counted as the new
        // load's own.
        let tracks = tracks(4);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);
        clear_guard(&mut queue, &mut transport);

        assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "an end arriving before any event of the new load is the old track's"
        );
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t3")),
            "a later end advances from the jumped-to entry"
        );
    }

    #[test]
    fn a_jump_during_a_failure_run_is_carried_past_its_own_entry() {
        // An accepted race, shared with `next`: once the clicked entry's
        // `Loading` has cleared the guard, a trailing `Unavailable` from the
        // entry the run left behind is indistinguishable from the clicked
        // entry's own, so the queue advances once more and skips the click.
        let tracks = tracks(8);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        // One failed load has already carried the queue to t1.
        assert_eq!(skip(&mut queue, &mut transport), Some(play("t1")));

        // The click lands while that failure's load is still in flight.
        assert_eq!(queue.jump(4, &mut transport), Some(play("t4")));
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            Some(play("t5")),
            "the old load's trailing failure skips the clicked entry"
        );
        assert_eq!(
            upcoming(&queue),
            vec![(6, "t6".to_owned()), (7, "t7".to_owned())],
            "the queue sits one past the entry the click picked"
        );
    }

    #[test]
    fn jump_resets_the_skip_streak() {
        let tracks = tracks(16);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        for _ in 0..=MAX_CONSECUTIVE_SKIPS {
            skip(&mut queue, &mut transport);
        }
        assert_eq!(skip(&mut queue, &mut transport), None, "capped");

        assert_eq!(queue.jump(7, &mut transport), Some(play("t7")));
        for i in 8..=12 {
            assert_eq!(
                skip(&mut queue, &mut transport),
                Some(play(&format!("t{i}"))),
                "the jump must have reset the streak"
            );
        }
        assert_eq!(skip(&mut queue, &mut transport), None);
    }

    #[test]
    fn an_out_of_range_jump_changes_nothing_at_all() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 1, &mut transport);
        // A load is in flight: the guard must survive a refused click.
        let before = transport.clone();

        for index in [3, 4, usize::MAX] {
            assert_eq!(queue.jump(index, &mut transport), None, "{index}");
        }

        assert_eq!(
            transport, before,
            "a refused jump must not touch the transport"
        );
        assert_eq!(
            queue.previous(1_000, &mut transport),
            Some(play("t0")),
            "the index never moved"
        );
    }

    #[test]
    fn an_out_of_range_jump_leaves_the_guard_armed() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(queue.jump(9, &mut transport), None);
        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "a refused click must not disarm an in-flight load"
        );
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    }

    #[test]
    fn jump_to_the_last_entry_leaves_nothing_upcoming() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
        assert!(queue.upcoming().next().is_none());
        assert!(!queue.can_next());
    }

    #[test]
    fn jump_back_to_the_first_entry_still_restarts_it() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 2, &mut transport);

        assert_eq!(queue.jump(0, &mut transport), Some(play("t0")));
        seekable(&mut queue, &mut transport, 1_000);
        assert_eq!(
            queue.previous(1_000, &mut transport),
            Some(Command::Seek { position_ms: 0 }),
            "the first entry has nothing before it, so Previous restarts it"
        );
    }

    #[test]
    fn previous_after_a_jump_steps_to_the_entry_before_the_target() {
        let tracks = tracks(5);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(queue.jump(3, &mut transport), Some(play("t3")));
        // A position inside the restart threshold is a step back, not a restart.
        assert_eq!(
            queue.previous(RESTART_AFTER_MS, &mut transport),
            Some(play("t2")),
            "the threshold itself is a step back"
        );
    }

    #[test]
    fn jump_to_the_current_index_replays_it() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 1, &mut transport);
        clear_guard(&mut queue, &mut transport);

        assert_eq!(queue.jump(1, &mut transport), Some(play("t1")));
        assert_eq!(
            upcoming(&queue),
            vec![(2, "t2".to_owned())],
            "replaying leaves what follows it alone"
        );
    }

    #[test]
    fn jump_passes_the_entrys_artists_into_now_playing() {
        let tracks = vec![track_by("a", "A Band"), track_by("b", "B Band")];
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(queue.jump(1, &mut transport), Some(play("b")));
        assert_eq!(
            transport.now_playing.expect("a named track").artists,
            "B Band"
        );
    }
}
