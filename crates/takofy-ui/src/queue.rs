//! The play queue: what the window is stepping through, and when it advances.
//!
//! The window owns the queue and the index, because it is the thing the bar
//! draws. The worker never learns about it: a move is an ordinary `Play`.
//!
//! Every move funnels through `goto`, so the guard and the index cannot drift
//! from each other. The guard is set only by a `Play` and cleared by the load
//! events that follow it; a restart by `Seek` never sets it, because no load
//! ever follows.
//!
//! Where a finished track goes is the repeat mode's business, and the Next
//! button asks the same table, so the end of a track and a click cannot disagree
//! about what follows. A loop plays nothing new, so it counts against the skip
//! cap exactly as an unavailable row does.
//!
//! Shuffle permutes the rows that are coming and remembers the order it found
//! them in, so turning it off puts the user's list back and re-seats `index` on
//! the entry it was already on. Rows before the current one are history and
//! never move.
//!
//! The queue also edits its own order and keeps the selection over its rows,
//! because both are indices into it. One rule holds edits apart from the
//! playing entry: an edit never touches `index`, `awaiting_start` or `skips`; a
//! mode switch may re-seat `index` but never changes which entry is current.
//! The rows an edit may move all sit after the current entry, so the playing
//! entry keeps its place and a load in flight keeps its guard.

use std::collections::HashMap;
use std::ops::Range;

use takofy_model::TrackEntry;

use crate::message::{Command, TransportUpdate, Update};
use crate::player::{RESTART_AFTER_MS, Transport, TransportState};
use crate::reorder;
use crate::repeat::{self, Cause, Hop, Repeat};
use crate::selection::{Press, Selection};
use crate::shuffle;

/// How many tracks an unplayable run may skip before the queue stops and shows
/// the failure. A free account fails every track, and an uncapped run would
/// fire a load per entry.
pub const MAX_CONSECUTIVE_SKIPS: u32 = 5;

/// The direction a block of rows steps in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

/// The queue and the index it is on, when it is on one.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    tracks: Vec<TrackEntry>,
    /// The entry that is playing, or `None` while the queue is armed: it holds
    /// rows and nothing is current, so the first play starts the first row.
    index: Option<usize>,
    /// Set when a `Play` is in flight; `Ended`/`Unavailable` before the load's
    /// first event are the previous track's and must be ignored.
    awaiting_start: bool,
    /// Consecutive auto-advances that played nothing new: an unavailable row or
    /// a repeat loop. A user move, a playing position or a known duration clears
    /// it.
    skips: u32,
    /// The upcoming rows the pointer has picked.
    selection: Selection,
    /// The row the keyboard hands focus to once this frame's rows are drawn.
    refocus: Option<usize>,
    /// Where a finished track goes.
    repeat: Repeat,
    /// Whether the coming rows are in a drawn order rather than the list's.
    shuffle: bool,
    /// Every row's uri in the order the list had when shuffle turned on.
    snapshot: Vec<String>,
    /// The generator's state, advanced once per draw.
    rng: u64,
}

