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
        queue_revision: String::new(),
        options: None,
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
    report.options = Some(Options {
        shuffle: true,
        repeat_context: true,
        repeat_track: true,
    });

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
fn a_report_that_names_no_mode_asserts_none() {
    let report = report(Status::Playing);
    assert_eq!(report.options, None, "the fixture names no mode");

    let state = player_state_of(&report, AGE_MS, NOW_MS, "session");

    assert!(
        state.options.is_none(),
        "a put that names no mode must not read back as the modes being off"
    );
}

#[test]
fn the_queue_revision_is_published_back() {
    let mut report = report(Status::Playing);
    assert_eq!(
        player_state_of(&report, AGE_MS, NOW_MS, "session").queue_revision,
        "",
        "a report with no revision puts none"
    );

    report.queue_revision = "rev-4".to_owned();
    assert_eq!(
        player_state_of(&report, AGE_MS, NOW_MS, "session").queue_revision,
        "rev-4",
        "the revision the phone pushed is the one it reads back"
    );
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
