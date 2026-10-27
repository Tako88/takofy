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

use takofy_model::uri::{UriKind, parse};
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

/// Whether `uri` is a row this queue holds: a Spotify uri of a kind that names a
/// playable thing.
///
/// The window is built in `takofy-ui`, which cannot reach librespot's parser, and
/// a row only has to be one the worker can be handed; what is refused is a row
/// that names no track at all, which would fail only once someone played it.
fn spotify_uri(uri: &str) -> bool {
    parse(uri).is_some_and(|parsed| {
        matches!(
            parsed.kind,
            UriKind::Track | UriKind::Episode | UriKind::Local
        )
    })
}

#[cfg(test)]
mod tests;
