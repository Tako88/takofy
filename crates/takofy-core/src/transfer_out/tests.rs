use super::*;
use takofy_model::Options;

fn row(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        duration_ms: None,
        cover: None,
        artist_names: Vec::new(),
        added_at: None,
        named: true,
    }
}

fn report() -> Report {
    Report {
        status: Status::Playing,
        track: Some(row("spotify:track:b")),
        prev: vec![row("spotify:track:a")],
        next: vec![row("spotify:track:c"), row("spotify:track:d")],
        context: Some("spotify:playlist:ours".to_owned()),
        queue_revision: String::new(),
        options: Some(Options {
            shuffle: true,
            repeat_context: false,
            repeat_track: true,
        }),
        position_ms: 4_200,
        duration_ms: Some(200_000),
    }
}

fn state(body: &str) -> TransferState {
    let data = body
        .split("\"data\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the body carries data");
    TransferState::parse_from_bytes(&decode(data)).expect("the data is a transfer state")
}

#[test]
fn the_body_asks_for_a_transfer_and_names_us() {
    let body = body(&report(), "our-device", 1_000, &[]).expect("a report with a row");
    assert!(body.starts_with("{\"message_id\":"), "{body}");
    assert!(
        body.contains("\"sent_by_device_id\":\"our-device\""),
        "{body}"
    );
    assert!(body.contains("\"endpoint\":\"transfer\""), "{body}");
    assert!(!body.contains('\n'), "a body is one line");
}

#[test]
fn the_state_carries_every_row_around_the_current_one() {
    let state = state(&body(&report(), "own", 1_000, &[]).expect("a body"));
    let context = state.current_session.context.as_ref().expect("a context");
    assert_eq!(context.uri.as_deref(), Some("spotify:playlist:ours"));
    let tracks = &context.pages.first().expect("a page").tracks;
    let uris: Vec<&str> = tracks
        .iter()
        .filter_map(|track| track.uri.as_deref())
        .collect();
    assert_eq!(
        uris,
        vec![
            "spotify:track:a",
            "spotify:track:b",
            "spotify:track:c",
            "spotify:track:d"
        ]
    );

    // The uid the session names is the one the current row carries.
    let current_uid = state.current_session.current_uid.as_deref();
    assert_eq!(current_uid, Some("row1"));
    assert_eq!(
        tracks
            .iter()
            .find(|track| track.uid.as_deref() == current_uid)
            .and_then(|track| track.uri.as_deref()),
        Some("spotify:track:b")
    );
    assert_eq!(
        state.playback.current_track.uri.as_deref(),
        Some("spotify:track:b")
    );
}

#[test]
fn the_state_carries_the_position_the_modes_and_whether_it_plays() {
    let state = state(&body(&report(), "own", 1_000, &[]).expect("a body"));
    let playback = state.playback.as_ref().expect("a playback");
    assert_eq!(playback.position_as_of_timestamp, Some(4_200));
    assert_eq!(playback.timestamp, Some(1_000));
    assert_eq!(playback.is_paused, Some(false));

    let options = state.options.as_ref().expect("options");
    assert_eq!(options.shuffling_context, Some(true));
    assert_eq!(options.repeating_context, Some(false));
    assert_eq!(options.repeating_track, Some(true));
}

#[test]
fn a_paused_window_moves_a_paused_state() {
    let mut paused = report();
    paused.status = Status::Paused;
    let state = state(&body(&paused, "own", 1_000, &[]).expect("a body"));
    assert_eq!(
        state
            .playback
            .as_ref()
            .and_then(|playback| playback.is_paused),
        Some(true)
    );
}

fn resolved(uris: &[(&str, &str)]) -> Vec<ContextRow> {
    uris.iter()
        .map(|(uri, uid)| ContextRow {
            uri: (*uri).to_owned(),
            uid: (*uid).to_owned(),
        })
        .collect()
}

