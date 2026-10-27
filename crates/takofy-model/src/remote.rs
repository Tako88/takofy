//! What the window and the engine exchange about the account's devices.
//!
//! Plain data: a request the engine is still working on is named by its id, the
//! window has no way to answer one except through that id, and the window's own
//! state travels back as a [`Report`] the engine turns into an announcement.

use crate::TrackEntry;

/// The handle the dealer gave one incoming command.
///
/// Only the engine mints one; the window keeps it so its answer can name the
/// request it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

/// What this window is doing, as the phone's now-playing needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The transport is moving.
    Playing,
    /// A track has been accepted and has not started yet.
    Loading,
    /// A track is loaded and still.
    Paused,
}

/// The context options a report carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub shuffle: bool,
    pub repeat_context: bool,
    pub repeat_track: bool,
}

/// The most rows behind the current one a report carries.
///
/// The phone's window draws no more, and the wire caps the same list again.
pub const MAX_PREV_TRACKS: usize = 10;

/// The most rows ahead of the current one a report carries.
pub const MAX_NEXT_TRACKS: usize = 80;

/// This window's player state, as the window itself sees it.
///
/// A report is a snapshot: it names the rows around the current one, so the
/// phone can draw the queue without reading our context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub status: Status,
    /// The row that is current, absent while nothing is loaded.
    pub track: Option<TrackEntry>,
    /// The rows before the current one, nearest last.
    pub prev: Vec<TrackEntry>,
    /// The rows after the current one, nearest first.
    pub next: Vec<TrackEntry>,
    /// The context the rows came from, absent when they came from nowhere.
    pub context: Option<String>,
    /// The newest queue revision the phone published, empty when it published
    /// none.
    ///
    /// It is echoed back so the phone can match its window to ours; it is never
    /// a token, because a revision that arrives late is still the truth about
    /// the queue that sent it.
    pub queue_revision: String,
    /// The context options, absent when the source did not name every mode: a
    /// put that carries no options asserts none of them, so the phone keeps its
    /// own.
    pub options: Option<Options>,
    pub position_ms: u32,
    pub duration_ms: Option<u32>,
}

/// One transport action the phone asked for while another device holds the
/// account's sound.
///
/// The phone stopped playing on its own, so these arrive as commands the window
/// has to carry out and answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    Pause,
    Resume,
    /// Jump to this position within the loaded track.
    Seek(u32),
    /// Step forward, landing on this track when the phone named one.
    Next(Option<String>),
    /// Step back, or restart the loaded track when it is already past the
    /// restart threshold.
    Prev,
    /// Turn shuffle on or off.
    SetShuffle(bool),
    /// The context options, each carried only when the phone named it.
    SetOptions {
        shuffling: Option<bool>,
        repeating_context: Option<bool>,
        repeating_track: Option<bool>,
    },
    /// Put this track next in the queue, when the phone named one.
    AddToQueue(Option<String>),
    /// The phone's own window around its current row, and the revision it came
    /// from.
    ///
    /// Rows are uris only: the phone's uids and metadata name nothing here, and
    /// separators and empty rows are for the queue to clean.
    SetQueue {
        /// The rows behind the current one, nearest last.
        prev: Vec<String>,
        /// The rows ahead of the current one, nearest first.
        next: Vec<String>,
        revision: String,
    },
}

/// Where a hand-over landed: the rows to adopt, and where in them to sit.
///
/// The engine builds one and hands it over; `epoch` names which activation it
/// belongs to, so a loss that crosses it in flight wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landing {
    pub rows: Vec<String>,
    pub index: usize,
    pub position_ms: u32,
    /// Whether the phone asked for this to play; the transport starts when true.
    pub playing: bool,
    pub context: Option<String>,
    /// The modes the landing names, absent where it names none so the queue
    /// keeps the mode it has.
    pub shuffle: Option<bool>,
    pub repeat_context: Option<bool>,
    pub repeat_track: Option<bool>,
    pub epoch: u64,
}

impl Landing {
    /// The report this landing publishes.
    ///
    /// A playing landing is `Loading`, not `Playing`: it has been accepted and
    /// the player has not started, and the phone reads `Loading` as the wait.
    pub fn report(&self) -> Report {
        let track = |uri: &String| TrackEntry {
            uri: uri.clone(),
            title: uri.clone(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: false,
            cover: None,
        };
        Report {
            status: if self.playing {
                Status::Loading
            } else {
                Status::Paused
            },
            track: self.rows.get(self.index).map(track),
            prev: self.rows.iter().take(self.index).map(track).collect(),
            next: self
                .rows
                .iter()
                .skip(self.index.saturating_add(1))
                .map(track)
                .collect(),
            context: self.context.clone(),
            // A landing is this window's own first word about the rows it takes,
            // so it has no revision to echo back yet.
            queue_revision: String::new(),
            // The wire states every mode at once, so a landing that named only
            // some of them asserts none; the queue's own report follows with
            // the modes it kept.
            options: match (self.shuffle, self.repeat_context, self.repeat_track) {
                (Some(shuffle), Some(repeat_context), Some(repeat_track)) => Some(Options {
                    shuffle,
                    repeat_context,
                    repeat_track,
                }),
                _ => None,
            },
            position_ms: self.position_ms,
            duration_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Landing, Options};

    fn landing(
        shuffle: Option<bool>,
        repeat_context: Option<bool>,
        repeat_track: Option<bool>,
    ) -> Landing {
        Landing {
            rows: vec!["spotify:track:a".to_owned()],
            index: 0,
            position_ms: 0,
            playing: false,
            context: None,
            shuffle,
            repeat_context,
            repeat_track,
            epoch: 1,
        }
    }

    #[test]
    fn a_landing_that_names_every_mode_asserts_them() {
        let report = landing(Some(true), Some(false), Some(true)).report();
        assert_eq!(
            report.options,
            Some(Options {
                shuffle: true,
                repeat_context: false,
                repeat_track: true,
            })
        );
    }

    #[test]
    fn a_landing_that_names_only_some_modes_asserts_none() {
        for (shuffle, repeat_context, repeat_track) in [
            (Some(true), None, Some(true)),
            (Some(true), Some(true), None),
            (None, Some(true), Some(true)),
            (None, None, None),
        ] {
            let report = landing(shuffle, repeat_context, repeat_track).report();
            assert_eq!(
                report.options, None,
                "must assert none for {shuffle:?}/{repeat_context:?}/{repeat_track:?}"
            );
        }
    }
}
