//! The transport's state and the position it draws.
//!
//! The position is a pure function of an anchor and an elapsed time, so the
//! interpolation is testable without a clock and the app owns the clock.

use std::time::Duration;

use takofy_model::{CoverSet, TrackEntry};

use crate::message::{Command, Failure, TransportUpdate, Update};
use crate::signin::failure_message;
use crate::volume::Volume;

/// How often the bar is redrawn while a track plays. A repaint rate this
/// interpolation depends on, so it is not a design token.
pub const REDRAW: Duration = Duration::from_millis(100);

/// How far into a track Previous still steps back rather than restarting it.
pub const RESTART_AFTER_MS: u32 = 4_000;

/// One arrow press moves the position this far.
pub const SEEK_STEP_MS: u32 = 5_000;

/// Where the transport is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportState {
    /// Nothing has been played.
    #[default]
    Idle,
    /// A track is being loaded.
    Loading,
    /// Playback is running.
    Playing,
    /// Playback is paused.
    Paused,
    /// The track could not be played.
    Unavailable,
    /// The connection was lost and the worker is rebuilding it. The track and
    /// its position are kept.
    Reconnecting,
    /// Playback ended or was stopped.
    Stopped,
    /// The command could not be carried out.
    Failed(Failure),
}

impl TransportState {
    /// Operator-facing text for a state, drawn as the Now Playing pane's state
    /// line. The playbar names no state of its own.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Loading => "Loading",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Unavailable => "Unavailable",
            Self::Reconnecting => "Reconnecting",
            Self::Stopped => "Stopped",
            Self::Failed(failure) => failure_message(*failure),
        }
    }
}

/// What the bar names. Kept here rather than looked up when the bar draws,
/// because the list the track came from may no longer be on screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NowPlaying {
    pub uri: String,
    pub title: String,
    /// The track's artists, as one line. Held for the same reason as the title:
    /// the list the track came from may no longer be on screen.
    pub artists: String,
    /// The track's album, when the metadata carried one.
    pub album_uri: Option<String>,
    /// The URI of the credit the line names: an artist, or the show when the entry
    /// is an episode. A joined credit is one link, and it goes to this credit.
    pub artist_uri: Option<String>,
    /// The track's album cover, as it was known when the track started. The
    /// panel and the playbar look it up each frame, so a cover that arrives
    /// later still reaches them.
    pub cover: Option<CoverSet>,
}

impl From<&TrackEntry> for NowPlaying {
    fn from(entry: &TrackEntry) -> Self {
        Self {
            uri: entry.uri.clone(),
            title: entry.title.clone(),
            artists: entry.artists.clone(),
            album_uri: entry.album_uri.clone(),
            artist_uri: entry.artist_uris.first().cloned(),
            cover: entry.cover.clone(),
        }
    }
}

/// A track named by hand, for tests that only care about its words.
#[cfg(test)]
impl NowPlaying {
    pub fn named(uri: &str, title: &str, artists: &str) -> Self {
        Self {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: artists.to_owned(),
            ..Self::default()
        }
    }
}

/// The transport's anchor and the track it is about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transport {
    pub position_ms: u32,
    pub state: TransportState,
    pub now_playing: Option<NowPlaying>,
    /// The loaded track's length, once the player has reported it.
    pub duration_ms: Option<u32>,
    /// Where a drag on the seek bar has left the position. It outlives the
    /// release, so the bar holds the pointer's answer instead of falling back to
    /// where the track still is, and is dropped when the player confirms.
    pub scrub: Option<u32>,
    /// Whether the pointer is still down. Ending a drag is a one-shot transition,
    /// so asking twice about the same release asks once.
    pub dragging: bool,
    /// The player's level and its lid.
    pub volume: Volume,
}

/// The position and state a redraw interpolates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub position_ms: u32,
    pub state: TransportState,
}

impl Transport {
    /// The level the player has not been told, as the command that tells it.
    ///
    /// Taken rather than read, so a frame sends one command per change instead of
    /// one per redraw of an unchanged bar.
    pub fn take_volume(&mut self) -> Option<Command> {
        self.volume
            .take()
            .map(|percent| Command::Volume { percent })
    }

