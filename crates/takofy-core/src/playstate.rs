//! This window's player state as the account's wire carries it.
//!
//! The status table and the row caps are librespot's own, because they are the
//! shape the phone has been proven to read. The one rule this file adds is the
//! silent-account guard: an active state cannot be built without a track.

use librespot::protocol::player::{
    ContextIndex, ContextPlayerOptions, PlayOrigin, PlayerState, ProvidedTrack, Suppressions,
};
use protobuf::MessageField;
use takofy_model::{Options, Report, Status, TrackEntry};

pub(crate) use takofy_model::{MAX_NEXT_TRACKS, MAX_PREV_TRACKS};

/// The name prefix for the uids this window invents for its own rows.
const UID_PREFIX: &str = "takofy-";

/// Every row this window publishes belongs to the context it adopted.
const PROVIDER: &str = "context";

/// A moving transport reports its speed; a still one reports none.
const PLAYING_SPEED: f64 = 1.0;
const PAUSED_SPEED: f64 = 0.0;

/// A player state that can hold the sound: the report, and how old it is.
///
/// Only [`ActiveState::new`] builds one, and it refuses a report with no track,
/// so an active announcement can never be trackless.
#[derive(Debug, Clone)]
pub(crate) struct ActiveState {
    report: Report,
    age_ms: i64,
}

impl ActiveState {
    pub(crate) fn new(report: Report, age_ms: i64) -> Option<Self> {
        let track = report.track.as_ref()?;
        if track.uri.is_empty() {
            return None;
        }
        Some(Self { report, age_ms })
    }

    pub(crate) fn report(&self) -> &Report {
        &self.report
    }

    pub(crate) fn age_ms(&self) -> i64 {
        self.age_ms
    }
}

/// Builds the wire state for one report, aged by the time since it was taken.
pub(crate) fn player_state_of(
    report: &Report,
    age_ms: i64,
    now_ms: i64,
    session_id: &str,
) -> PlayerState {
    let (is_playing, is_paused, is_buffering) = flags(report.status);
    let prev = &report.prev[report.prev.len().saturating_sub(MAX_PREV_TRACKS)..];
    let next = &report.next[..report.next.len().min(MAX_NEXT_TRACKS)];
    PlayerState {
        session_id: session_id.to_owned(),
        timestamp: now_ms,
        track: report
            .track
            .as_ref()
            .map(|track| provided(track, prev.len()))
            .into(),
        prev_tracks: rows(prev, 0),
        next_tracks: rows(next, prev.len() + 1),
        index: MessageField::some(ContextIndex {
            page: 0,
            track: prev.len() as u32,
            ..Default::default()
        }),
        context_uri: report.context.clone().unwrap_or_default(),
        // The phone's own window carries a revision, and a put that leaves it
        // out tells the phone its push was never seen.
        queue_revision: report.queue_revision.clone(),
        position_as_of_timestamp: position_of(report, age_ms),
        duration: report.duration_ms.map_or(0, i64::from),
        is_playing,
        is_paused,
        is_buffering,
        playback_speed: if is_paused {
            PAUSED_SPEED
        } else {
            PLAYING_SPEED
        },
        is_system_initiated: true,
        // A report that names no mode asserts none, so the phone keeps its own.
        options: report.options.map(options_of).into(),
        play_origin: MessageField::some(PlayOrigin::new()),
        suppressions: MessageField::some(Suppressions::new()),
        ..Default::default()
    }
}

/// The receiver's flag table, as librespot publishes it.
///
/// A paused body is all three true: desktop and mobile grey the play button
/// otherwise. Loading is playing and buffering, and playing is only playing.
fn flags(status: Status) -> (bool, bool, bool) {
    match status {
        Status::Playing => (true, false, false),
        Status::Loading => (true, false, true),
        Status::Paused => (true, true, true),
    }
}

fn options_of(options: Options) -> ContextPlayerOptions {
    ContextPlayerOptions {
        shuffling_context: options.shuffle,
        repeating_context: options.repeat_context,
        repeating_track: options.repeat_track,
        ..Default::default()
    }
}

/// A moving report's position advances with its age; a still one does not.
fn position_of(report: &Report, age_ms: i64) -> i64 {
    let base = i64::from(report.position_ms);
    if report.status == Status::Playing && age_ms > 0 {
        base.saturating_add(age_ms)
    } else {
        base
    }
}

fn rows(tracks: &[TrackEntry], offset: usize) -> Vec<ProvidedTrack> {
    tracks
        .iter()
        .enumerate()
        .map(|(at, track)| provided(track, offset + at))
        .collect()
}

fn provided(track: &TrackEntry, at: usize) -> ProvidedTrack {
    ProvidedTrack {
        uri: track.uri.clone(),
        uid: format!("{UID_PREFIX}{at}"),
        provider: PROVIDER.to_owned(),
        album_uri: track.album_uri.clone().unwrap_or_default(),
        artist_uri: track.artist_uris.first().cloned().unwrap_or_default(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