impl Queue {
    /// A queue whose shuffle draws come from `seed`.
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: seed,
            ..Self::default()
        }
    }

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
        self.selection.clear();
        self.refocus = None;
        self.goto(index, transport)
    }

    /// Steps forward one entry, if there is one.
    pub fn next(&mut self, transport: &mut Transport) -> Option<Command> {
        let hop = self.hop(Cause::Manual)?;
        self.skips = 0;
        self.follow(hop, transport)
    }

    /// Restarts the current entry when it is past the threshold or is the
    /// first, otherwise steps back one.
    pub fn previous(&mut self, position_ms: u32, transport: &mut Transport) -> Option<Command> {
        let index = self.index?;
        self.skips = 0;
        if position_ms > RESTART_AFTER_MS || index == 0 {
            if transport.seekable() {
                // A restart is a `Seek`, and no load follows it, so the guard
                // stays clear.
                Some(Command::Seek { position_ms: 0 })
            } else {
                self.goto(index, transport)
            }
        } else {
            self.goto(index - 1, transport)
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
                // An armed queue has no current entry to replay, so the first
                // play starts the first row.
                self.goto(self.index.unwrap_or(0), transport)
            }
        }
    }

    /// Folds one update in and returns the command it implies, if any.
    pub fn apply(&mut self, update: &Update, transport: &mut Transport) -> Option<Command> {
        let Update::Transport(update) = update else {
            return None;
        };
        match update {
            TransportUpdate::Loading => {
                self.awaiting_start = false;
                None
            }
            // A duration proves the track loaded, so it ends a run of advances
            // that never did.
            TransportUpdate::Duration { .. } => {
                self.awaiting_start = false;
                self.skips = 0;
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
                self.move_on(Cause::Ended, transport)
            }
            TransportUpdate::Unavailable => {
                if self.awaiting_start {
                    return None;
                }
                self.move_on(Cause::Unavailable, transport)
            }
            TransportUpdate::Seeked { .. } | TransportUpdate::Stopped => None,
        }
    }

    /// Whether Next would move: the same answer `next` gives, without playing.
    pub fn can_next(&self) -> bool {
        self.hop(Cause::Manual).is_some()
    }

    /// Whether the queue has a current entry, and so something to step back
    /// from.
    pub fn can_previous(&self) -> bool {
        self.index.is_some()
    }

    /// Whether the queue holds rows with nothing current: a drop brought it
    /// entries and no play has started them yet.
    pub fn armed(&self) -> bool {
        self.index.is_none() && !self.tracks.is_empty()
    }

    /// The entry that is playing, if one is.
    pub fn current(&self) -> Option<&TrackEntry> {
        self.index.and_then(|index| self.tracks.get(index))
    }

    /// Every entry after the current one, with its absolute index in the queue.
    ///
    /// The index is the position in the list `start` was given, so it stays the
    /// row's identity across advances: advancing only moves the current index.
    pub fn upcoming(&self) -> impl ExactSizeIterator<Item = (usize, &TrackEntry)> + '_ {
        let first = self.first_upcoming();
        self.tracks.iter().enumerate().skip(first)
    }

    /// The first index an edit may touch: the entry after the current one, or
    /// the first row while the queue is armed.
    pub fn first_upcoming(&self) -> usize {
        self.index.map_or(0, |index| index + 1)
    }

    /// Whether row `row` is picked. Only the upcoming rows can be.
    pub fn selected(&self, row: usize) -> bool {
        row >= self.first_upcoming() && self.selection.contains(row)
    }

    /// Applies a press to `row`, ignoring the rows the queue will not edit.
    pub fn press(&mut self, row: usize, press: Press) {
        if self.editable(row) {
            self.selection.press(row, press);
        }
    }

    /// Picks `row` for a carry, and returns the rows that travel with it.
    pub fn lift(&mut self, row: usize) -> Option<Vec<usize>> {
        if !self.editable(row) {
            return None;
        }
        self.selection.lift(row);
        Some(self.selected_rows())
    }

    /// The rows a command opened on `row` acts on: the whole pick when `row` is
    /// in it, otherwise `row` alone. The rows a queue will not edit act on
    /// nothing.
    ///
    /// The pick may hold rows the queue has since passed, so the cut is filtered
    /// to the editable region: a stale row never travels into a removal that
    /// would then be refused whole.
    pub fn act_on(&mut self, row: usize) -> Vec<usize> {
        if !self.editable(row) {
            return Vec::new();
        }
        let rows = self.selection.act_on(row);
        self.editable_rows(&rows)
    }

    /// Takes `rows` out of the queue, and says how many went.
    ///
    /// The whole removal is refused when any row is not editable or no longer
    /// holds the entries the command was opened on, so a stale menu changes
    /// nothing. The current entry, the load's guard and the skip count stay as
    /// they were.
    pub fn remove_rows(&mut self, rows: &[usize], expect: &[TrackEntry]) -> Option<usize> {
        if rows.is_empty() || rows.iter().any(|row| !self.editable(*row)) {
            return None;
        }
        if self.entries_at(rows) != expect {
            return None;
        }

        let mut rows = rows.to_vec();
        rows.sort_unstable();
        rows.dedup();
        for row in rows.iter().rev() {
            self.tracks.remove(*row);
        }
        self.selection.without(&rows);
        let first = rows[0];
        self.refocus = (first < self.tracks.len()).then_some(first);
        Some(rows.len())
    }

    /// The entries at `rows`, skipping any that have gone.
    pub fn entries_at(&self, rows: &[usize]) -> Vec<TrackEntry> {
        rows.iter()
            .filter_map(|row| self.tracks.get(*row))
            .cloned()
            .collect()
    }

    /// Moves `rows` to the entry now at `boundary`, and says where they landed.
    ///
    /// A contiguous block dropped on itself, or just beside itself, is not a
    /// move: it returns `None` and changes nothing.
    pub fn move_rows(&mut self, rows: &[usize], boundary: usize) -> Option<Range<usize>> {
        let boundary = self.edit_boundary(boundary);
        let rows = self.editable_rows(rows);
        let found = reorder::gather(self.tracks.len(), &rows, boundary)?;
        let moved = self.entries_at(&found.rows);
        for row in found.rows.iter().rev() {
            self.tracks.remove(*row);
        }
        self.tracks.splice(found.landed..found.landed, moved);

        let range = found.landed..found.landed + found.rows.len();
        self.selection.set(range.clone());
        Some(range)
    }

    /// Inserts `entries` before the entry now at `boundary`, and says where they
    /// landed. The new rows arrive unpicked.
    pub fn insert_at(&mut self, boundary: usize, entries: Vec<TrackEntry>) -> Option<Range<usize>> {
        if entries.is_empty() {
            return None;
        }
        let boundary = self.edit_boundary(boundary);
        let count = entries.len();

        self.tracks.splice(boundary..boundary, entries);
        self.selection.open_gap(boundary, count);

        Some(boundary..boundary + count)
    }

    /// Adds `entries` after the last entry, unpicked, and says where they landed.
    /// An add has no position to choose, so an empty queue takes them at the
    /// first row and is left armed.
    pub fn append(&mut self, entries: Vec<TrackEntry>) -> Option<Range<usize>> {
        self.insert_at(self.tracks.len(), entries)
    }

    /// Bubbles `rows` one row towards `step`, and reports each `(from, to)`.
    ///
    /// A row held back by a picked row that could not move stays where it is.
    /// The picked rows follow the entries they moved with.
    pub fn step_rows(&mut self, rows: &[usize], step: Step) -> Option<Vec<(usize, usize)>> {
        let mut rows = self.editable_rows(rows);
        rows.sort_unstable();
        rows.dedup();
        if rows.is_empty() {
            return None;
        }

        let first = self.first_upcoming();
        let last = self.tracks.len() - 1;
        let mut moved = Vec::new();
        match step {
            Step::Up => {
                let mut above: Option<(usize, bool)> = None;
                for row in rows.iter().copied() {
                    let stays = row == first
                        || matches!(above, Some((previous, false)) if previous + 1 == row);
                    if !stays {
                        moved.push((row, row - 1));
                    }
                    above = Some((row, !stays));
                }
                // Ascending, so each row finds the gap the one above it left.
                for (from, to) in &moved {
                    self.tracks.swap(*to, *from);
                }
            }
            Step::Down => {
                let mut below: Option<(usize, bool)> = None;
                for row in rows.iter().rev().copied() {
                    let stays = row == last
                        || matches!(below, Some((previous, false)) if previous == row + 1);
                    if !stays {
                        moved.push((row, row + 1));
                    }
                    below = Some((row, !stays));
                }
                // Descending, so each row finds the gap the one below it left.
                for (from, to) in &moved {
                    self.tracks.swap(*from, *to);
                }
            }
        }

        if moved.is_empty() {
            return None;
        }
        moved.sort_unstable();
        let landed: HashMap<usize, usize> = moved.iter().copied().collect();
        // A row held at the edge keeps its pick; only the moved ones change row.
        self.selection.set(
            rows.iter()
                .map(|row| landed.get(row).copied().unwrap_or(*row)),
        );
        Some(moved)
    }

    /// The rows an Alt+↑/↓ step acts on: the picked rows, else the focused one.
    pub fn acting_rows(&self, focused: Option<usize>) -> Vec<usize> {
        let picked = self.selected_rows();
        if !picked.is_empty() {
            return picked;
        }
        focused
            .filter(|row| self.editable(*row))
            .into_iter()
            .collect()
    }

    /// The row the keyboard should hand focus to once the frame knows it.
    pub fn refocus(&self) -> Option<usize> {
        self.refocus
    }

    /// Asks for `row` to take the keyboard on the frame that draws it.
    pub fn refocus_at(&mut self, row: usize) {
        self.refocus = Some(row);
    }

    /// Takes the pending refocus, after this frame's rows have been drawn.
    pub fn settle_refocus(&mut self) {
        self.refocus = None;
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

    /// The repeat mode.
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    /// Steps to the repeat mode after this one.
    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
    }

    /// Whether shuffle is on.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Turns shuffle on or off.
    ///
    /// On, the coming rows are permuted and the list's order is remembered. Off,
    /// that order comes back and `index` is re-seated on the entry it was already
    /// on. Either way the pick is cleared: it names rows, and the rows now hold
    /// other tracks.
    pub fn toggle_shuffle(&mut self) {
        if self.shuffle {
            self.shuffle_off();
        } else {
            self.shuffle_on();
        }
        self.selection.clear();
        self.refocus = None;
    }

    /// Where the queue goes for `cause`.
    fn hop(&self, cause: Cause) -> Option<Hop> {
        repeat::advance(self.repeat, self.index, self.tracks.len(), cause)
    }

    /// Permutes the coming rows and remembers the order they had.
    fn shuffle_on(&mut self) {
        self.snapshot = self.uris();
        let first = self.first_upcoming();
        let seed = shuffle::splitmix(&mut self.rng);
        let order = shuffle::order(self.tracks.len() - first, seed);
        self.reorder_tail(first, &order);
        self.shuffle = true;
    }

    /// Puts the list back in the order it had, with `index` on the same entry.
    fn shuffle_off(&mut self) {
        let order = shuffle::restore(&self.snapshot, &self.uris());
        self.reorder_tail(0, &order);
        self.snapshot.clear();
        self.shuffle = false;
    }

    /// Every row's uri, in the order the list has them.
    fn uris(&self) -> Vec<String> {
        self.tracks.iter().map(|entry| entry.uri.clone()).collect()
    }

    /// Rebuilds the rows from `from` on in `order`, a permutation of their
    /// indices, carrying `index` onto the row it was on.
    fn reorder_tail(&mut self, from: usize, order: &[usize]) {
        let current = self.index.filter(|index| *index >= from);
        let mut slots: Vec<Option<TrackEntry>> =
            self.tracks.split_off(from).into_iter().map(Some).collect();
        let mut index = None;
        for old in order {
            let Some(entry) = slots.get_mut(*old).and_then(Option::take) else {
                continue;
            };
            if current == Some(from + *old) {
                index = Some(self.tracks.len());
            }
            self.tracks.push(entry);
        }
        if current.is_some() {
            self.index = index;
        }
    }

    /// Steps the queue for `cause`, counting a lap that played nothing.
    ///
    /// A failed run and a repeat loop are the same hazard: a load per lap with
    /// nothing playing between them.
    fn move_on(&mut self, cause: Cause, transport: &mut Transport) -> Option<Command> {
        let hop = self.hop(cause)?;
        if cause == Cause::Unavailable || matches!(hop, Hop::Wrap | Hop::Replay) {
            self.skips = self.skips.saturating_add(1);
            if self.skips > MAX_CONSECUTIVE_SKIPS {
                return None;
            }
        }
        self.follow(hop, transport)
    }

    /// Plays what `hop` names.
    fn follow(&mut self, hop: Hop, transport: &mut Transport) -> Option<Command> {
        let index = match hop {
            Hop::Next => self.index? + 1,
            Hop::Wrap => 0,
            Hop::Replay => self.index?,
        };
        self.goto(index, transport)
    }

    /// Whether `row` is in the editable region: after the current entry.
    fn editable(&self, row: usize) -> bool {
        row >= self.first_upcoming() && row < self.tracks.len()
    }

    /// The rows of `rows` that are in the editable region, in the order given.
    fn editable_rows(&self, rows: &[usize]) -> Vec<usize> {
        rows.iter()
            .copied()
            .filter(|row| self.editable(*row))
            .collect()
    }

    /// The picked rows in the editable region, lowest first.
    fn selected_rows(&self) -> Vec<usize> {
        self.selection
            .rows()
            .filter(|row| *row >= self.first_upcoming())
            .collect()
    }

    /// `boundary` pulled into the editable region.
    fn edit_boundary(&self, boundary: usize) -> usize {
        boundary.clamp(self.first_upcoming(), self.tracks.len())
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
        self.index = Some(index);
        self.awaiting_start = true;
        Some(transport.play(&uri, &title, &artists, cover))
    }
}

