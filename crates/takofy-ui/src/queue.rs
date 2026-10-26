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
//! them in. Turning it on leaves the rows before the current one alone; turning
//! it off restores the whole list, history included, and re-seats `index` on the
//! entry it was already on.
//!
//! The queue also edits its own order and keeps the selection over its rows,
//! because both are indices into it. One rule holds edits apart from the
//! playing entry: an edit never touches `index`, `awaiting_start` or `skips`; a
//! mode switch may re-seat `index` but never changes which entry is current.
//! The rows an edit may move all sit after the current entry, so the playing
//! entry keeps its place and a load in flight keeps its guard.
//!
//! The phone edits the queue too, and it names a window around the row it is on
//! rather than a whole list: that window replaces the rows on both sides of the
//! playing entry, and a window that cannot be anchored or parsed changes
//! nothing.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use takofy_model::{Landing, TrackEntry};

use crate::message::{Command, RemoteUpdate, TransportUpdate, Update};
use crate::player::{NowPlaying, RESTART_AFTER_MS, Transport, TransportState};
use crate::reorder;
use crate::repeat::{self, Cause, Hop, Repeat};
use crate::selection::{Press, Selection};
use crate::shuffle;

/// How many tracks an unplayable run may skip before the queue stops and shows
/// the failure. A free account fails every track, and an uncapped run would
/// fire a load per entry.
pub const MAX_CONSECUTIVE_SKIPS: u32 = 5;

/// The most rows one label request may name.
///
/// A hand-over can adopt thousands of rows, and one command carrying every URI
/// would grow without bound on every frame; the rest wait for the next pass, so
/// a row stays a URI a little longer instead of the request growing.
pub const LABEL_BATCH: usize = 256;

/// The most rows a phone's queue window may replace ours with.
///
/// The phone draws a handful either side of its current row; a window past this
/// is a payload that cannot be one, and taking it would drop every row the user
/// is looking at.
pub const MAX_WINDOW_ROWS: usize = 2_000;

/// The row the phone's own queue keeps between blocks; it names no track.
const DELIMITER_URI: &str = "spotify:delimiter";

/// Why a phone's queue window could not replace ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRefusal {
    /// The queue holds no current row, so the window has nothing to sit around.
    NoCurrent,
    /// A row that is neither a separator nor a Spotify uri.
    BadRow,
    /// A window larger than [`MAX_WINDOW_ROWS`].
    TooLarge,
}