#[test]
fn the_page_comes_from_the_resolved_context_and_names_the_row() {
    let rows = resolved(&[
        ("spotify:track:a", "server-a"),
        ("spotify:track:b", "server-b"),
        ("spotify:track:c", "server-c"),
    ]);
    let state = state(&body(&report(), "own", 1_000, &rows).expect("a report with a row"));
    let tracks = &state
        .current_session
        .context
        .as_ref()
        .expect("a context")
        .pages
        .first()
        .expect("a page")
        .tracks;
    assert_eq!(tracks.len(), 3, "the context's own order, not a window");
    assert_eq!(
        state.current_session.current_uid.as_deref(),
        Some("server-b"),
        "the row the sound is on, named the way the account names it"
    );
    assert_eq!(
        state.playback.current_track.uri.as_deref(),
        Some("spotify:track:b")
    );
    assert_eq!(
        state.playback.current_track.uid.as_deref(),
        Some("server-b")
    );
}

#[test]
fn a_context_that_does_not_name_the_row_moves_the_near_rows() {
    let rows = resolved(&[
        ("spotify:track:a", "server-a"),
        ("spotify:track:d", "server-d"),
    ]);
    // The report is on a track this context does not hold.
    let state = state(&body(&report(), "own", 1_000, &rows).expect("a body"));
    let tracks = &state
        .current_session
        .context
        .as_ref()
        .expect("a context")
        .pages
        .first()
        .expect("a page")
        .tracks;
    assert_eq!(tracks.len(), 4, "the rows around the current one");
    assert_eq!(
        state.playback.current_track.uri.as_deref(),
        Some("spotify:track:b"),
        "the current row is still carried"
    );
}

#[test]
fn a_long_context_is_windowed_around_the_row() {
    let mut uris: Vec<(String, String)> = (0..1_000)
        .map(|index| (format!("spotify:track:{index}"), format!("server-{index}")))
        .collect();
    let rows: Vec<ContextRow> = uris
        .drain(..)
        .map(|(uri, uid)| ContextRow { uri, uid })
        .collect();
    let mut long = report();
    long.track = Some(row("spotify:track:900"));
    let state = state(&body(&long, "own", 1_000, &rows).expect("a body"));
    let page = state
        .current_session
        .context
        .as_ref()
        .expect("a context")
        .pages
        .first()
        .expect("a page");
    assert_eq!(page.tracks.len(), MOVE_MAX_ROWS);
    assert_eq!(
        state.current_session.current_uid.as_deref(),
        Some("server-900")
    );
    assert!(
        page.tracks
            .iter()
            .any(|track| track.uid.as_deref() == Some("server-900")),
        "the current row is inside the window"
    );
}

#[test]
fn a_report_with_no_row_cannot_move_anything() {
    let mut empty = report();
    empty.track = None;
    assert_eq!(body(&empty, "own", 1_000, &[]), Err(MoveError::NoTrack));
}

#[test]
fn a_report_with_no_context_cannot_move_anything() {
    let mut empty = report();
    empty.context = None;
    assert_eq!(body(&empty, "own", 1_000, &[]), Err(MoveError::NoContext));
}

#[test]
fn a_report_with_an_empty_context_cannot_move_anything() {
    let mut empty = report();
    empty.context = Some(String::new());
    assert_eq!(body(&empty, "own", 1_000, &[]), Err(MoveError::NoContext));
}

#[test]
fn a_hostile_device_id_stays_inside_its_field() {
    let body = body(&report(), "ow\"n\\x", 1_000, &[]).expect("a body");
    assert!(
        body.contains("\"sent_by_device_id\":\"ow\\\"n\\\\x\""),
        "{body}"
    );
}

#[test]
fn the_encoding_round_trips_and_matches_the_reference_vectors() {
    assert_eq!(encode(b""), "");
    assert_eq!(encode(b"f"), "Zg==");
    assert_eq!(encode(b"fo"), "Zm8=");
    assert_eq!(encode(b"foo"), "Zm9v");
    assert_eq!(encode(b"foob"), "Zm9vYg==");
    assert_eq!(encode(b"fooba"), "Zm9vYmE=");
    assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    for length in 0..40 {
        let bytes: Vec<u8> = (0..length).map(|value| (value * 37 % 256) as u8).collect();
        assert_eq!(decode(&encode(&bytes)), bytes);
    }
}