#[cfg(test)]
mod tests {
    use takofy_model::TrackEntry;

    use super::*;
    use crate::message::Failure;
    use crate::reorder;

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

    /// A queue started on `count` tracks, with the current entry at `index`.
    fn started(count: usize, index: usize) -> Queue {
        let tracks = tracks(count);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, index, &mut transport);
        queue
    }

    /// The whole queue as uris, the current entry included.
    fn order(queue: &Queue) -> Vec<String> {
        queue.tracks.iter().map(|entry| entry.uri.clone()).collect()
    }

    fn uris(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// The queue's entries as their artists, which tells duplicates apart.
    fn artists(queue: &Queue) -> Vec<String> {
        queue
            .tracks
            .iter()
            .map(|entry| entry.artists.clone())
            .collect()
    }

    /// The rows the queue would show as picked.
    fn picked(queue: &Queue) -> Vec<usize> {
        (0..queue.tracks.len())
            .filter(|row| queue.selected(*row))
            .collect()
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

    /// The number of repeat clicks that reach `repeat` from Off.
    fn clicks(repeat: Repeat) -> usize {
        match repeat {
            Repeat::Off => 0,
            Repeat::All => 1,
            Repeat::One => 2,
        }
    }

    /// A queue started on `count` tracks at `index`, in `repeat` mode.
    fn started_in(count: usize, index: usize, repeat: Repeat) -> Queue {
        let mut queue = started(count, index);
        for _ in 0..clicks(repeat) {
            queue.cycle_repeat();
        }
        queue
    }

    #[test]
    fn repeat_all_wraps_to_row_zero_on_a_natural_end() {
        let mut queue = started_in(3, 2, Repeat::All);
        let mut transport = Transport::default();
        clear_guard(&mut queue, &mut transport);

        assert_eq!(end(&mut queue, &mut transport), Some(play("t0")));
        assert_eq!(
            queue.index,
            Some(0),
            "a wrap leaves the current row at zero"
        );
    }

    #[test]
    fn repeat_all_manual_next_wraps_and_can_next_is_true_on_the_last_row() {
        let mut queue = started_in(3, 2, Repeat::All);
        let mut transport = Transport::default();

        assert!(queue.can_next(), "All always has somewhere to go");
        assert_eq!(queue.next(&mut transport), Some(play("t0")));
    }

    #[test]
    fn can_next_agrees_with_next_for_every_state() {
        let mut transport = Transport::default();
        for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
            for count in 0..=4 {
                for index in 0..count {
                    let mut queue = started_in(count, index, repeat);
                    let can = queue.can_next();
                    assert_eq!(
                        queue.next(&mut transport).is_some(),
                        can,
                        "{repeat:?} on {count} rows at {index}"
                    );
                }
            }
            let mut armed = armed(3);
            for _ in 0..clicks(repeat) {
                armed.cycle_repeat();
            }
            assert!(!armed.can_next(), "{repeat:?} on an armed queue");
            assert_eq!(armed.next(&mut transport), None, "{repeat:?} armed");
        }
    }

    #[test]
    fn repeat_one_replays_the_current_row_and_arms_the_guard() {
        let mut queue = started_in(3, 1, Repeat::One);
        let mut transport = Transport::default();
        clear_guard(&mut queue, &mut transport);

        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
        assert_eq!(queue.index, Some(1));
        assert!(queue.awaiting_start, "a replay is a fresh play");
    }

    #[test]
    fn a_stale_ended_after_a_replay_is_ignored() {
        let mut queue = started_in(3, 1, Repeat::One);
        let mut transport = Transport::default();
        clear_guard(&mut queue, &mut transport);

        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "the end that follows a replay belongs to the load before it"
        );
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    }

    #[test]
    fn repeat_one_manual_next_advances_and_stops_at_the_end() {
        let mut queue = started_in(2, 0, Repeat::One);
        let mut transport = Transport::default();

        assert_eq!(queue.next(&mut transport), Some(play("t1")));
        assert!(!queue.can_next(), "One governs the end, not the button");
        assert_eq!(queue.next(&mut transport), None);
    }

    #[test]
    fn repeat_one_unavailable_moves_on_like_off() {
        let mut queue = started_in(3, 0, Repeat::One);
        let mut transport = Transport::default();
        assert_eq!(skip(&mut queue, &mut transport), Some(play("t1")));

        let mut last = started_in(2, 1, Repeat::One);
        assert_eq!(
            skip(&mut last, &mut transport),
            None,
            "an unplayable last row stops even under One"
        );
    }

    #[test]
    fn repeat_one_that_ends_without_a_position_stops_at_the_cap() {
        let mut queue = started_in(1, 0, Repeat::One);
        let mut transport = Transport::default();

        for lap in 1..=MAX_CONSECUTIVE_SKIPS {
            clear_guard(&mut queue, &mut transport);
            assert_eq!(
                end(&mut queue, &mut transport),
                Some(play("t0")),
                "replay {lap}"
            );
        }
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "a loop that never reports a position must stop at the cap"
        );
    }

    #[test]
    fn a_healthy_repeat_one_loop_never_hits_the_cap() {
        let mut queue = started_in(1, 0, Repeat::One);
        let mut transport = Transport::default();

        for lap in 0..30 {
            clear_guard(&mut queue, &mut transport);
            feed(
                &mut queue,
                &mut transport,
                TransportUpdate::Position {
                    position_ms: 1_000,
                    paused: false,
                },
            );
            assert_eq!(
                end(&mut queue, &mut transport),
                Some(play("t0")),
                "lap {lap}"
            );
        }
    }

    #[test]
    fn a_duration_resets_the_skip_run() {
        let mut queue = started_in(1, 0, Repeat::One);
        let mut transport = Transport::default();

        for lap in 0..30 {
            clear_guard(&mut queue, &mut transport);
            feed(
                &mut queue,
                &mut transport,
                TransportUpdate::Duration {
                    duration_ms: 222_000,
                },
            );
            assert_eq!(
                end(&mut queue, &mut transport),
                Some(play("t0")),
                "lap {lap}"
            );
            assert_eq!(queue.skips, 1, "a known duration must reset the streak");
        }
    }

    #[test]
    fn repeat_all_over_a_failing_queue_stops_at_the_cap() {
        let mut queue = started_in(3, 0, Repeat::All);
        let mut transport = Transport::default();

        for (lap, uri) in ["t1", "t2", "t0", "t1", "t2"].into_iter().enumerate() {
            assert_eq!(
                skip(&mut queue, &mut transport),
                Some(play(uri)),
                "lap {lap}"
            );
        }
        assert_eq!(
            skip(&mut queue, &mut transport),
            None,
            "a failing queue that wraps must still stop at the cap"
        );
    }

    #[test]
    fn a_silent_wrap_on_one_row_counts_toward_the_cap() {
        let mut queue = started_in(1, 0, Repeat::All);
        let mut transport = Transport::default();

        for lap in 1..=MAX_CONSECUTIVE_SKIPS {
            clear_guard(&mut queue, &mut transport);
            assert_eq!(
                end(&mut queue, &mut transport),
                Some(play("t0")),
                "wrap {lap}"
            );
        }
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), None, "capped");
    }

    #[test]
    fn repeat_one_on_an_armed_queue_ignores_ended_and_toggle_starts_row_zero() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.append(tracks(3)), Some(0..3));
        queue.cycle_repeat();
        queue.cycle_repeat();
        assert_eq!(queue.repeat(), Repeat::One);

        assert_eq!(end(&mut queue, &mut transport), None);
        assert!(queue.armed(), "nothing was playing, so nothing repeats");
        assert_eq!(queue.toggle(&mut transport), Some(play("t0")));
    }

    #[test]
    fn changing_repeat_never_touches_index_guard_or_skips() {
        let mut queue = started(6, 1);
        let mut transport = Transport::default();
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            Some(play("t2"))
        );
        let before = (queue.index, queue.awaiting_start, queue.skips);

        for _ in 0..3 {
            queue.cycle_repeat();
            assert_eq!((queue.index, queue.awaiting_start, queue.skips), before);
        }
        assert_eq!(queue.repeat(), Repeat::Off);
    }

    /// A queue of `count` tracks started at `index`, shuffled from `seed`.
    fn shuffled_at(count: usize, index: usize, seed: u64) -> Queue {
        let tracks = tracks(count);
        let mut queue = Queue::seeded(seed);
        let mut transport = Transport::default();
        queue.start(&tracks, index, &mut transport);
        queue.toggle_shuffle();
        queue
    }

    /// Whether the queue holds rows `t0..count`, in some order.
    fn holds_exactly(queue: &Queue, count: usize) -> bool {
        let mut held = order(queue);
        held.sort();
        let mut expected: Vec<String> = (0..count).map(|row| format!("t{row}")).collect();
        expected.sort();
        held == expected
    }

    #[test]
    fn shuffle_on_permutes_only_the_coming_rows() {
        let mut queue = started(5, 2);
        let before = order(&queue);
        queue.toggle_shuffle();

        assert_eq!(
            &order(&queue)[..2],
            &uris(&["t0", "t1"]),
            "the rows before the current one are history"
        );
        assert_eq!(order(&queue)[2], "t2", "the current entry keeps its row");
        assert_ne!(order(&queue), before);
        assert_eq!(queue.index, Some(2));
        assert!(holds_exactly(&queue, 5));
    }

    #[test]
    fn shuffle_on_an_armed_queue_permutes_every_row_and_stays_armed() {
        let mut queue = Queue::seeded(7);
        queue.append(tracks(5));
        let before = order(&queue);

        queue.toggle_shuffle();

        assert!(queue.armed(), "a permutation is not a play");
        assert_eq!(queue.first_upcoming(), 0);
        assert_ne!(order(&queue), before);
        assert!(holds_exactly(&queue, 5));
    }

    #[test]
    fn shuffle_on_and_off_never_alter_awaiting_start_or_skips() {
        let mut queue = started(6, 1);
        let mut transport = Transport::default();
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            Some(play("t2"))
        );
        let before = (queue.index, queue.awaiting_start, queue.skips);

        queue.toggle_shuffle();
        assert_eq!((queue.index, queue.awaiting_start, queue.skips), before);
        queue.toggle_shuffle();
        assert_eq!(
            (queue.index, queue.awaiting_start, queue.skips),
            before,
            "the entry is the same one, so the guard is still its load's"
        );
    }

    #[test]
    fn a_stale_ended_after_the_toggle_is_still_ignored() {
        let mut queue = started(5, 0);
        let mut transport = Transport::default();
        queue.toggle_shuffle();

        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "a toggle is no play, so the load in flight is still in flight"
        );
    }

    #[test]
    fn what_plays_next_is_what_the_pane_lists_first() {
        let mut queue = shuffled_at(6, 1, 4);
        let mut transport = Transport::default();
        let listed: Vec<String> = upcoming(&queue).into_iter().map(|(_, uri)| uri).collect();
        clear_guard(&mut queue, &mut transport);

        assert_eq!(end(&mut queue, &mut transport), Some(play(&listed[0])));
        assert_eq!(queue.index, Some(2));
        let rest: Vec<String> = upcoming(&queue).into_iter().map(|(_, uri)| uri).collect();
        assert_eq!(rest, listed[1..], "the pane still reads in play order");
    }

    #[test]
    fn a_shuffled_run_plays_every_row_once_and_is_not_the_lists_order() {
        for count in 3..=6 {
            for seed in 0..50 {
                let list = tracks(count);
                let mut queue = Queue::seeded(seed);
                let mut transport = Transport::default();
                queue.start(&list, 0, &mut transport);
                queue.toggle_shuffle();

                let mut played = Vec::new();
                loop {
                    played.push(
                        queue
                            .current()
                            .map(|entry| entry.uri.clone())
                            .expect("a current row"),
                    );
                    if queue.next(&mut transport).is_none() {
                        break;
                    }
                }

                let mut once = played.clone();
                once.sort();
                let mut listed: Vec<String> = list.iter().map(|entry| entry.uri.clone()).collect();
                listed.sort();
                assert_eq!(once, listed, "{count} rows, seed {seed}");
                assert_ne!(
                    played, listed,
                    "{count} rows, seed {seed}: the run repeated the list's order"
                );
            }
        }
    }

    #[test]
    fn shuffle_on_clears_the_pick_and_the_refocus() {
        let mut queue = started(5, 0);
        queue.press(1, Press::Replace);
        queue.press(3, Press::Toggle);
        queue.refocus_at(3);
        assert_eq!(picked(&queue), vec![1, 3]);

        queue.toggle_shuffle();
        assert!(
            picked(&queue).is_empty(),
            "a pick cannot survive other rows"
        );
        assert_eq!(queue.refocus(), None);

        queue.press(1, Press::Replace);
        queue.toggle_shuffle();
        assert!(
            picked(&queue).is_empty(),
            "and turning it off clears it too"
        );
    }

    #[test]
    fn shuffle_off_restores_the_original_order_and_keeps_the_current_entry() {
        for count in 1..=7 {
            for index in 0..count {
                for seed in 0..20 {
                    let mut queue = shuffled_at(count, index, seed);
                    assert!(holds_exactly(&queue, count), "{count} {index} {seed}");
                    let current = queue.current().map(|entry| entry.uri.clone());
                    assert!(current.is_some(), "{count} {index} {seed}");

                    queue.toggle_shuffle();

                    let expected: Vec<String> = (0..count).map(|row| format!("t{row}")).collect();
                    assert_eq!(order(&queue), expected, "{count} {index} {seed}");
                    assert_eq!(
                        queue.current().map(|entry| entry.uri.clone()),
                        current,
                        "{count} {index} {seed}: the current entry changed"
                    );
                    assert_eq!(
                        queue.index,
                        Some(index),
                        "{count} {index} {seed}: the entry is back on its own row"
                    );
                }
            }
        }

        let mut empty = Queue::default();
        empty.toggle_shuffle();
        empty.toggle_shuffle();
        assert_eq!(order(&empty), Vec::<String>::new());
        assert!(!empty.armed(), "an empty queue is not armed");
    }

    #[test]
    fn shuffle_off_after_advancing_keeps_the_current_entry() {
        let mut queue = shuffled_at(6, 0, 11);
        let mut transport = Transport::default();
        for _ in 0..2 {
            assert!(queue.next(&mut transport).is_some());
        }
        let current = queue.current().map(|entry| entry.uri.clone());

        queue.toggle_shuffle();

        assert_eq!(queue.current().map(|entry| entry.uri.clone()), current);
        assert_eq!(
            order(&queue),
            (0..6).map(|row| format!("t{row}")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn shuffle_off_honours_a_removal_puts_an_append_last_and_discards_a_move() {
        let mut queue = shuffled_at(5, 0, 3);
        let row = order(&queue)
            .iter()
            .position(|uri| uri == "t3")
            .expect("t3 is queued");
        assert_ne!(row, 0, "the current row is never editable");
        assert_eq!(
            queue.remove_rows(&[row], &queue.entries_at(&[row])),
            Some(1)
        );
        assert!(queue.append(vec![track("x")]).is_some());
        let last = order(&queue).len();
        assert!(
            queue.move_rows(&[1], last).is_some(),
            "a move the restore has to discard"
        );

        queue.toggle_shuffle();

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t4", "x"]));
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t0"));
    }

    #[test]
    fn shuffle_off_on_an_armed_queue() {
        let mut queue = Queue::seeded(5);
        queue.append(tracks(5));
        let before = order(&queue);

        queue.toggle_shuffle();
        assert!(queue.armed());
        queue.toggle_shuffle();

        assert_eq!(order(&queue), before);
        assert!(
            queue.armed(),
            "nothing was playing, so nothing became current"
        );
        assert_eq!(queue.first_upcoming(), 0);
    }

    #[test]
    fn shuffle_off_with_a_load_in_flight_keeps_the_guard_and_the_entry() {
        let mut queue = started(5, 1);
        let mut transport = Transport::default();

        queue.toggle_shuffle();
        queue.toggle_shuffle();

        assert!(queue.awaiting_start, "a mode switch is no play");
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
        assert_eq!(
            end(&mut queue, &mut transport),
            None,
            "the load in flight is still ignored"
        );
    }

    #[test]
    fn the_same_seed_gives_the_same_queue_and_two_seeds_differ() {
        let first = shuffled_at(6, 0, 1);
        let second = shuffled_at(6, 0, 1);
        let other = shuffled_at(6, 0, 2);

        assert_eq!(order(&first), order(&second));
        assert_ne!(order(&first), order(&other));
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
            access: None,
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

    #[test]
    fn moving_one_row_down_puts_it_before_the_boundary_entry() {
        let mut queue = started(5, 0);
        assert_eq!(queue.move_rows(&[1], 3), Some(2..3));
        assert_eq!(order(&queue), uris(&["t0", "t2", "t1", "t3", "t4"]));
    }

    #[test]
    fn moving_one_row_up_puts_it_before_the_boundary_entry() {
        let mut queue = started(5, 0);
        assert_eq!(queue.move_rows(&[3], 2), Some(2..3));
        assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
    }

    /// A queue holding `count` rows with nothing current, so every row and
    /// boundary is editable.
    fn armed(count: usize) -> Queue {
        let mut queue = Queue::default();
        queue.append(tracks(count));
        queue
    }

    /// Every non-empty pick set over `len` rows, each ascending.
    fn subsets(len: usize) -> Vec<Vec<usize>> {
        (1..(1usize << len) as u64)
            .map(|mask| {
                (0..len)
                    .filter(|row| mask >> row & 1 == 1)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn moving_a_block_leaves_the_order_a_playlist_block_leaves() {
        for len in 1..=6 {
            for rows in subsets(len) {
                for boundary in 0..=len {
                    let mut queue = armed(len);
                    let before = order(&queue);
                    let found = reorder::block(&before, &rows, boundary);
                    let moved = queue.move_rows(&rows, boundary);
                    match found {
                        None => {
                            assert_eq!(moved, None, "{len} {rows:?} {boundary}");
                            assert_eq!(order(&queue), before, "{len} {rows:?} {boundary}");
                        }
                        Some(found) => {
                            assert_eq!(
                                moved,
                                Some(found.landed..found.landed + rows.len()),
                                "{len} {rows:?} {boundary}"
                            );
                            assert_eq!(order(&queue), found.order, "{len} {rows:?} {boundary}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn moving_a_block_past_the_current_entry_agrees_where_the_queue_may_edit() {
        // A started queue clamps a boundary and drops the rows at or before the
        // current entry, so the sweep is its own editable region.
        for len in 1..=6 {
            for rows in subsets(len).into_iter().filter(|rows| rows[0] >= 1) {
                for boundary in 1..=len {
                    let mut queue = started(len, 0);
                    let before = order(&queue);
                    let found = reorder::block(&before, &rows, boundary);
                    let moved = queue.move_rows(&rows, boundary);
                    match found {
                        None => assert_eq!(moved, None, "{len} {rows:?} {boundary}"),
                        Some(found) => {
                            assert_eq!(
                                moved,
                                Some(found.landed..found.landed + rows.len()),
                                "{len} {rows:?} {boundary}"
                            );
                            assert_eq!(order(&queue), found.order, "{len} {rows:?} {boundary}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn moving_a_block_keeps_its_order() {
        let mut queue = started(6, 0);
        assert_eq!(queue.move_rows(&[2, 1], 5), Some(3..5));
        assert_eq!(order(&queue), uris(&["t0", "t3", "t4", "t1", "t2", "t5"]));
    }

    #[test]
    fn a_gapped_selection_gathers_at_the_boundary() {
        let mut queue = started(6, 0);
        assert_eq!(queue.move_rows(&[3, 1], 5), Some(3..5));
        assert_eq!(order(&queue), uris(&["t0", "t2", "t4", "t1", "t3", "t5"]));
    }

    #[test]
    fn a_contiguous_block_dropped_inside_or_beside_itself_changes_nothing() {
        let mut queue = started(6, 0);
        let before = order(&queue);

        for boundary in [2, 3, 4, 5] {
            assert_eq!(queue.move_rows(&[2, 3, 4], boundary), None, "{boundary}");
        }

        assert_eq!(order(&queue), before);
    }

    #[test]
    fn a_move_selects_the_landed_block() {
        let mut queue = started(6, 0);
        queue.press(1, Press::Replace);
        queue.press(3, Press::Toggle);
        assert_eq!(picked(&queue), vec![1, 3]);

        assert_eq!(queue.move_rows(&[1, 3], 5), Some(3..5));

        assert_eq!(picked(&queue), vec![3, 4]);
    }

    #[test]
    fn rows_at_or_before_the_current_entry_cannot_move() {
        let mut queue = started(5, 2);
        let before = order(&queue);

        assert_eq!(queue.move_rows(&[0, 1, 2], 4), None);
        assert_eq!(queue.move_rows(&[1], 4), None);

        assert_eq!(order(&queue), before);
    }

    #[test]
    fn a_boundary_before_the_first_upcoming_row_lands_there() {
        let mut queue = started(5, 1);
        assert_eq!(queue.move_rows(&[3], 0), Some(2..3));
        assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
    }

    #[test]
    fn a_boundary_past_the_end_appends() {
        let mut queue = started(5, 0);
        assert_eq!(queue.move_rows(&[1], 99), Some(4..5));
        assert_eq!(order(&queue), uris(&["t0", "t2", "t3", "t4", "t1"]));
    }

    #[test]
    fn an_edit_never_touches_the_index_the_guard_or_the_skips() {
        let mut queue = started(6, 1);
        let mut transport = Transport::default();
        clear_guard(&mut queue, &mut transport);
        // A failed load leaves the index one further on, the guard armed for the
        // new load and one skip counted.
        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            Some(play("t2"))
        );
        assert_eq!(
            (queue.index, queue.awaiting_start, queue.skips),
            (Some(2), true, 1)
        );

        assert!(queue.move_rows(&[3], 5).is_some());
        assert!(queue.insert_at(4, vec![track("x")]).is_some());
        assert!(queue.step_rows(&[3], Step::Down).is_some());

        assert_eq!(
            (queue.index, queue.awaiting_start, queue.skips),
            (Some(2), true, 1),
            "no edit may move the index, disarm a load or reset a skip"
        );
    }

    #[test]
    fn duplicates_keep_their_own_identity() {
        let entries = vec![
            track_by("same", "A"),
            track_by("same", "B"),
            track_by("same", "C"),
        ];
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&entries, 0, &mut transport);

        assert_eq!(queue.move_rows(&[2], 1), Some(1..2));
        assert_eq!(artists(&queue), uris(&["A", "C", "B"]));
    }

    #[test]
    fn insert_in_the_middle() {
        let mut queue = started(3, 0);
        assert_eq!(queue.insert_at(2, vec![track("x")]), Some(2..3));
        assert_eq!(order(&queue), uris(&["t0", "t1", "x", "t2"]));
    }

    #[test]
    fn insert_at_the_front_of_the_upcoming_rows() {
        let mut queue = started(4, 1);
        assert_eq!(queue.insert_at(0, vec![track("x")]), Some(2..3));
        assert_eq!(order(&queue), uris(&["t0", "t1", "x", "t2", "t3"]));
    }

    #[test]
    fn insert_at_the_end() {
        let mut queue = started(3, 0);
        assert_eq!(queue.insert_at(99, vec![track("x")]), Some(3..4));
        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "x"]));
    }

    #[test]
    fn an_insert_shifts_selected_rows_after_it() {
        let mut queue = started(5, 0);
        queue.press(2, Press::Replace);
        queue.press(4, Press::Toggle);

        assert_eq!(queue.insert_at(3, vec![track("x"), track("y")]), Some(3..5));

        assert_eq!(picked(&queue), vec![2, 6]);
    }

    #[test]
    fn an_empty_queue_takes_nothing_to_play() {
        let mut queue = Queue::default();

        assert_eq!(
            queue.toggle(&mut Transport::default()),
            None,
            "an empty queue has no first row to start"
        );
    }

    #[test]
    fn an_insert_lands_in_an_empty_queue_and_leaves_it_armed() {
        let mut queue = Queue::default();

        assert_eq!(queue.insert_at(0, vec![track("x")]), Some(0..1));

        assert_eq!(order(&queue), uris(&["x"]));
        assert_eq!(queue.first_upcoming(), 0, "nothing is current yet");
        assert_eq!(queue.upcoming().len(), 1);
        assert!(queue.armed(), "the row waits for a first play");
    }

    #[test]
    fn the_first_play_of_an_armed_queue_starts_its_first_row() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        let armed = queue.append(vec![track("x"), track("y")]);
        assert_eq!(armed, Some(0..2));

        assert!(!queue.can_previous(), "nothing has played yet");
        assert!(!queue.can_next(), "nothing is current to step from");
        assert_eq!(queue.next(&mut transport), None);
        assert_eq!(queue.previous(0, &mut transport), None);

        assert_eq!(queue.toggle(&mut transport), Some(play("x")));

        assert_eq!(queue.first_upcoming(), 1);
        assert!(queue.can_next());
        assert!(!queue.armed(), "it is playing the row that was waiting");
    }

    #[test]
    fn an_armed_queue_ignores_the_events_of_a_track_it_never_started() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(queue.append(vec![track("x"), track("y")]), Some(0..2));

        for update in [TransportUpdate::Ended, TransportUpdate::Unavailable] {
            assert_eq!(
                feed(&mut queue, &mut transport, update.clone()),
                None,
                "nothing is playing, so {update:?} is nobody's"
            );
        }
        assert!(
            queue.armed(),
            "an armed queue must not walk itself on a stray event"
        );
        assert_eq!(order(&queue), uris(&["x", "y"]));
    }

    #[test]
    fn a_jump_starts_an_armed_row_and_leaves_the_ones_before_it() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.append(vec![track("x"), track("y"), track("z")]),
            Some(0..3)
        );

        assert_eq!(queue.jump(2, &mut transport), Some(play("z")));

        assert_eq!(queue.first_upcoming(), 3);
        assert_eq!(order(&queue), uris(&["x", "y", "z"]));
        assert!(!queue.armed());
    }

    #[test]
    fn an_armed_queue_edits_every_row() {
        let mut queue = Queue::default();
        let armed = queue.append(vec![track("x"), track("y"), track("z")]);
        assert_eq!(armed, Some(0..3));

        assert_eq!(queue.lift(0), Some(vec![0]), "the first row is editable");
        queue.press(0, Press::Replace);
        assert_eq!(picked(&queue), vec![0]);
        assert_eq!(queue.step_rows(&[0], Step::Down), Some(vec![(0, 1)]));
        assert_eq!(order(&queue), uris(&["y", "x", "z"]));
        assert_eq!(picked(&queue), vec![1], "the pick follows the row");
    }

    #[test]
    fn an_empty_queue_moves_nothing() {
        let mut queue = Queue::default();
        assert_eq!(queue.first_upcoming(), 0);
        assert_eq!(queue.move_rows(&[0], 0), None);
        assert_eq!(queue.step_rows(&[0], Step::Up), None);
        assert_eq!(queue.lift(0), None);
    }

    #[test]
    fn no_entries_insert_nothing() {
        let mut queue = started(3, 0);
        let before = order(&queue);

        assert_eq!(queue.insert_at(1, Vec::new()), None);

        assert_eq!(order(&queue), before);
    }

    #[test]
    fn append_adds_after_the_last_entry_unpicked() {
        let mut queue = started(3, 0);

        assert_eq!(queue.append(vec![track("x"), track("y")]), Some(3..5));

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "x", "y"]));
        assert!(picked(&queue).is_empty(), "an add must not pick its rows");
    }

    #[test]
    fn append_while_the_last_entry_plays() {
        let mut queue = started(3, 2);

        assert_eq!(queue.append(vec![track("x")]), Some(3..4));

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "x"]));
        assert!(queue.can_next(), "the added entry is reachable");
        assert!(picked(&queue).is_empty());
    }

    #[test]
    fn append_keeps_an_existing_pick() {
        let mut queue = started(5, 0);
        queue.press(2, Press::Replace);
        queue.press(4, Press::Toggle);

        assert_eq!(queue.append(vec![track("x")]), Some(5..6));

        assert_eq!(picked(&queue), vec![2, 4]);
    }

    #[test]
    fn an_empty_queue_takes_an_append_at_its_first_row() {
        let mut queue = Queue::default();

        assert_eq!(queue.append(vec![track("x")]), Some(0..1));

        assert_eq!(order(&queue), uris(&["x"]));
        assert_eq!(queue.first_upcoming(), 0);
    }

    #[test]
    fn appending_nothing_changes_nothing() {
        let mut queue = started(3, 0);
        let before = order(&queue);

        assert_eq!(queue.append(Vec::new()), None);

        assert_eq!(order(&queue), before);
    }

    #[test]
    fn step_moves_one_row_up_and_down() {
        let mut queue = started(5, 0);
        assert_eq!(queue.step_rows(&[3], Step::Up), Some(vec![(3, 2)]));
        assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
        assert_eq!(queue.step_rows(&[2], Step::Down), Some(vec![(2, 3)]));
        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4"]));
    }

    #[test]
    fn step_stops_at_the_first_upcoming_row_and_the_last_entry() {
        let mut queue = started(4, 1);

        assert_eq!(queue.step_rows(&[2], Step::Up), None);
        assert_eq!(queue.step_rows(&[3], Step::Down), None);

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3"]));
    }

    #[test]
    fn a_block_steps_as_a_unit() {
        let mut queue = started(6, 0);

        assert_eq!(
            queue.step_rows(&[2, 3, 4], Step::Up),
            Some(vec![(2, 1), (3, 2), (4, 3)])
        );
        assert_eq!(order(&queue), uris(&["t0", "t2", "t3", "t4", "t1", "t5"]));
        assert_eq!(picked(&queue), vec![1, 2, 3]);

        assert_eq!(
            queue.step_rows(&[1, 2, 3], Step::Down),
            Some(vec![(1, 2), (2, 3), (3, 4)])
        );
        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4", "t5"]));
    }

    #[test]
    fn a_blocked_row_blocks_the_ones_behind_it() {
        let mut queue = started(5, 0);
        let before = order(&queue);

        assert_eq!(queue.step_rows(&[1, 2], Step::Up), None);
        assert_eq!(order(&queue), before);

        assert_eq!(queue.step_rows(&[3, 4], Step::Down), None);
        assert_eq!(order(&queue), before);
    }

    #[test]
    fn a_pick_blocked_at_the_top_keeps_its_pick() {
        let mut queue = started(5, 0);
        queue.press(1, Press::Replace);
        queue.press(3, Press::Toggle);

        assert_eq!(queue.step_rows(&[1, 3], Step::Up), Some(vec![(3, 2)]));

        assert_eq!(picked(&queue), vec![1, 2]);
        assert_eq!(queue.acting_rows(None), vec![1, 2]);
        assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
    }

    #[test]
    fn a_pick_blocked_at_the_bottom_keeps_its_pick() {
        let mut queue = started(5, 0);
        queue.press(2, Press::Replace);
        queue.press(4, Press::Toggle);

        assert_eq!(queue.step_rows(&[2, 4], Step::Down), Some(vec![(2, 3)]));

        assert_eq!(picked(&queue), vec![3, 4]);
        assert_eq!(queue.acting_rows(None), vec![3, 4]);
        assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
    }

    #[test]
    fn step_reports_old_and_new_positions() {
        let mut queue = started(5, 0);

        assert_eq!(
            queue.step_rows(&[3, 1], Step::Down),
            Some(vec![(1, 2), (3, 4)])
        );

        assert_eq!(order(&queue), uris(&["t0", "t2", "t1", "t4", "t3"]));
    }

    #[test]
    fn press_outside_the_region_is_ignored() {
        let mut queue = started(4, 1);

        queue.press(0, Press::Replace);
        queue.press(1, Press::Replace);
        queue.press(4, Press::Replace);

        assert!(picked(&queue).is_empty());
    }

    #[test]
    fn lift_keeps_a_selection_it_is_in() {
        let mut queue = started(8, 0);
        queue.press(2, Press::Replace);
        queue.press(3, Press::Toggle);
        queue.press(6, Press::Toggle);

        assert_eq!(queue.lift(3), Some(vec![2, 3, 6]));
        assert_eq!(picked(&queue), vec![2, 3, 6]);
    }

    #[test]
    fn lift_selects_an_unpicked_row() {
        let mut queue = started(8, 0);
        queue.press(2, Press::Replace);
        queue.press(3, Press::Toggle);

        assert_eq!(queue.lift(1), Some(vec![1]));
    }

    #[test]
    fn lifting_outside_the_region_carries_nothing() {
        let mut queue = started(3, 1);

        assert_eq!(queue.lift(1), None);
        assert_eq!(queue.lift(3), None);
    }

    #[test]
    fn start_clears_the_selection() {
        let mut queue = started(4, 0);
        queue.press(2, Press::Replace);
        queue.refocus_at(3);
        assert_eq!(picked(&queue), vec![2]);
        assert_eq!(queue.refocus(), Some(3));

        let tracks = tracks(3);
        let mut transport = Transport::default();
        queue.start(&tracks, 0, &mut transport);

        assert!(picked(&queue).is_empty());
        assert_eq!(queue.refocus(), None);
    }

    #[test]
    fn advancing_leaves_the_selection_and_the_refocus_alone() {
        let mut queue = started(4, 0);
        let mut transport = Transport::default();
        queue.press(2, Press::Replace);
        queue.refocus_at(2);

        assert_eq!(queue.next(&mut transport), Some(play("t1")));

        assert!(queue.selection.contains(2));
        assert_eq!(queue.refocus(), Some(2));
    }

    #[test]
    fn a_selected_row_that_became_current_is_not_selected() {
        let mut queue = started(4, 0);
        let mut transport = Transport::default();
        queue.press(1, Press::Replace);
        queue.press(2, Press::Toggle);
        assert_eq!(picked(&queue), vec![1, 2]);

        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));

        assert!(!queue.selected(1), "the current entry is never picked");
        assert!(queue.selected(2));
        assert_eq!(queue.acting_rows(None), vec![2]);
    }

    #[test]
    fn acting_rows_prefers_the_selection_then_the_focused_row() {
        let mut queue = started(6, 1);

        assert_eq!(queue.acting_rows(Some(4)), vec![4]);
        assert!(queue.acting_rows(Some(1)).is_empty());
        assert!(queue.acting_rows(None).is_empty());

        queue.press(2, Press::Replace);
        queue.press(3, Press::Toggle);
        assert_eq!(queue.acting_rows(Some(5)), vec![2, 3]);
    }

    #[test]
    fn refocus_lives_until_settled() {
        let mut queue = started(4, 0);

        assert_eq!(queue.refocus(), None);
        queue.refocus_at(3);
        assert_eq!(queue.refocus(), Some(3));
        queue.settle_refocus();
        assert_eq!(queue.refocus(), None);
    }

    #[test]
    fn current_is_the_entry_at_index_and_none_when_armed_or_empty() {
        assert_eq!(Queue::default().current(), None);
        assert_eq!(armed(3).current(), None, "an armed queue has no current");
        let queue = started(4, 2);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t2"));
    }

    #[test]
    fn act_on_inside_the_pick_returns_it_ascending_and_keeps_it() {
        let mut queue = started(8, 0);
        queue.press(5, Press::Replace);
        queue.press(2, Press::Toggle);

        assert_eq!(queue.act_on(5), vec![2, 5]);
        assert_eq!(picked(&queue), vec![2, 5], "act_on collapsed the pick");
    }

    #[test]
    fn act_on_outside_replaces_the_pick() {
        let mut queue = started(8, 0);
        queue.press(5, Press::Replace);
        queue.press(2, Press::Toggle);

        assert_eq!(queue.act_on(4), vec![4]);
        assert_eq!(picked(&queue), vec![4]);
    }

    #[test]
    fn act_on_the_current_entry_is_empty() {
        let mut queue = started(8, 3);
        queue.press(5, Press::Replace);

        assert!(queue.act_on(3).is_empty());
        assert!(queue.act_on(0).is_empty());
        assert_eq!(picked(&queue), vec![5], "a refused row touched the pick");
    }

    #[test]
    fn act_on_drops_a_picked_row_the_queue_has_passed() {
        let mut queue = started(6, 1);
        let mut transport = Transport::default();
        queue.press(2, Press::Replace);
        queue.press(3, Press::Toggle);
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t2")));

        assert_eq!(
            queue.act_on(3),
            vec![3],
            "the row that became current went into the cut"
        );
    }

    #[test]
    fn act_on_a_pick_the_queue_has_passed_is_empty() {
        let mut queue = started(6, 1);
        let mut transport = Transport::default();
        queue.press(2, Press::Replace);
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t2")));

        assert!(
            queue.act_on(2).is_empty(),
            "a pick the queue has passed was offered"
        );
    }

    #[test]
    fn remove_rows_takes_the_rows_and_keeps_the_rest_in_order() {
        let mut queue = started(6, 0);
        let expect = queue.entries_at(&[1, 3, 4]);

        assert_eq!(queue.remove_rows(&[1, 3, 4], &expect), Some(3));
        assert_eq!(order(&queue), uris(&["t0", "t2", "t5"]));
    }

    #[test]
    fn a_queue_holding_the_same_entry_twice_removes_the_one_picked() {
        let tracks = vec![track("t0"), track("t1"), track("t1"), track("t2")];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, &mut Transport::default());
        queue.press(2, Press::Replace);

        let rows = queue.act_on(2);
        let expect = queue.entries_at(&rows);

        assert_eq!(rows, vec![2], "the pick did not name the second copy");
        assert_eq!(
            queue.remove_rows(&rows, &expect),
            Some(1),
            "the identical neighbour was taken too"
        );
        assert_eq!(order(&queue), uris(&["t0", "t1", "t2"]));
    }

    #[test]
    fn remove_rows_never_moves_index_awaiting_start_or_skips() {
        for len in 1..=6 {
            for index in 0..len {
                for rows in subsets(len).into_iter().filter(|rows| rows[0] > index) {
                    let mut queue = started(len, index);
                    let expect = queue.entries_at(&rows);
                    let before = (queue.index, queue.awaiting_start, queue.skips);

                    assert_eq!(
                        queue.remove_rows(&rows, &expect),
                        Some(rows.len()),
                        "len {len} at {index}, rows {rows:?}"
                    );
                    assert_eq!(
                        (queue.index, queue.awaiting_start, queue.skips),
                        before,
                        "len {len} at {index}, rows {rows:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_armed_queue_stays_armed_or_empties() {
        let mut queue = armed(4);
        let expect = queue.entries_at(&[1]);
        assert_eq!(queue.remove_rows(&[1], &expect), Some(1));
        assert!(queue.armed());
        assert_eq!(order(&queue), uris(&["t0", "t2", "t3"]));

        let expect = queue.entries_at(&[0, 1, 2]);
        assert_eq!(queue.remove_rows(&[0, 1, 2], &expect), Some(3));
        assert!(!queue.armed(), "an emptied queue is not armed");
        assert!(order(&queue).is_empty());
    }

    #[test]
    fn emptying_the_upcoming_region_leaves_the_current_entry_and_no_next() {
        let mut queue = started(4, 1);
        let expect = queue.entries_at(&[2, 3]);

        assert_eq!(queue.remove_rows(&[2, 3], &expect), Some(2));
        assert_eq!(order(&queue), uris(&["t0", "t1"]));
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
        assert!(!queue.can_next());
        assert_eq!(queue.refocus(), None);
    }

    #[test]
    fn a_non_upcoming_row_refuses_the_whole_removal() {
        let mut queue = started(6, 2);
        let expect = queue.entries_at(&[2, 3]);
        assert_eq!(queue.remove_rows(&[2, 3], &expect), None, "the current row");

        let expect = queue.entries_at(&[1, 3]);
        assert_eq!(queue.remove_rows(&[1, 3], &expect), None, "a row before it");

        let expect = queue.entries_at(&[3, 9]);
        assert_eq!(
            queue.remove_rows(&[3, 9], &expect),
            None,
            "a row past the end"
        );

        assert_eq!(
            order(&queue),
            uris(&["t0", "t1", "t2", "t3", "t4", "t5"]),
            "a refused removal changed the list"
        );
    }

    #[test]
    fn a_stale_entry_refuses_it() {
        let mut queue = started(6, 0);
        let mut other = track("t1");
        other.artists = "another band".to_owned();

        assert_eq!(
            queue.remove_rows(&[1, 2], &[other, track("t2")]),
            None,
            "identical neighbours would pass a comparison of uris alone"
        );
        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4", "t5"]));
        assert!(picked(&queue).is_empty());
    }

    #[test]
    fn the_selection_shifts_and_is_empty_after_a_whole_pick_goes() {
        let mut queue = started(8, 0);
        queue.press(2, Press::Replace);
        queue.press(5, Press::Toggle);
        queue.press(6, Press::Toggle);

        let expect = queue.entries_at(&[2, 3]);
        assert_eq!(queue.remove_rows(&[2, 3], &expect), Some(2));
        assert_eq!(picked(&queue), vec![3, 4]);

        let rest = queue.acting_rows(None);
        let expect = queue.entries_at(&rest);
        assert_eq!(queue.remove_rows(&rest, &expect), Some(2));
        assert!(picked(&queue).is_empty());
    }

    #[test]
    fn refocus_lands_on_the_row_now_at_the_first_removed_index_or_is_none() {
        let mut queue = started(6, 0);
        let expect = queue.entries_at(&[2, 3]);
        queue.remove_rows(&[2, 3], &expect);

        assert_eq!(queue.refocus(), Some(2));
        assert_eq!(queue.tracks[2].uri, "t4");

        let mut queue = started(4, 0);
        let expect = queue.entries_at(&[3]);
        queue.remove_rows(&[3], &expect);
        assert_eq!(queue.refocus(), None, "the last row left nothing in place");
    }

    #[test]
    fn refocus_is_none_when_nothing_upcoming_remains() {
        let mut queue = started(4, 1);
        let expect = queue.entries_at(&[2, 3]);
        queue.remove_rows(&[2, 3], &expect);

        assert_eq!(queue.refocus(), None);
        assert_ne!(
            queue.refocus(),
            queue.index,
            "clamped onto the current entry"
        );
    }

    #[test]
    fn a_stale_refocus_is_overwritten() {
        let mut queue = started(6, 0);
        queue.refocus_at(5);
        let expect = queue.entries_at(&[1]);
        queue.remove_rows(&[1], &expect);
        assert_eq!(queue.refocus(), Some(1));

        let mut queue = started(4, 1);
        queue.refocus_at(2);
        let expect = queue.entries_at(&[2, 3]);
        queue.remove_rows(&[2, 3], &expect);
        assert_eq!(queue.refocus(), None);
    }

    #[test]
    fn remove_rows_returns_the_count() {
        let mut queue = started(8, 0);
        let expect = queue.entries_at(&[5, 1, 2]);

        assert_eq!(queue.remove_rows(&[5, 1, 2], &expect), Some(3));
        assert_eq!(order(&queue), uris(&["t0", "t3", "t4", "t6", "t7"]));
        assert_eq!(queue.refocus(), Some(1));
    }
}
