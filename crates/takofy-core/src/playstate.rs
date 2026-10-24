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

/// Librespot publishes at most this many rows behind the current one.
pub(crate) const MAX_PREV_TRACKS: usize = 10;

/// Librespot publishes at most this many rows ahead of the current one.
pub(crate) const MAX_NEXT_TRACKS: usize = 80;

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
        options: MessageField::some(options_of(report.options)),
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
mod tests {
    use super::*;

    const AGE_MS: i64 = 3_000;
    const NOW_MS: i64 = 1_700_000_000_000;

    fn entry(uri: &str) -> TrackEntry {
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

    fn report(status: Status) -> Report {
        Report {
            status,
            track: Some(entry("spotify:track:current")),
            prev: Vec::new(),
            next: Vec::new(),
            context: Some("spotify:album:context".to_owned()),
            options: Options::default(),
            position_ms: 10_000,
            duration_ms: Some(200_000),
        }
    }

    fn state(status: Status) -> PlayerState {
        player_state_of(&report(status), AGE_MS, NOW_MS, "session")
    }

    #[test]
    fn status_flags_follow_the_receiver_table() {
        let table = [
            (Status::Playing, (true, false, false)),
            (Status::Loading, (true, false, true)),
            (Status::Paused, (true, true, true)),
        ];

        for (status, expected) in table {
            let state = state(status);
            assert_eq!(
                (state.is_playing, state.is_paused, state.is_buffering),
                expected,
                "{status:?}"
            );
        }
    }

    #[test]
    fn speed_is_zero_when_paused_and_one_otherwise() {
        let table = [
            (Status::Playing, PLAYING_SPEED),
            (Status::Loading, PLAYING_SPEED),
            (Status::Paused, PAUSED_SPEED),
        ];

        for (status, expected) in table {
            assert_eq!(state(status).playback_speed, expected, "{status:?}");
        }
    }

    #[test]
    fn a_playing_report_is_advanced_by_its_age_and_a_paused_one_is_not() {
        assert_eq!(
            state(Status::Playing).position_as_of_timestamp,
            i64::from(report(Status::Playing).position_ms) + AGE_MS
        );
        assert_eq!(
            state(Status::Paused).position_as_of_timestamp,
            i64::from(report(Status::Paused).position_ms)
        );
        assert_eq!(
            state(Status::Loading).position_as_of_timestamp,
            i64::from(report(Status::Loading).position_ms),
            "a load has not advanced the row yet"
        );
    }

    #[test]
    fn the_track_context_options_and_duration_are_carried() {
        let mut report = report(Status::Playing);
        report.options = Options {
            shuffle: true,
            repeat_context: true,
            repeat_track: true,
        };

        let state = player_state_of(&report, AGE_MS, NOW_MS, "session");
        let options = state.options.as_ref().expect("the options are carried");

        assert_eq!(
            state.track.as_ref().map(|track| track.uri.as_str()),
            Some("spotify:track:current")
        );
        assert_eq!(state.context_uri, "spotify:album:context");
        assert!(options.shuffling_context);
        assert!(options.repeating_context);
        assert!(options.repeating_track);
        assert_eq!(state.duration, 200_000);
    }

    #[test]
    fn prev_keeps_the_nearest_ten_and_next_the_first_eighty() {
        let mut report = report(Status::Playing);
        report.prev = (0..MAX_PREV_TRACKS + 5)
            .map(|at| entry(&format!("spotify:track:prev-{at}")))
            .collect();
        report.next = (0..MAX_NEXT_TRACKS + 20)
            .map(|at| entry(&format!("spotify:track:next-{at}")))
            .collect();

        let state = player_state_of(&report, AGE_MS, NOW_MS, "session");

        assert_eq!(state.prev_tracks.len(), MAX_PREV_TRACKS);
        assert_eq!(state.prev_tracks[0].uri, "spotify:track:prev-5");
        assert_eq!(
            state.prev_tracks[MAX_PREV_TRACKS - 1].uri,
            format!("spotify:track:prev-{}", MAX_PREV_TRACKS + 4)
        );
        assert_eq!(state.next_tracks.len(), MAX_NEXT_TRACKS);
        assert_eq!(state.next_tracks[0].uri, "spotify:track:next-0");
        assert_eq!(
            state.next_tracks[MAX_NEXT_TRACKS - 1].uri,
            format!("spotify:track:next-{}", MAX_NEXT_TRACKS - 1)
        );
        assert_eq!(
            state.index.as_ref().map(|index| index.track),
            Some(MAX_PREV_TRACKS as u32)
        );
    }

    #[test]
    fn an_active_state_needs_a_track() {
        assert!(ActiveState::new(report(Status::Playing), AGE_MS).is_some());

        let mut trackless = report(Status::Playing);
        trackless.track = None;
        assert!(ActiveState::new(trackless, AGE_MS).is_none());

        let mut blank = report(Status::Playing);
        blank.track = Some(entry(""));
        assert!(ActiveState::new(blank, AGE_MS).is_none());
    }

    #[test]
    fn the_session_id_and_timestamps_are_set() {
        let state = player_state_of(&report(Status::Playing), AGE_MS, NOW_MS, "session-1");

        assert_eq!(state.session_id, "session-1");
        assert_eq!(state.timestamp, NOW_MS);
        assert_eq!(
            state.position_as_of_timestamp,
            i64::from(report(Status::Playing).position_ms) + AGE_MS
        );
    }
}