impl WindowRefusal {
    /// The word a log carries. Which refusal it was is the only thing that says
    /// whether the phone's window or our own queue was the problem, and it names
    /// a kind, never a row.
    pub fn word(&self) -> &'static str {
        match self {
            Self::NoCurrent => "no current row",
            Self::BadRow => "bad row",
            Self::TooLarge => "too large",
        }
    }
}

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
    /// The context the current rows came from: the page a local start was
    /// played from, or the landing a hand-over adopted. A move carries it, so a
    /// queue with no context cannot be handed to another device.
    origin: Option<String>,
    /// The newest activation epoch a loss named. A landing that names it or an
    /// older one was overtaken by that loss and is not adopted.
    lost_epoch: u64,
    /// Row URIs a label was asked for, so a row that never comes back named
    /// does not become a request per frame.
    label_asked: HashSet<String>,
    /// How many rows at the head of the upcoming region the phone queued to play
    /// next, counted from `first_upcoming`.
    ///
    /// It is a count from the current row, so the rows that stay the phone's
    /// block are the ones still ahead of it: a move forward into the block leaves
    /// its tail, a move past it or back out of it leaves nothing, and an edit
    /// clamps it to the rows that remain.
    queued: usize,
    /// The newest queue revision the phone published.
    ///
    /// It is echoed back so the phone can match its window to ours; it is never
    /// a token, because a revision that arrives late is still the truth about the
    /// queue that sent it. A start, a landing, a draw and every edit forget it,
    /// because each of those makes these rows no longer the window the revision
    /// names.
    queue_revision: String,
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
    ///
    /// The modes survive: with shuffle on, the clicked row plays first and the
    /// rest are drawn again, and the list's own order is what turning shuffle off
    /// puts back.
    /// Starts a list with shuffle on, drawing the rest of it in a new order.
    ///
    /// Shuffle is a mode, so it stays on afterwards: a band's shuffle is the same
    /// switch as the playbar's, not a one-off.
    pub fn start_with_shuffle(
        &mut self,
        tracks: &[TrackEntry],
        index: usize,
        origin: Option<&str>,
        transport: &mut Transport,
    ) -> Option<Command> {
        self.shuffle = true;
        self.start(tracks, index, origin, transport)
    }

    /// Replaces the queue with `tracks` and starts on `index`.
    ///
    /// The modes survive: with shuffle on, the clicked row plays first and the
    /// rest are drawn again, and the list's own order is what turning shuffle off
    /// puts back. `origin` is the page the rows came from, so a move can tell the
    /// target what to play; a list with no page of its own passes `None`.
    pub fn start(
        &mut self,
        tracks: &[TrackEntry],
        index: usize,
        origin: Option<&str>,
        transport: &mut Transport,
    ) -> Option<Command> {
        if index >= tracks.len() {
            return None;
        }
        self.tracks = tracks.to_vec();
        self.skips = 0;
        self.queued = 0;
        self.queue_revision.clear();
        self.selection.clear();
        self.refocus = None;
        self.origin = origin.map(str::to_owned);
        self.index = None;
        let first = self.start_shuffled(index);
        self.goto(first, transport)
    }

    /// The row a new list starts on, with the rest of it drawn if shuffle is on.
    fn start_shuffled(&mut self, index: usize) -> usize {
        if !self.shuffle {
            return index;
        }
        self.snapshot = self.uris();
        self.tracks.swap(0, index);
        self.draw_after(1);
        0
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
            // A rebuild is already bringing the track back, so a play asked
            // mid-rebuild would start a second load.
            TransportState::Reconnecting => None,
            TransportState::Idle
            | TransportState::Stopped
            | TransportState::Unavailable
            | TransportState::Failed(_) => {
                self.skips = 0;
                // An armed queue has no current entry to replay, so the first
                // play starts the first row.
                let row = self.index.unwrap_or(0);
                self.draw_if_undrawn(row);
                self.goto(row, transport)
            }
        }
    }

    /// Folds one update in and returns the command it implies, if any.
    pub fn apply(&mut self, update: &Update, transport: &mut Transport) -> Option<Command> {
        match update {
            Update::Transport(update) => self.apply_transport(update, transport),
            Update::Remote(update) => self.apply_remote(update, transport),
            _ => None,
        }
    }

    fn apply_transport(
        &mut self,
        update: &TransportUpdate,
        transport: &mut Transport,
    ) -> Option<Command> {
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
            // The rebuild keeps the row and its guard: it is neither a load
            // that started nor a row that would not play.
            TransportUpdate::Reconnecting => None,
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

    fn apply_remote(
        &mut self,
        update: &RemoteUpdate,
        transport: &mut Transport,
    ) -> Option<Command> {
        match update {
            // The bridge loads next, so adopting asks for nothing.
            RemoteUpdate::Landed(landing) => {
                self.adopt(landing, transport);
                None
            }
            // The sound belongs to another device now, so a running player is
            // paused rather than stopped: the rows, the index and the position
            // stay, and a local resume continues.
            RemoteUpdate::Lost { epoch } => {
                self.lost_epoch = self.lost_epoch.max(*epoch);
                matches!(
                    transport.state,
                    TransportState::Playing | TransportState::Loading
                )
                .then_some(Command::Pause)
            }
            RemoteUpdate::Labelled(entries) => {
                self.relabel(entries);
                // The bar names the loaded row itself, so a row that was only a
                // URI when it landed stays that way until the label is copied
                // across.
                self.rename_loaded(transport);
                None
            }
            // An order is applied by the frame, which owns the clock it needs.
            RemoteUpdate::Order { .. } => None,
        }
    }

    /// Adopts a hand-over: its rows, its current row and the state the landed
    /// track starts in.
    ///
    /// `false` when the landing names an activation a loss has already
    /// overtaken, or names no row; nothing changes then, so the local transport
    /// is not set to a track another device is playing.
    pub fn adopt(&mut self, landing: &Landing, transport: &mut Transport) -> bool {
        if landing.epoch <= self.lost_epoch {
            return false;
        }
        // The landing names rows only by URI, and its report shapes exactly
        // those rows as entries, so the queue starts on what the phone was told.
        let report = landing.report();
        let Some(landed) = report.track else {
            return false;
        };

        self.tracks = report
            .prev
            .into_iter()
            .chain([landed.clone()])
            .chain(report.next)
            .collect();
        self.skips = 0;
        self.queued = 0;
        self.queue_revision.clear();
        self.selection.clear();
        self.refocus = None;
        self.label_asked.clear();
        // A mode the landing names is taken; one it names not at all is the mode
        // the queue already has, not a mode read back as off.
        let (context, track) = self.repeat.flags();
        self.repeat = Repeat::from_flags(
            landing.repeat_context.unwrap_or(context),
            landing.repeat_track.unwrap_or(track),
        );
        self.shuffle = landing.shuffle.unwrap_or(self.shuffle);
        self.origin = landing.context.clone();
        self.index = Some(landing.index);
        if self.shuffle {
            // Only the rows after the landed one are drawn, so it stays the
            // current row, and the order the phone had is what turning shuffle
            // off puts back.
            self.snapshot = self.uris();
            self.draw_after(landing.index + 1);
        } else {
            self.snapshot.clear();
        }
        // The bridge loads the landed track, so the events of the previous
        // track's load are no longer this queue's.
        self.awaiting_start = true;
        transport.landed(&landed, landing.position_ms, landing.playing);
        true
    }

    /// Replaces the rows with the window the phone published around the current
    /// one, and keeps the revision it came from.
    ///
    /// The window is anchored on the current row, so there has to be one. A
    /// separator names no track and a row the phone left empty is the current
    /// row; every other row has to be a Spotify uri, and a window past
    /// [`MAX_WINDOW_ROWS`] is not one. A refusal changes nothing.
    pub fn replace_window(
        &mut self,
        prev: &[String],
        next: &[String],
        revision: String,
    ) -> Result<(), WindowRefusal> {
        let Some(current) = self.current().cloned() else {
            return Err(WindowRefusal::NoCurrent);
        };
        let before = window_rows(prev, &current)?;
        let after = window_rows(next, &current)?;
        if before.len().saturating_add(after.len()) > MAX_WINDOW_ROWS {
            return Err(WindowRefusal::TooLarge);
        }

        let index = before.len();
        let mut rows = before;
        rows.push(current);
        rows.extend(after);

        self.tracks = rows;
        self.index = Some(index);
        self.queue_revision = revision;
        // The phone draws its own block ahead of the first separator, and the
        // rows after the separator are the context that resumes after it.
        self.queued = next
            .iter()
            .position(|uri| uri == DELIMITER_URI)
            .unwrap_or(0);
        self.skips = 0;
        self.selection.clear();
        self.refocus = None;
        self.label_asked.clear();
        // The drawn order belonged to the rows the window just replaced.
        self.snapshot.clear();
        Ok(())
    }

    /// The rows after the current one that are still only a URI, asked for once.
    pub fn label_wants(&mut self) -> Option<Command> {
        let mut uris = Vec::new();
        for entry in self.tracks.iter().skip(self.first_upcoming()) {
            if entry.named || self.label_asked.contains(&entry.uri) {
                continue;
            }
            uris.push(entry.uri.clone());
            if uris.len() == LABEL_BATCH {
                break;
            }
        }
        if uris.is_empty() {
            return None;
        }
        self.label_asked.extend(uris.iter().cloned());
        Some(Command::LabelRows { uris })
    }

    /// Names the rows a label answered for, leaving the order and the current
    /// row alone.
    pub fn relabel(&mut self, entries: &[TrackEntry]) {
        for named in entries {
            for row in &mut self.tracks {
                if row.uri == named.uri {
                    *row = named.clone();
                }
            }
        }
    }

    /// Copies a newly named current row into the playbar.
    fn rename_loaded(&self, transport: &mut Transport) {
        let Some(current) = self.current() else {
            return;
        };
        let Some(now) = transport.now_playing.as_mut() else {
            return;
        };
        if now.uri != current.uri {
            return;
        }
        now.title = current.title.clone();
        now.artists = current.artists.clone();
        now.cover = current.cover.clone();
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
    /// The index is the position in the list `start` was given. Advancing alone
    /// only moves the current index, so the rows keep their positions; a draw,
    /// from turning shuffle on or a wrap, rebuilds the rows after the current
    /// one, so an upcoming index names a row only until the next draw.
    pub fn upcoming(&self) -> impl ExactSizeIterator<Item = (usize, &TrackEntry)> + '_ {
        let first = self.first_upcoming();
        self.tracks.iter().enumerate().skip(first)
    }

    /// The entries before the current one, at most `count`, nearest last.
    pub fn history(&self, count: usize) -> impl ExactSizeIterator<Item = &TrackEntry> + '_ {
        let end = self.index.unwrap_or(0);
        let start = end.saturating_sub(count);
        self.tracks[start..end].iter()
    }

    /// The page the current rows came from, or `None` for a list that has none.
    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
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
        let block = self.first_upcoming()..self.queued_end();
        let removed = rows.iter().filter(|row| block.contains(row)).count();
        for row in rows.iter().rev() {
            self.tracks.remove(*row);
        }
        self.queued = self.queued.saturating_sub(removed);
        self.clamp_queued();
        self.forget_revision();
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
        self.clamp_queued();
        self.forget_revision();

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
        self.clamp_queued();
        self.forget_revision();
        self.selection.open_gap(boundary, count);

        Some(boundary..boundary + count)
    }

    /// Adds `entries` after the last entry, unpicked, and says where they landed.
    /// An add has no position to choose, so an empty queue takes them at the
    /// first row and is left armed.
    pub fn append(&mut self, entries: Vec<TrackEntry>) -> Option<Range<usize>> {
        self.insert_at(self.tracks.len(), entries)
    }

    /// Adds `entries` to the queue the phone sent, after the rows it queued
    /// before, and says where they landed.
    ///
    /// The block plays before the context resumes, so it sits right after the
    /// current row; each add goes behind the earlier ones so a run of them keeps
    /// the order the phone sent them in. With nothing playing there is no row to
    /// queue behind, so the add joins the end.
    pub fn add_next(&mut self, entries: Vec<TrackEntry>) -> Option<Range<usize>> {
        if self.index.is_none() {
            return self.append(entries);
        }
        let at = self.first_upcoming().saturating_add(self.queued);
        let landed = self.insert_at(at, entries)?;
        self.queued = self.queued.saturating_add(landed.len());
        Some(landed)
    }

    /// Adds one track the phone queued, by uri, where [`Self::add_next`] puts
    /// it.
    ///
    /// A uri the queue cannot hold is refused here rather than becoming a row
    /// that fails where it is played.
    pub fn add_next_uri(&mut self, uri: &str) -> Option<Range<usize>> {
        if !spotify_uri(uri) {
            return None;
        }
        self.add_next(vec![unnamed(uri)])
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
        self.clamp_queued();
        self.forget_revision();
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
        self.draw_if_undrawn(index);
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

    /// Sets the repeat mode the phone named, which names a state and not a step.
    pub fn set_repeat(&mut self, mode: Repeat) {
        self.repeat = mode;
    }

    /// Steps to the repeat mode after this one.
    pub fn cycle_repeat(&mut self) {
        self.set_repeat(self.repeat.next());
    }

    /// Whether shuffle is on.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// The newest queue revision the phone published, empty when it published
    /// none.
    pub fn queue_revision(&self) -> &str {
        &self.queue_revision
    }

    /// Applies the saved modes, seeding shuffle only while nothing is loaded.
    ///
    /// Returns whether shuffle was applied: `false` means repeat took and
    /// shuffle did not.
    ///
    /// Shuffle seeded on an empty queue leaves the snapshot empty, so the first
    /// play draws the coming rows. Rows already present keep their order: a
    /// saved flag must not permute a list someone is looking at.
    pub fn restore_modes(&mut self, repeat: Repeat, shuffle: bool) -> bool {
        self.repeat = repeat;
        if !self.tracks.is_empty() {
            return false;
        }
        self.shuffle = shuffle;
        true
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
        self.queued = 0;
        self.forget_revision();
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
        self.draw_after(self.first_upcoming());
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
        // A caller passing a short order would silently leave rows behind.
        debug_assert_eq!(order.len(), slots.len());
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

    /// Plays what `hop` names. A wrap starts a new lap, so the rows after it are
    /// drawn again.
    fn follow(&mut self, hop: Hop, transport: &mut Transport) -> Option<Command> {
        let index = match hop {
            Hop::Next => self.index? + 1,
            Hop::Wrap => 0,
            Hop::Replay => self.index?,
        };
        let command = self.goto(index, transport);
        if hop == Hop::Wrap {
            self.reshuffle_tail();
        }
        command
    }

    /// Draws a new order for the rows after the current one.
    fn reshuffle_tail(&mut self) {
        if self.shuffle {
            self.draw_after(self.first_upcoming());
        }
    }

    /// Draws a new order for the rows from `first` on.
    fn draw_after(&mut self, first: usize) {
        let seed = shuffle::splitmix(&mut self.rng);
        let order = shuffle::order(self.tracks.len() - first, seed);
        self.reorder_tail(first, &order);
        // A draw moves every row after `first`, so the block the phone queued
        // ahead of the current row is no longer where it was, and the new order
        // is not the window the phone published.
        self.queued = 0;
        self.forget_revision();
    }

    /// Draws the rows after `row` for a first play that no `start` draws for.
    ///
    /// An armed queue with shuffle on holds its rows in list order: the row
    /// about to play keeps its place, the rows before it stay history, and the
    /// rest are drawn, with the list's order kept for turning shuffle off.
    fn draw_if_undrawn(&mut self, row: usize) {
        // A started queue has already drawn and an empty one has nothing to
        // draw: the callers exclude both, so these two guards are defences.
        if !self.shuffle
            || self.index.is_some()
            || !self.snapshot.is_empty()
            || self.tracks.is_empty()
        {
            return;
        }
        self.snapshot = self.uris();
        self.draw_after(row + 1);
        // The draw moved the rows the pick names onto other tracks.
        self.selection.clear();
        self.refocus = None;
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
        let now = {
            let entry = self.tracks.get(index)?;
            NowPlaying::from(entry)
        };
        self.land_on(index);
        self.index = Some(index);
        self.awaiting_start = true;
        Some(transport.play(now))
    }

    /// Keeps the queued block to what is still ahead of the row the queue moves
    /// onto.
    ///
    /// A move into the block leaves its tail; a move past it, or back out of it,
    /// leaves nothing; a restart of the same row leaves it alone.
    fn land_on(&mut self, index: usize) {
        let Some(from) = self.index else {
            self.queued = 0;
            return;
        };
        self.queued = match index.cmp(&from) {
            std::cmp::Ordering::Greater => self.queued_end().saturating_sub(index + 1),
            // A restart loads the same row again, so the block has not moved.
            std::cmp::Ordering::Equal => return,
            std::cmp::Ordering::Less => 0,
        };
    }

    /// One past the queued block's last row.
    fn queued_end(&self) -> usize {
        self.first_upcoming().saturating_add(self.queued)
    }

    /// Keeps the block to the rows still ahead of the current one, after an edit
    /// has removed or moved rows around it.
    fn clamp_queued(&mut self) {
        let ahead = self.tracks.len().saturating_sub(self.first_upcoming());
        self.queued = self.queued.min(ahead);
    }

    /// Forgets the revision the phone published, because an edit has made these
    /// rows something other than the window it named.
    fn forget_revision(&mut self) {
        self.queue_revision.clear();
    }
}

/// The rows of one half of a phone's window, cleaned and made into entries.
///
/// A separator is dropped rather than refused: it is the phone's own punctuation
/// between blocks. A row that names a uri the worker can be handed is kept as it
/// is; a local file is one, and a row that cannot be played fails where it is
/// played.
fn window_rows(rows: &[String], current: &TrackEntry) -> Result<Vec<TrackEntry>, WindowRefusal> {
    let mut entries = Vec::with_capacity(rows.len());
    for uri in rows {
        if uri == DELIMITER_URI {
            continue;
        }
        // The phone sends an empty row for the track it is dragging, so the row
        // it stands for is the one that is playing.
        if uri.is_empty() {
            entries.push(current.clone());
        } else if spotify_uri(uri) {
            entries.push(unnamed(uri));
        } else {
            return Err(WindowRefusal::BadRow);
        }
    }
    Ok(entries)
}

/// A row that is only a uri, waiting for its names.
fn unnamed(uri: &str) -> TrackEntry {
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
        named: false,
        cover: None,
    }
}

