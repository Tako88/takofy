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
    pub options: Options,
    pub position_ms: u32,
    pub duration_ms: Option<u32>,
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
    pub shuffle: bool,
    pub repeat_context: bool,
    pub repeat_track: bool,
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
            options: Options {
                shuffle: self.shuffle,
                repeat_context: self.repeat_context,
                repeat_track: self.repeat_track,
            },
            position_ms: self.position_ms,
            duration_ms: None,
        }
    }
}