    /// Folds one update in, reporting whether the anchor moved so the caller
    /// can restamp its clock.
    pub fn apply(&mut self, update: &Update) -> bool {
        let Update::Transport(update) = update else {
            return false;
        };
        let before = self.anchor();
        match update {
            TransportUpdate::Loading => self.state = TransportState::Loading,
            TransportUpdate::Position {
                position_ms,
                paused,
            } => {
                self.position_ms = *position_ms;
                self.state = if *paused {
                    TransportState::Paused
                } else {
                    TransportState::Playing
                };
            }
            TransportUpdate::Ended | TransportUpdate::Stopped => {
                self.state = TransportState::Stopped;
                self.position_ms = 0;
                self.scrub = None;
            }
            TransportUpdate::Unavailable => {
                self.state = TransportState::Unavailable;
                self.scrub = None;
            }
            TransportUpdate::Reconnecting => {
                self.state = TransportState::Reconnecting;
                self.scrub = None;
            }
            TransportUpdate::Duration { duration_ms } => self.duration_ms = Some(*duration_ms),
            TransportUpdate::Seeked { position_ms } => {
                self.position_ms = *position_ms;
                self.scrub = None;
            }
            TransportUpdate::Failed(failure) => {
                self.state = TransportState::Failed(*failure);
                self.scrub = None;
            }
        }
        self.anchor() != before
    }

    /// What the next redraw interpolates from.
    pub fn anchor(&self) -> Anchor {
        Anchor {
            position_ms: self.position_ms,
            state: self.state,
        }
    }

    /// Names the track and returns the command that plays it.
    pub fn play(&mut self, now: NowPlaying) -> Command {
        let uri = now.uri.clone();
        self.now_playing = Some(now);
        self.position_ms = 0;
        self.duration_ms = None;
        self.scrub = None;
        self.state = TransportState::Loading;
        Command::Play { uri }
    }

    /// Settles on a track the engine loaded for a hand-over.
    ///
    /// The engine owns that load, so no `Play` is asked for here: naming the
    /// track and the state it starts in is the whole of it. A playing landing is
    /// `Loading`, because it has been accepted and has not started; the length
    /// is unknown until the player reports it.
    pub fn landed(&mut self, entry: &TrackEntry, position_ms: u32, playing: bool) {
        self.now_playing = Some(NowPlaying::from(entry));
        self.position_ms = position_ms;
        self.duration_ms = None;
        self.scrub = None;
        self.state = if playing {
            TransportState::Loading
        } else {
            TransportState::Paused
        };
    }

    /// How the transport holds `uri`: whether it is the loaded track, and
    /// whether a pause would act on it.
    ///
    /// One decision, because two callers draw the same row: the search menu's
    /// glyph promises what Enter will do — play, or pause on the loaded track —
    /// and the shell performs it.
    /// A glyph that promised a pause the action turns into a restart is what
    /// this rules out.
    ///
    /// A rebuild holds its track too: the worker is already loading it back, so
    /// a play asked mid-rebuild would start a second load. A held `Reconnecting`
    /// row sends the shell to the transport's toggle, which asks for nothing.
    pub fn hold(&self, uri: &str) -> Hold {
        let loaded = self.now_playing.as_ref().is_some_and(|now| now.uri == uri);
        Hold {
            loaded,
            held: loaded
                && matches!(
                    self.state,
                    TransportState::Playing
                        | TransportState::Loading
                        | TransportState::Paused
                        | TransportState::Reconnecting
                ),
        }
    }

    /// Whether the loaded track can be jumped in: playing or paused, with a
    /// known non-zero length.
    pub fn seekable(&self) -> bool {
        matches!(self.state, TransportState::Playing | TransportState::Paused)
            && self.length_known()
    }

    /// The command that jumps to a fraction of the track, if it is seekable.
    pub fn seek_to_fraction(&self, fraction: f32) -> Option<Command> {
        self.seek_target(fraction)
            .map(|position_ms| Command::Seek { position_ms })
    }