/// The kinds a queue row can carry: the phone sends rows it can play.
const ROW_KINDS: [&str; 3] = ["track", "episode", "local"];

/// Whether `uri` is a row this queue holds: a Spotify uri of a kind that names a
/// playable thing, with something naming it.
///
/// The window is built in `takofy-ui`, which cannot reach librespot's parser, and
/// a row only has to be one the worker can be handed; what is refused is a row
/// that names no track at all, which would fail only once someone played it.
fn spotify_uri(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("spotify:") else {
        return false;
    };
    let Some((kind, id)) = rest.split_once(':') else {
        return false;
    };
    ROW_KINDS.contains(&kind) && !id.is_empty()
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
            artist_names: Vec::new(),
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
        queue.start(&tracks, index, None, &mut transport);
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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    }

    #[test]
    fn ended_at_the_last_entry_is_idle_with_no_command() {
        let tracks = tracks(1);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), None);
        assert!(!queue.can_next());
    }

    #[test]
    fn unavailable_advances_and_the_chain_stops_at_the_cap() {
        let tracks = tracks(12);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
        clear_guard(&mut queue, &mut transport);
        assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));

        assert_eq!(queue.previous(4_000, &mut transport), Some(play("t0")));
    }

    #[test]
    fn previous_restarts_past_the_threshold() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 1, None, &mut transport),
            Some(play("t1"))
        );
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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
        clear_guard(&mut queue, &mut transport);

        assert_eq!(queue.previous(1_000, &mut transport), Some(play("t0")));
    }

    #[test]
    fn a_seek_restart_leaves_the_guard_unset() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        assert_eq!(
            queue.start(&tracks, 1, None, &mut transport),
            Some(play("t1"))
        );

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
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );

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
        queue.start(&tracks, index, None, &mut transport);
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
                queue.start(&list, 0, None, &mut transport);
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
        let mut transport = Transport::default();
        for seed in 0..20 {
            for advances in 0..6 {
                let mut queue = shuffled_at(6, 0, seed);
                for _ in 0..advances {
                    assert!(queue.next(&mut transport).is_some());
                }
                let current = queue.current().map(|entry| entry.uri.clone());

                queue.toggle_shuffle();

                assert_eq!(
                    queue.current().map(|entry| entry.uri.clone()),
                    current,
                    "seed {seed} advances {advances}"
                );
                assert_eq!(
                    order(&queue),
                    (0..6).map(|row| format!("t{row}")).collect::<Vec<_>>(),
                    "seed {seed} advances {advances}"
                );
            }
        }
    }

    #[test]
    fn shuffle_off_restores_history_too() {
        let mut queue = started(5, 0);
        let mut transport = Transport::default();
        queue.toggle_shuffle();

        let held = order(&queue);
        let t3 = held
            .iter()
            .position(|uri| uri == "t3")
            .expect("t3 is queued");
        let t1 = held
            .iter()
            .position(|uri| uri == "t1")
            .expect("t1 is queued");
        queue.jump(t3, &mut transport);
        queue.jump(t1, &mut transport);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));

        queue.toggle_shuffle();

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4"]));
        assert_eq!(queue.index, Some(1), "the entry is back on its own row");
        assert_eq!(
            order(&queue)[3],
            "t3",
            "a row that already played is ahead of the current one"
        );
        assert_eq!(queue.previous(0, &mut transport), Some(play("t0")));
    }

    #[test]
    fn shuffle_off_with_duplicate_uris_keeps_every_row_and_the_entry() {
        let list = vec![
            track_by("a", "1"),
            track_by("b", "2"),
            track_by("a", "3"),
            track_by("b", "4"),
            track_by("a", "5"),
        ];
        let mut transport = Transport::default();
        let mut expected: Vec<String> = list.iter().map(|entry| entry.artists.clone()).collect();
        expected.sort();
        for seed in 0..20 {
            for index in 0..list.len() {
                let mut queue = Queue::seeded(seed);
                queue.start(&list, index, None, &mut transport);
                let current = queue.current().map(|entry| entry.artists.clone());

                queue.toggle_shuffle();
                queue.toggle_shuffle();

                assert_eq!(
                    order(&queue),
                    uris(&["a", "b", "a", "b", "a"]),
                    "seed {seed} index {index}"
                );
                let mut held = artists(&queue);
                held.sort();
                assert_eq!(
                    held, expected,
                    "seed {seed} index {index}: a row went missing"
                );
                assert_eq!(
                    queue.current().map(|entry| entry.artists.clone()),
                    current,
                    "seed {seed} index {index}: the current entry changed"
                );
            }
        }
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
    fn restore_modes_on_an_empty_queue_sets_both_and_returns_true() {
        let mut queue = Queue::seeded(9);

        assert!(
            queue.restore_modes(Repeat::All, true),
            "an empty queue takes both saved modes"
        );
        assert_eq!(queue.repeat(), Repeat::All);
        assert!(queue.shuffle());
    }

    #[test]
    fn restore_modes_with_rows_sets_only_repeat_and_returns_false() {
        let mut queue = Queue::seeded(9);
        queue.append(tracks(4));
        let before = order(&queue);

        assert!(
            !queue.restore_modes(Repeat::One, true),
            "rows already hold an order, so the shuffle flag is not applied"
        );
        assert_eq!(queue.repeat(), Repeat::One, "repeat is only a mode");
        assert!(!queue.shuffle(), "the loaded order was not drawn");
        assert!(queue.armed());
        assert_eq!(order(&queue), before);
    }

    #[test]
    fn restore_modes_off_false_on_a_default_queue_changes_nothing() {
        let mut queue = Queue::default();

        assert!(
            queue.restore_modes(Repeat::Off, false),
            "an empty queue takes both saved modes"
        );
        assert_eq!(queue.repeat(), Repeat::Off);
        assert!(!queue.shuffle());
        assert!(queue.tracks.is_empty());
        assert!(queue.snapshot.is_empty());
    }

    #[test]
    fn the_first_play_of_an_armed_restored_shuffle_queue_draws_the_rest() {
        let list = tracks(4);
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(list.clone());
        let mut transport = Transport::default();

        assert_eq!(
            queue.toggle(&mut transport),
            Some(play("t0")),
            "the first row is the user's choice"
        );

        let mut drawn = Queue::seeded(9);
        drawn.start_with_shuffle(&list, 0, None, &mut Transport::default());
        assert_eq!(
            order(&queue),
            order(&drawn),
            "the rest is drawn as a started list draws it"
        );
        assert!(holds_exactly(&queue, 4), "every row is still queued");
        assert!(
            !queue.snapshot.is_empty(),
            "shuffle off needs the list's order back"
        );
    }

    #[test]
    fn shuffle_off_after_an_armed_first_play_restores_list_order() {
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(tracks(4));
        queue.toggle(&mut Transport::default());

        queue.toggle_shuffle();

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3"]));
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("t0"),
            "the playing row keeps its entry"
        );
        assert_eq!(queue.index, Some(0));
    }

    #[test]
    fn a_jump_on_an_armed_restored_shuffle_queue_draws_only_the_rows_after_it() {
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(tracks(4));
        let mut transport = Transport::default();

        assert_eq!(queue.jump(1, &mut transport), Some(play("t1")));

        let order = order(&queue);
        assert_eq!(
            order[..2],
            uris(&["t0", "t1"]),
            "the row jumped to and the ones before it keep their place"
        );
        let mut tail = order[2..].to_vec();
        tail.sort();
        assert_eq!(tail, uris(&["t2", "t3"]), "the rest is a permutation");
        assert_ne!(
            &order[2..],
            &uris(&["t2", "t3"]),
            "a drawn order never repeats the one the rows had"
        );
        assert_eq!(
            queue.snapshot,
            uris(&["t0", "t1", "t2", "t3"]),
            "the snapshot holds the list's order"
        );
        assert_eq!(queue.index, Some(1));
    }

    #[test]
    fn a_one_row_armed_shuffle_queue_plays_its_only_row() {
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(tracks(1));
        let mut transport = Transport::default();

        assert_eq!(queue.toggle(&mut transport), Some(play("t0")));

        assert_eq!(order(&queue), uris(&["t0"]));
        assert_eq!(queue.index, Some(0));
    }

    #[test]
    fn a_jump_to_the_last_row_of_an_armed_shuffle_queue_plays_it() {
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(tracks(4));
        let mut transport = Transport::default();

        assert_eq!(queue.jump(3, &mut transport), Some(play("t3")));

        assert_eq!(queue.index, Some(3));
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("t3"),
            "the row jumped to is the one that plays"
        );
        assert_eq!(
            order(&queue),
            uris(&["t0", "t1", "t2", "t3"]),
            "there is no tail to draw, so every row keeps its place"
        );
    }

    #[test]
    fn the_first_play_of_a_drawn_queue_clears_the_picks() {
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(tracks(4));
        queue.press(2, Press::Replace);
        queue.refocus_at(3);
        assert_eq!(picked(&queue), vec![2]);
        let mut transport = Transport::default();

        queue.toggle(&mut transport);

        assert!(
            picked(&queue).is_empty(),
            "the picks name rows that now hold other tracks"
        );
        assert!(queue.refocus().is_none());
    }

    #[test]
    fn an_armed_queue_with_shuffle_off_is_not_drawn_on_first_play() {
        let mut queue = Queue::seeded(9);
        queue.append(tracks(4));
        let mut transport = Transport::default();

        assert_eq!(queue.toggle(&mut transport), Some(play("t0")));

        assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3"]));
        assert!(queue.snapshot.is_empty(), "nothing was drawn");
    }

    #[test]
    fn a_second_toggle_or_jump_after_the_first_play_does_not_redraw() {
        let mut queue = Queue::seeded(9);
        queue.restore_modes(Repeat::Off, true);
        queue.append(tracks(4));
        let mut transport = Transport::default();
        queue.toggle(&mut transport);
        let before = order(&queue);
        let rng = queue.rng;

        queue.toggle(&mut transport);
        queue.jump(2, &mut transport);

        assert_eq!(order(&queue), before, "the draw happens once");
        assert_eq!(queue.rng, rng, "and the generator is not advanced again");
    }

    #[test]
    fn shuffle_toggled_on_an_armed_queue_is_not_drawn_twice() {
        let mut queue = Queue::seeded(9);
        queue.append(tracks(5));
        queue.toggle_shuffle();
        let before = order(&queue);
        let rng = queue.rng;
        let mut transport = Transport::default();

        queue.toggle(&mut transport);

        assert_eq!(order(&queue), before, "the first play kept the drawn order");
        assert_eq!(queue.rng, rng, "and drew nothing itself");
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

    /// The band's shuffle is the mode switch, not a one-off: it starts the list
    /// in a new order and leaves shuffle on for whatever plays next.
    #[test]
    fn start_with_shuffle_turns_the_mode_on_and_keeps_it_on() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        let list = tracks(8);
        assert!(!queue.shuffle());

        assert_eq!(
            queue.start_with_shuffle(&list, 0, None, &mut transport),
            Some(play("t0"))
        );

        assert!(queue.shuffle(), "the switch stayed on");
        assert_eq!(queue.index, Some(0), "the first row is what plays");
        assert_ne!(
            &order(&queue)[1..],
            &uris(&["t1", "t2", "t3", "t4", "t5", "t6", "t7"]),
            "the coming rows are not the list's order"
        );
        assert!(holds_exactly(&queue, 8));
    }

    #[test]
    fn start_with_shuffle_on_plays_the_chosen_row_first_and_shuffles_the_rest() {
        let mut queue = Queue::seeded(13);
        queue.append(tracks(2));
        queue.toggle_shuffle();
        let mut transport = Transport::default();
        let list = tracks(6);

        assert_eq!(
            queue.start(&list, 3, None, &mut transport),
            Some(play("t3"))
        );

        assert_eq!(queue.index, Some(0), "the clicked row is what plays");
        assert_eq!(order(&queue)[0], "t3");
        assert!(holds_exactly(&queue, 6));
        assert_ne!(
            &order(&queue)[1..],
            &uris(&["t0", "t1", "t2", "t4", "t5"]),
            "the rest kept the list's order"
        );
    }

    #[test]
    fn shuffle_and_repeat_survive_a_new_start() {
        let mut queue = started(3, 0);
        let mut transport = Transport::default();
        queue.cycle_repeat();
        queue.toggle_shuffle();

        let list = tracks(2);
        assert_eq!(
            queue.start(&list, 1, None, &mut transport),
            Some(play("t1"))
        );

        assert!(queue.shuffle());
        assert_eq!(queue.repeat(), Repeat::All);
    }

    #[test]
    fn a_second_start_replaces_the_snapshot() {
        let mut queue = Queue::seeded(3);
        let mut transport = Transport::default();
        queue.toggle_shuffle();
        queue.start(&tracks(4), 0, None, &mut transport);

        let second = vec![track("x"), track("y"), track("z")];
        assert_eq!(
            queue.start(&second, 1, None, &mut transport),
            Some(play("y"))
        );
        queue.toggle_shuffle();

        assert_eq!(
            order(&queue),
            uris(&["x", "y", "z"]),
            "the second list's order, not the first list's"
        );
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("y"));
    }

    #[test]
    fn a_bad_index_start_changes_nothing_with_shuffle_on() {
        let mut queue = shuffled_at(4, 1, 6);
        let mut transport = Transport::default();
        let before = (
            order(&queue),
            queue.index,
            queue.awaiting_start,
            queue.skips,
            queue.snapshot.clone(),
        );

        assert_eq!(queue.start(&tracks(2), 5, None, &mut transport), None);
        assert_eq!(queue.start(&[], 0, None, &mut transport), None);

        assert_eq!(
            (
                order(&queue),
                queue.index,
                queue.awaiting_start,
                queue.skips,
                queue.snapshot.clone(),
            ),
            before,
            "a refused start must not draw, disarm or reseed anything"
        );
    }

    #[test]
    fn wrapping_under_shuffle_reshuffles_the_tail_and_keeps_row_zero() {
        let mut queue = seeded_started(6, 0, 21);
        let mut transport = Transport::default();
        queue.cycle_repeat();
        queue.toggle_shuffle();
        for _ in 0..5 {
            assert!(queue.next(&mut transport).is_some());
        }
        assert_eq!(queue.index, Some(5), "the lap is at its last row");
        let row_zero = order(&queue)[0].clone();
        let tail = order(&queue)[1..].to_vec();

        assert_eq!(queue.next(&mut transport), Some(play(&row_zero)));

        assert_eq!(queue.index, Some(0));
        assert_eq!(order(&queue)[0], row_zero, "a wrap must not move row zero");
        assert_ne!(
            order(&queue)[1..],
            tail[..],
            "lap two replayed lap one's order"
        );
        assert!(holds_exactly(&queue, 6));
    }

    /// A queue of `count` tracks started at `index` from `seed`, not yet shuffled.
    fn seeded_started(count: usize, index: usize, seed: u64) -> Queue {
        let list = tracks(count);
        let mut queue = Queue::seeded(seed);
        let mut transport = Transport::default();
        queue.start(&list, index, None, &mut transport);
        queue
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
            (TransportState::Reconnecting, None),
            (TransportState::Failed(Failure::Other), Some(play("t0"))),
        ];
        for (state, expected) in cases {
            let tracks = tracks(2);
            let mut queue = Queue::default();
            let mut transport = Transport::default();
            queue.start(&tracks, 0, None, &mut transport);
            transport.state = state;
            assert_eq!(queue.toggle(&mut transport), expected, "{state:?}");
        }
    }

    /// A rebuild is not a load and not a skip: the guard and the streak stand
    /// still, so the row that failed does not advance on the rebuild's account.
    #[test]
    fn reconnecting_asks_for_nothing_and_keeps_the_guard() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
        let before = (queue.index, queue.awaiting_start, queue.skips);

        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Reconnecting),
            None
        );
        assert_eq!(
            (queue.index, queue.awaiting_start, queue.skips),
            before,
            "a rebuild is neither a load that started nor a skipped row"
        );
    }

    #[test]
    fn toggle_while_reconnecting_asks_for_nothing() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
        transport.state = TransportState::Reconnecting;

        assert_eq!(queue.toggle(&mut transport), None);
        assert_eq!(queue.index, Some(0), "the row stays current");
    }

    #[test]
    fn an_unavailable_after_reconnecting_still_advances() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
        clear_guard(&mut queue, &mut transport);
        feed(&mut queue, &mut transport, TransportUpdate::Reconnecting);

        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            Some(play("t1"))
        );
        assert_eq!(queue.skips, 1, "a row that would not play still counts");
    }

    #[test]
    fn a_failed_after_reconnecting_only_clears_the_guard() {
        let tracks = tracks(2);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&tracks, 0, None, &mut transport),
            Some(play("t0"))
        );
        feed(&mut queue, &mut transport, TransportUpdate::Reconnecting);
        assert!(
            queue.awaiting_start,
            "a rebuild says nothing about the load in flight"
        );

        assert_eq!(
            feed(
                &mut queue,
                &mut transport,
                TransportUpdate::Failed(Failure::Reconnect)
            ),
            None
        );
        assert!(!queue.awaiting_start);
        assert_eq!(queue.index, Some(0));
        assert_eq!(queue.skips, 0);
    }

    #[test]
    fn replacing_the_queue_mid_play_leaves_no_stale_guard() {
        let first = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        assert_eq!(
            queue.start(&first, 0, None, &mut transport),
            Some(play("t0"))
        );
        clear_guard(&mut queue, &mut transport);

        let second = vec![track("x"), track("y")];
        assert_eq!(
            queue.start(&second, 0, None, &mut transport),
            Some(play("x"))
        );
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
        assert_eq!(queue.start(&all, 0, None, &mut transport), Some(play("t0")));
        clear_guard(&mut queue, &mut transport);

        let playlists = Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
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
            queue.start(&duplicates, 0, None, &mut transport),
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
        assert_eq!(queue.start(&tracks, 2, None, &mut transport), None);
        assert_eq!(queue.start(&[], 0, None, &mut transport), None);
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
        queue.start(&tracks, 0, None, &mut transport);
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

        let command = queue.start(&[episode.entry()], 0, None, &mut transport);

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
        entry.cover = Some(takofy_model::CoverSet::single(cover.clone()));
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        let _ = queue.start(&[entry], 0, None, &mut transport);

        assert_eq!(
            transport.now_playing.expect("a named track").cover,
            Some(takofy_model::CoverSet::single(cover))
        );
    }

    #[test]
    fn goto_passes_the_entrys_artists_into_now_playing() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        let _ = queue.start(&[track_by("a", "A Band")], 0, None, &mut transport);

        assert_eq!(
            transport.now_playing.expect("a named track").artists,
            "A Band"
        );
    }

    #[test]
    fn goto_passes_the_entrys_album_and_first_artist_uris() {
        let mut entry = track_by("a", "A Band, Another Band");
        entry.album_uri = Some("spotify:album:one".to_owned());
        entry.artist_uris = vec![
            "spotify:artist:first".to_owned(),
            "spotify:artist:second".to_owned(),
        ];
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        let _ = queue.start(&[entry], 0, None, &mut transport);

        let now = transport.now_playing.expect("a named track");
        assert_eq!(now.album_uri.as_deref(), Some("spotify:album:one"));
        assert_eq!(now.artist_uri.as_deref(), Some("spotify:artist:first"));
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
        queue.start(&tracks, 2, None, &mut transport);

        assert_eq!(upcoming(&queue), Vec::new());
        assert!(!queue.can_next(), "nothing upcoming is exactly `!can_next`");
    }

    #[test]
    fn upcoming_lists_what_follows_the_current_entry_with_absolute_indices() {
        let tracks = tracks(5);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 1, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 2, None, &mut transport);

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
        queue.start(&duplicates, 0, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut transport);
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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 1, None, &mut transport);
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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut transport);

        assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
        assert!(queue.upcoming().next().is_none());
        assert!(!queue.can_next());
    }

    #[test]
    fn jump_back_to_the_first_entry_still_restarts_it() {
        let tracks = tracks(3);
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.start(&tracks, 2, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 1, None, &mut transport);
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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&entries, 0, None, &mut transport);

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

    /// One row the phone sends: a uri and nothing else.
    fn word(name: &str) -> String {
        format!("spotify:track:{name}")
    }

    /// A window's rows, each named the way the phone names one.
    fn window(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|row| word(row)).collect()
    }

    #[test]
    fn add_next_lands_after_the_current_row_and_after_earlier_adds() {
        let mut queue = started(3, 0);

        assert_eq!(queue.add_next(vec![track("x")]), Some(1..2));
        assert_eq!(queue.add_next(vec![track("y")]), Some(2..3));

        assert_eq!(order(&queue), uris(&["t0", "x", "y", "t1", "t2"]));
        assert_eq!(queue.queued, 2);
        assert_eq!(
            upcoming(&queue),
            vec![
                (1, "x".to_owned()),
                (2, "y".to_owned()),
                (3, "t1".to_owned()),
                (4, "t2".to_owned()),
            ],
            "the context resumes after the block"
        );
    }

    #[test]
    fn queued_shrinks_when_the_queue_advances_past_it() {
        let mut queue = started(3, 0);
        let mut transport = Transport::default();
        queue.add_next(vec![track("x")]);
        queue.add_next(vec![track("y")]);

        assert_eq!(queue.next(&mut transport), Some(play("x")));
        assert_eq!(queue.queued, 1, "x was the row the phone queued first");
        assert_eq!(queue.next(&mut transport), Some(play("y")));
        assert_eq!(queue.queued, 0);
        assert_eq!(queue.next(&mut transport), Some(play("t1")));
        assert_eq!(queue.queued, 0, "a hop past the block cannot go negative");
    }

    #[test]
    fn a_skip_past_the_queued_block_clears_it() {
        let mut queue = started(4, 0);
        let mut transport = Transport::default();
        queue.add_next(vec![track("x")]);
        assert_eq!(queue.queued, 1);

        assert_eq!(queue.jump(2, &mut transport), Some(play("t1")));
        assert_eq!(queue.queued, 0, "the block is behind the row that plays");

        queue.add_next(vec![track("w")]);
        assert_eq!(
            order(&queue),
            uris(&["t0", "x", "t1", "w", "t2", "t3"]),
            "an add after a skip past the block sits right after the current row"
        );
    }

    #[test]
    fn a_skip_inside_the_block_shrinks_it() {
        let mut queue = started(3, 0);
        let mut transport = Transport::default();
        queue.add_next(vec![track("x")]);
        queue.add_next(vec![track("y")]);
        assert_eq!(queue.queued, 2);

        assert_eq!(queue.jump(1, &mut transport), Some(play("x")));
        assert_eq!(queue.queued, 1, "the row that played left the block");

        queue.add_next(vec![track("w")]);
        assert_eq!(
            order(&queue),
            uris(&["t0", "x", "y", "w", "t1", "t2"]),
            "the rest of the block still plays before the context"
        );
    }

    #[test]
    fn a_move_back_out_of_the_block_clears_it() {
        let mut queue = started(4, 1);
        let mut transport = Transport::default();
        queue.add_next(vec![track("x")]);
        assert_eq!(queue.queued, 1);

        // Back to a history row: the block is no longer counted from here.
        assert_eq!(queue.previous(0, &mut transport), Some(play("t0")));
        assert_eq!(queue.queued, 0);

        queue.add_next(vec![track("w")]);
        assert_eq!(
            order(&queue),
            uris(&["t0", "w", "t1", "x", "t2", "t3"]),
            "an add goes right after the row that plays, not into the old block"
        );
    }

    #[test]
    fn removing_rows_clamps_the_queued_block() {
        let mut queue = started(3, 0);
        queue.add_next(vec![track("x")]);
        queue.add_next(vec![track("y")]);
        assert_eq!(queue.queued, 2);

        let gone = queue.entries_at(&[2]);
        assert_eq!(
            queue.remove_rows(&[2], &gone),
            Some(1),
            "the second block row"
        );
        assert_eq!(queue.queued, 1, "the block counts the rows that remain");

        queue.add_next(vec![track("w")]);
        assert_eq!(
            order(&queue),
            uris(&["t0", "x", "w", "t1", "t2"]),
            "an add goes after the rows that are still queued"
        );
    }

    #[test]
    fn start_adopt_and_a_shuffle_toggle_reset_the_queued_block() {
        let mut queue = started(3, 0);
        let mut transport = Transport::default();
        queue.add_next(vec![track("x")]);

        queue.toggle_shuffle();
        assert_eq!(queue.queued, 0, "the draw moved the rows the block named");

        queue.add_next(vec![track("y")]);
        queue.start(&tracks(3), 0, None, &mut transport);
        assert_eq!(queue.queued, 0, "a new list has no phone block");

        queue.add_next(vec![track("z")]);
        assert!(queue.adopt(&landing(&["a", "b"], 0), &mut transport));
        assert_eq!(queue.queued, 0, "a landing replaces the rows");
    }

    #[test]
    fn replace_window_keeps_the_current_row_and_puts_the_index_after_prev() {
        let mut queue = started(2, 1);

        assert_eq!(
            queue.replace_window(&window(&["a", "b"]), &window(&["c"]), "rev-1".to_owned()),
            Ok(())
        );

        assert_eq!(
            order(&queue),
            vec![word("a"), word("b"), "t1".to_owned(), word("c"),],
            "the window sits around the row that is playing"
        );
        assert_eq!(queue.index, Some(2));
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
    }

    #[test]
    fn replace_window_replaces_everything_else() {
        let mut queue = started(6, 2);

        assert_eq!(
            queue.replace_window(&window(&["a"]), &window(&["b"]), "rev-1".to_owned()),
            Ok(())
        );

        assert_eq!(order(&queue), vec![word("a"), "t2".to_owned(), word("b")]);
        assert_eq!(
            queue.index,
            Some(1),
            "the index sits after the rows before it"
        );
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("t2"),
            "the row that is playing stays current"
        );
        assert_eq!(
            queue
                .history(usize::MAX)
                .map(|entry| entry.uri.as_str())
                .collect::<Vec<_>>(),
            vec!["spotify:track:a"],
            "the rows before the current one are the phone's alone"
        );
        assert_eq!(
            queue
                .upcoming()
                .map(|(_, entry)| entry.uri.as_str())
                .collect::<Vec<_>>(),
            vec!["spotify:track:b"],
            "and the rows after it are the phone's alone too"
        );
        assert_eq!(
            queue.tracks.len(),
            3,
            "the five rows the window did not name are gone"
        );
    }

    #[test]
    fn replace_window_drops_delimiter_rows_from_both_lists() {
        let mut queue = started(1, 0);
        let mut prev = window(&["a", "b"]);
        prev.insert(1, DELIMITER_URI.to_owned());
        let mut next = window(&["c"]);
        next.insert(0, DELIMITER_URI.to_owned());

        assert_eq!(
            queue.replace_window(&prev, &next, "rev-1".to_owned()),
            Ok(())
        );

        assert_eq!(
            order(&queue),
            vec![word("a"), word("b"), "t0".to_owned(), word("c")]
        );
        assert_eq!(queue.index, Some(2), "a separator is not a row");
    }

    #[test]
    fn an_empty_uri_row_becomes_a_copy_of_the_current_row() {
        let mut queue = started(2, 1);

        assert_eq!(
            queue.replace_window(&[String::new()], &[String::new()], "rev-1".to_owned()),
            Ok(())
        );

        assert_eq!(order(&queue), uris(&["t1", "t1", "t1"]));
        assert_eq!(queue.index, Some(1));
    }

    #[test]
    fn an_empty_window_leaves_only_the_current_row() {
        let mut queue = started(3, 1);

        assert_eq!(queue.replace_window(&[], &[], "rev-1".to_owned()), Ok(()));

        assert_eq!(order(&queue), uris(&["t1"]));
        assert_eq!(queue.index, Some(0));
        assert!(!queue.can_next(), "nothing follows the current row");
    }

    #[test]
    fn an_unparsable_row_refuses_and_changes_nothing() {
        let mut queue = started(3, 1);
        let before = order(&queue);
        let good = word("a");
        let bad = [
            "https://example.test/row".to_owned(),
            "t2".to_owned(),
            "spotify:track:".to_owned(),
            "spotify::x".to_owned(),
            "spotify:".to_owned(),
            "spotify:foo:bar".to_owned(),
        ];

        for uri in bad {
            // A bad row in either list refuses the whole window, so a caller
            // that applies one list before validating the other cannot keep
            // the half it already wrote.
            for (prev, next) in [
                (vec![uri.clone()], vec![good.clone()]),
                (vec![good.clone()], vec![uri.clone()]),
            ] {
                assert_eq!(
                    queue.replace_window(&prev, &next, "rev-1".to_owned()),
                    Err(WindowRefusal::BadRow),
                    "{uri}"
                );
                assert_eq!(order(&queue), before, "{uri}");
                assert_eq!(queue.index, Some(1), "{uri}");
                assert!(queue.queue_revision.is_empty(), "{uri} stored a revision");
            }
        }
    }

    #[test]
    fn an_oversized_window_refuses_and_changes_nothing() {
        let mut queue = started(2, 0);
        let before = order(&queue);
        // One row past the bound, split so neither list alone is too large.
        let half = MAX_WINDOW_ROWS / 2;
        let rows = |count: usize, prefix: &str| -> Vec<String> {
            (0..count)
                .map(|row| word(&format!("{prefix}{row}")))
                .collect()
        };
        let prev = rows(half, "p");
        let next = rows(half + 1, "n");

        assert_eq!(
            queue.replace_window(&prev, &next, "rev-1".to_owned()),
            Err(WindowRefusal::TooLarge)
        );
        assert_eq!(order(&queue), before);
        assert_eq!(queue.index, Some(0));
        assert!(queue.queue_revision.is_empty());

        assert_eq!(
            queue.replace_window(&prev, &next[..half], "rev-1".to_owned()),
            Ok(()),
            "the bound itself is a window"
        );
    }

    #[test]
    fn replace_window_with_no_current_entry_is_refused() {
        let mut queue = armed(3);
        let before = order(&queue);

        assert_eq!(
            queue.replace_window(&window(&["a"]), &window(&["b"]), "rev-1".to_owned()),
            Err(WindowRefusal::NoCurrent)
        );

        assert_eq!(order(&queue), before);
        assert!(queue.armed(), "the rows still wait for a first play");
    }

    #[test]
    fn replace_window_stores_the_newest_revision() {
        let mut queue = started(2, 0);

        assert_eq!(
            queue.replace_window(&[], &window(&["a"]), "rev-1".to_owned()),
            Ok(())
        );
        assert_eq!(queue.queue_revision, "rev-1");
        assert_eq!(
            queue.replace_window(&[], &window(&["b"]), "rev-2".to_owned()),
            Ok(())
        );
        assert_eq!(queue.queue_revision, "rev-2", "the newest one is kept");

        assert_eq!(
            queue.replace_window(&["nonsense".to_owned()], &[], "rev-3".to_owned()),
            Err(WindowRefusal::BadRow)
        );
        assert_eq!(queue.queue_revision, "rev-2", "a refusal stored nothing");
    }

    #[test]
    fn replace_window_clears_selection_and_shuffle_snapshot() {
        let mut queue = started(5, 1);
        queue.toggle_shuffle();
        queue.press(2, Press::Replace);
        queue.press(3, Press::Toggle);
        queue.refocus_at(2);
        assert_eq!(picked(&queue), vec![2, 3]);

        assert_eq!(
            queue.replace_window(&window(&["a"]), &window(&["b", "c"]), "rev-1".to_owned()),
            Ok(())
        );

        assert!(
            picked(&queue).is_empty(),
            "a pick cannot survive other rows"
        );
        assert_eq!(queue.refocus(), None);
        assert!(
            queue.snapshot.is_empty(),
            "the drawn order belonged to the rows that went"
        );
    }

    #[test]
    fn replace_window_forgets_the_skip_run_the_queued_block_and_the_label_asks() {
        let mut queue = started(3, 0);
        assert!(queue.add_next_uri(&word("x")).is_some());
        queue.skips = MAX_CONSECUTIVE_SKIPS;
        assert!(
            queue.label_wants().is_some(),
            "the first ask covers the rows as they were"
        );

        assert_eq!(
            queue.replace_window(&[], &window(&["x"]), "rev-1".to_owned()),
            Ok(())
        );

        assert_eq!(queue.queued, 0, "a window with no separator names no block");
        assert_eq!(queue.skips, 0, "a window is not a run of failed loads");
        assert_eq!(
            queue.label_wants(),
            Some(Command::LabelRows {
                uris: window(&["x"])
            }),
            "a row the window replaced is asked about again"
        );
    }

    #[test]
    fn a_pushed_window_seeds_the_queued_block_from_its_delimiter() {
        let mut queue = started(2, 0);
        let mut next = window(&["x", "y"]);
        next.push(DELIMITER_URI.to_owned());
        next.push(word("z"));

        assert_eq!(queue.replace_window(&[], &next, "rev-1".to_owned()), Ok(()));
        assert_eq!(
            queue.queued, 2,
            "the rows before the separator are the phone's own block"
        );

        queue.add_next(vec![track("w")]);
        assert_eq!(
            order(&queue),
            vec![
                "t0".to_owned(),
                word("x"),
                word("y"),
                "w".to_owned(),
                word("z"),
            ],
            "the block plays before the context resumes"
        );
    }

    #[test]
    fn a_pushed_window_without_a_delimiter_queues_nothing() {
        let mut queue = started(2, 0);
        queue.add_next(vec![track("x")]);
        assert_eq!(queue.queued, 1);

        assert_eq!(
            queue.replace_window(&[], &window(&["y", "z"]), "rev-1".to_owned()),
            Ok(())
        );
        assert_eq!(queue.queued, 0, "a window with no separator names no block");

        queue.add_next(vec![track("w")]);
        assert_eq!(
            order(&queue),
            vec!["t0".to_owned(), "w".to_owned(), word("y"), word("z")],
            "an add goes right after the current row"
        );
    }

    #[test]
    fn a_local_start_forgets_the_pushed_revision() {
        let mut queue = started(2, 0);
        let mut transport = Transport::default();
        assert_eq!(
            queue.replace_window(&[], &window(&["a"]), "rev-1".to_owned()),
            Ok(())
        );
        assert_eq!(queue.queue_revision(), "rev-1");

        queue.start(&tracks(2), 0, None, &mut transport);

        assert_eq!(
            queue.queue_revision(),
            "",
            "a list this window started is not the phone's window"
        );
    }

    #[test]
    fn adopting_a_new_landing_forgets_the_pushed_revision() {
        let mut queue = started(2, 0);
        let mut transport = Transport::default();
        assert_eq!(
            queue.replace_window(&[], &window(&["a"]), "rev-1".to_owned()),
            Ok(())
        );

        assert!(queue.adopt(&landing(&["a", "b"], 0), &mut transport));

        assert_eq!(
            queue.queue_revision(),
            "",
            "a landing names the rows again, without the phone's revision"
        );
    }

    #[test]
    fn a_local_edit_forgets_the_pushed_revision() {
        let mut queue = started(4, 1);
        let pushed = window(&["a", "b", "c"]);

        assert_eq!(
            queue.replace_window(&[], &pushed, "rev-1".to_owned()),
            Ok(())
        );
        assert!(queue.add_next_uri(&word("x")).is_some());
        assert_eq!(
            queue.queue_revision(),
            "",
            "an add is not the window the phone sent"
        );

        assert_eq!(
            queue.replace_window(&[], &pushed, "rev-1".to_owned()),
            Ok(())
        );
        let gone = queue.entries_at(&[2]);
        assert_eq!(queue.remove_rows(&[2], &gone), Some(1));
        assert_eq!(
            queue.queue_revision(),
            "",
            "a removal is not that window either"
        );

        assert_eq!(
            queue.replace_window(&[], &pushed, "rev-1".to_owned()),
            Ok(())
        );
        assert!(queue.move_rows(&[1], 4).is_some());
        assert_eq!(queue.queue_revision(), "", "a move is an edit too");

        assert_eq!(
            queue.replace_window(&[], &pushed, "rev-1".to_owned()),
            Ok(())
        );
        assert!(queue.step_rows(&[3], Step::Up).is_some());
        assert_eq!(queue.queue_revision(), "", "a step is an edit too");
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
        queue.start(&tracks, 0, None, &mut transport);

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
        queue.start(&tracks, 0, None, &mut Transport::default());
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

    /// A hand-over landing on `rows` at `index`, playing, at the first
    /// activation epoch.
    fn landing(rows: &[&str], index: usize) -> Landing {
        Landing {
            rows: rows.iter().map(|uri| (*uri).to_owned()).collect(),
            index,
            position_ms: 7_000,
            playing: true,
            context: Some("spotify:playlist:p".to_owned()),
            // A landing that names no mode, so the queue keeps the modes it is
            // in unless a test overrides one.
            shuffle: None,
            repeat_context: None,
            repeat_track: None,
            epoch: 1,
        }
    }

    fn landed(landing: &Landing) -> Update {
        Update::Remote(RemoteUpdate::Landed(landing.clone()))
    }

    fn lost(epoch: u64) -> Update {
        Update::Remote(RemoteUpdate::Lost { epoch })
    }

    fn labelled(entries: Vec<TrackEntry>) -> Update {
        Update::Remote(RemoteUpdate::Labelled(entries))
    }

    #[test]
    fn adopting_replaces_the_rows_and_sits_on_the_landing_index_without_a_play() {
        let mut queue = started(4, 0);
        let mut transport = Transport::default();

        assert!(queue.adopt(&landing(&["a", "b", "c"], 1), &mut transport));
        assert_eq!(order(&queue), uris(&["a", "b", "c"]));
        assert_eq!(queue.index, Some(1));
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("b"));
        assert_eq!(upcoming(&queue), vec![(2, "c".to_owned())]);
    }

    #[test]
    fn adopt_arms_the_load_guard() {
        let mut queue = started(3, 0);
        let mut transport = Transport::default();
        queue.adopt(&landing(&["a", "b", "c"], 1), &mut transport);

        // The landed track's load has not started yet, so a failure naming the
        // previous track must not advance the landed queue.
        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            None
        );
        assert_eq!(queue.index, Some(1));

        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
            Some(play("c"))
        );
    }

    #[test]
    fn adopt_sets_the_transport_to_the_landed_track() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        let playing = landing(&["a", "b"], 1);

        assert!(queue.adopt(&playing, &mut transport));
        assert_eq!(transport.state, TransportState::Loading);
        let now_playing = transport.now_playing.clone().expect("a named track");
        assert_eq!(now_playing.uri, "b");
        assert_eq!(now_playing.title, "b", "a row the label owes is its URI");
        assert_eq!(transport.position_ms, 7_000, "the phone's own position");
        assert_eq!(transport.duration_ms, None, "the length is not known yet");

        assert!(queue.adopt(
            &Landing {
                playing: false,
                ..playing
            },
            &mut transport
        ));
        assert_eq!(transport.state, TransportState::Paused);
    }

    #[test]
    fn adopt_carries_the_modes_the_landing_names() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();

        assert!(queue.adopt(
            &Landing {
                shuffle: Some(true),
                repeat_context: Some(true),
                repeat_track: Some(true),
                ..landing(&["a", "b", "c"], 0)
            },
            &mut transport
        ));
        assert!(queue.shuffle());
        assert_eq!(
            queue.repeat(),
            Repeat::One,
            "repeat one outranks repeat all"
        );

        // The landing names the context loop alone, so the track loop it says
        // nothing about stays the one the queue is in.
        assert!(queue.adopt(
            &Landing {
                shuffle: Some(false),
                repeat_context: Some(true),
                ..landing(&["a", "b"], 0)
            },
            &mut transport
        ));
        assert_eq!(queue.repeat(), Repeat::One);
        assert!(!queue.shuffle(), "a landing that asks for off turns it off");
    }

    #[test]
    fn a_play_with_no_override_keeps_the_current_modes() {
        let mut queue = started(3, 0);
        let mut transport = Transport::default();
        queue.set_repeat(Repeat::All);
        queue.toggle_shuffle();

        // A play that names no options is a landing whose flags are all absent.
        assert!(queue.adopt(&landing(&["a", "b", "c"], 0), &mut transport));

        assert!(queue.shuffle(), "a play that names no shuffle keeps it on");
        assert_eq!(
            queue.repeat(),
            Repeat::All,
            "a play that names no repeat keeps the mode it is in"
        );
    }

    #[test]
    fn adopt_clears_the_selection_and_refocus() {
        let mut queue = started(5, 0);
        let mut transport = Transport::default();
        queue.press(1, Press::Replace);
        queue.press(3, Press::Toggle);
        queue.refocus_at(3);
        assert_eq!(picked(&queue), vec![1, 3]);

        queue.adopt(&landing(&["a", "b", "c", "d", "e"], 0), &mut transport);
        assert!(
            picked(&queue).is_empty(),
            "a pick cannot survive other rows"
        );
        assert_eq!(queue.refocus(), None);
    }

    #[test]
    fn adopt_keeps_the_landed_row_current_under_a_shuffle_draw() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        let rows = ["n0", "n1", "n2", "n3", "n4", "n5"];

        assert!(queue.adopt(
            &Landing {
                shuffle: Some(true),
                ..landing(&rows, 2)
            },
            &mut transport
        ));
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("n2"));
        assert_eq!(
            order(&queue)[..3],
            uris(&["n0", "n1", "n2"]),
            "the rows before the current one keep the phone's order"
        );

        let mut drawn: Vec<String> = order(&queue)[3..].to_vec();
        assert_ne!(
            drawn,
            uris(&["n3", "n4", "n5"]),
            "the coming rows are drawn, not listed"
        );
        drawn.sort();
        assert_eq!(drawn, uris(&["n3", "n4", "n5"]), "the draw moved no row");

        queue.toggle_shuffle();
        assert_eq!(order(&queue), uris(&rows), "the snapshot was the landing's");
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("n2"));
    }

    #[test]
    fn an_out_of_range_index_is_refused_and_changes_nothing() {
        let mut queue = started(3, 1);
        let mut transport = Transport::default();
        queue.jump(1, &mut transport);
        let before = transport.clone();
        let rows = order(&queue);

        assert!(!queue.adopt(&landing(&["a", "b"], 2), &mut transport));
        assert_eq!(order(&queue), rows, "a refused landing replaced the rows");
        assert_eq!(queue.index, Some(1));
        assert_eq!(
            transport, before,
            "a refused landing must not touch the transport"
        );
    }

    #[test]
    fn lost_pauses_a_playing_or_loading_transport() {
        for state in [TransportState::Playing, TransportState::Loading] {
            let mut queue = started(2, 0);
            let mut transport = Transport {
                state,
                ..Transport::default()
            };
            assert_eq!(
                queue.apply(&lost(1), &mut transport),
                Some(Command::Pause),
                "{state:?} was not paused"
            );
            assert_eq!(queue.lost_epoch, 1);
        }
    }

    #[test]
    fn lost_while_paused_sends_nothing() {
        let mut queue = started(2, 0);
        let mut transport = Transport {
            state: TransportState::Paused,
            ..Transport::default()
        };

        assert_eq!(queue.apply(&lost(2), &mut transport), None);
        assert_eq!(queue.lost_epoch, 2, "a loss while paused is still a loss");
        assert_eq!(
            queue.apply(&lost(1), &mut transport),
            None,
            "an older epoch does not lower the mark"
        );
        assert_eq!(queue.lost_epoch, 2);
    }

    #[test]
    fn lost_then_landed_does_not_play() {
        let mut queue = started(2, 0);
        let mut transport = Transport {
            state: TransportState::Playing,
            ..Transport::default()
        };
        assert_eq!(queue.apply(&lost(1), &mut transport), Some(Command::Pause));
        let rows = order(&queue);
        let before = transport.clone();

        assert_eq!(
            queue.apply(&landed(&landing(&["a", "b"], 0)), &mut transport),
            None
        );
        assert_eq!(order(&queue), rows, "a stale landing replaced the rows");
        assert_eq!(queue.index, Some(0));
        assert_eq!(transport, before, "a stale landing moved the transport");
    }

    #[test]
    fn a_landed_with_a_newer_epoch_after_a_loss_is_adopted() {
        let mut queue = started(2, 0);
        let mut transport = Transport {
            state: TransportState::Playing,
            ..Transport::default()
        };
        assert_eq!(queue.apply(&lost(1), &mut transport), Some(Command::Pause));

        let landing = Landing {
            epoch: 2,
            ..landing(&["a", "b", "c"], 1)
        };
        assert_eq!(queue.apply(&landed(&landing), &mut transport), None);
        assert_eq!(order(&queue), uris(&["a", "b", "c"]));
        assert_eq!(queue.index, Some(1));
        assert_eq!(transport.state, TransportState::Loading);
    }

    #[test]
    fn label_wants_name_the_unnamed_rows_after_the_current_once() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.adopt(&landing(&["a", "b", "c", "d"], 1), &mut transport);

        assert_eq!(
            queue.label_wants(),
            Some(Command::LabelRows {
                uris: uris(&["c", "d"]),
            }),
            "only the rows after the current one owe a name"
        );
        assert_eq!(queue.label_wants(), None, "a row is asked for once");
    }

    #[test]
    fn a_label_batch_is_capped_and_the_rest_wait_for_the_next_pass() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        let rows: Vec<String> = (0..LABEL_BATCH + 40)
            .map(|row| format!("spotify:track:{row}"))
            .collect();
        let borrowed: Vec<&str> = rows.iter().map(String::as_str).collect();
        queue.adopt(&landing(&borrowed, 0), &mut transport);

        let Some(Command::LabelRows { uris: first }) = queue.label_wants() else {
            panic!("the unnamed rows owe a request");
        };
        assert_eq!(first.len(), LABEL_BATCH, "one request names one batch");
        assert_eq!(first, rows[1..=LABEL_BATCH].to_vec());

        let Some(Command::LabelRows { uris: second }) = queue.label_wants() else {
            panic!("the rows left over owe the next request");
        };
        assert_eq!(
            second,
            rows[LABEL_BATCH + 1..].to_vec(),
            "the next pass names the rest, and nothing twice"
        );
        assert_eq!(queue.label_wants(), None);
    }

    #[test]
    fn relabel_fills_rows_by_uri_and_leaves_the_index() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.adopt(&landing(&["a", "b", "a"], 1), &mut transport);

        let mut a = track("a");
        a.title = "Alpha".to_owned();
        let mut b = track("b");
        b.title = "Beta".to_owned();
        assert_eq!(
            queue.apply(&labelled(vec![a, b, track("gone")]), &mut transport),
            None
        );

        assert_eq!(queue.index, Some(1), "the label moved the current row");
        assert_eq!(order(&queue), uris(&["a", "b", "a"]));
        assert_eq!(queue.tracks[0].title, "Alpha", "the row by URI, not index");
        assert_eq!(queue.tracks[1].title, "Beta");
        assert_eq!(queue.tracks[2].title, "Alpha", "every row with that URI");
        assert!(queue.tracks.iter().all(|entry| entry.named));
        assert_eq!(queue.label_wants(), None, "nothing left to ask for");
    }

    #[test]
    fn a_label_that_names_the_loaded_row_renames_the_playbar() {
        let mut queue = Queue::default();
        let mut transport = Transport::default();
        queue.adopt(&landing(&["a", "b"], 1), &mut transport);
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.title.as_str()),
            Some("b"),
            "the bar starts on the URI the landing carried"
        );

        let mut b = track("b");
        b.title = "Beta".to_owned();
        b.artists = "The Band".to_owned();
        assert_eq!(
            queue.apply(&labelled(vec![b]), &mut transport),
            None,
            "naming a row asks for nothing"
        );

        let now = transport.now_playing.expect("the loaded row is named");
        assert_eq!(now.uri, "b");
        assert_eq!(now.title, "Beta");
        assert_eq!(now.artists, "The Band");
    }
}