    /// Follows the pointer across the bar. Nothing is asked for while the drag
    /// runs: the player is told once, when the button comes back up.
    pub fn scrub_to(&mut self, fraction: f32) {
        self.scrub = self.seek_target(fraction);
        self.dragging = true;
    }

    /// Ends a drag, asking for the position it landed on. The preview stays until
    /// the player reports the seek, so a release does not snap the bar back.
    pub fn scrub_end(&mut self) -> Option<Command> {
        if !self.dragging {
            return None;
        }
        self.dragging = false;
        self.scrub.map(|position_ms| Command::Seek { position_ms })
    }

    /// The clamped position a fraction lands on, or `None` when the track cannot
    /// be jumped in.
    fn seek_target(&self, fraction: f32) -> Option<u32> {
        if !self.seekable() {
            return None;
        }
        self.position_at_fraction(fraction)
    }

    /// The clamped position a fraction lands on, asking only that the length is
    /// known: a holder's transport carries its track while it is still loading,
    /// and the seek the bar offers it is enabled on that alone.
    pub(crate) fn length_target(&self, fraction: f32) -> Option<u32> {
        if !self.length_known() {
            return None;
        }
        self.position_at_fraction(fraction)
    }

    /// The clamped position `delta_ms` after `position_ms`, asking only that the
    /// length is known.
    pub(crate) fn length_by(&self, position_ms: u32, delta_ms: i64) -> Option<u32> {
        if !self.length_known() {
            return None;
        }
        self.position_by(position_ms, delta_ms)
    }

    /// The command that jumps `delta_ms` from the anchor, clamped to the track.
    pub fn seek_by(&self, delta_ms: i64) -> Option<Command> {
        if !self.seekable() {
            return None;
        }
        self.position_by(self.position_ms, delta_ms)
            .map(|position_ms| Command::Seek { position_ms })
    }

    /// Whether a known non-zero length makes a seek possible at all.
    ///
    /// The bar's slider, its arrow keys and a holder's steer all ask here, so
    /// the three cannot disagree about which transports can be jumped in.
    pub(crate) fn length_known(&self) -> bool {
        self.duration_ms.is_some_and(|duration| duration > 0)
    }

    fn position_at_fraction(&self, fraction: f32) -> Option<u32> {
        let duration = self.duration_ms?;
        let target = (fraction.clamp(0.0, 1.0) * duration as f32) as u32;
        Some(target.min(duration.saturating_sub(1)))
    }

    fn position_by(&self, position_ms: u32, delta_ms: i64) -> Option<u32> {
        let max = i64::from(self.duration_ms?.saturating_sub(1));
        let target = i64::from(position_ms).saturating_add(delta_ms);
        Some(target.clamp(0, max) as u32)
    }

    /// The position to draw after `elapsed`, never past a known duration.
    pub fn shown_position(&self, elapsed: Duration) -> u32 {
        if let Some(scrub) = self.scrub {
            return scrub;
        }
        let position = position_at(&self.anchor(), elapsed);
        match self.duration_ms {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    /// The filled fraction at `position_ms`, or `None` with no length to
    /// measure it against.
    pub fn fraction(&self, position_ms: u32) -> Option<f32> {
        let duration = self.duration_ms?;
        (duration > 0).then(|| (position_ms as f32 / duration as f32).clamp(0.0, 1.0))
    }
}

/// How the transport holds one track.
///
/// [`Transport::hold`] is the only place that decides it, so a row's glyph and
/// the action it stands for cannot disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hold {
    /// The transport's own track is this one.
    pub loaded: bool,
    /// Pausing acts on it: it is playing, loading, or paused. `false` with
    /// [`Self::loaded`] means the transport is on the track but a move would
    /// restart it instead.
    pub held: bool,
}

/// The position after `elapsed`, or the anchor's own when not playing.
pub fn position_at(anchor: &Anchor, elapsed: Duration) -> u32 {
    if anchor.state != TransportState::Playing {
        return anchor.position_ms;
    }
    let elapsed_ms = u128::from(anchor.position_ms).saturating_add(elapsed.as_millis());
    elapsed_ms.min(u128::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests;
