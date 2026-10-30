use super::{Choice, format_playlist_line, format_track_line, parse_choice};
use takofy_model::{PlaylistEntry, TrackEntry};

#[test]
fn in_range_numbers_pick() {
    assert_eq!(parse_choice("1", 3), Choice::Pick(1));
    assert_eq!(parse_choice("3", 3), Choice::Pick(3));
}

#[test]
fn out_of_range_numbers_are_invalid() {
    assert_eq!(parse_choice("4", 3), Choice::Invalid);
    assert_eq!(parse_choice("999", 3), Choice::Invalid);
}

#[test]
fn zero_is_invalid() {
    assert_eq!(parse_choice("0", 3), Choice::Invalid);
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    assert_eq!(parse_choice("  2\t", 3), Choice::Pick(2));
}

#[test]
fn whitespace_only_quits() {
    assert_eq!(parse_choice("  \t ", 3), Choice::Quit);
}

#[test]
fn next_is_case_insensitive() {
    assert_eq!(parse_choice("n", 3), Choice::Next);
    assert_eq!(parse_choice("N", 3), Choice::Next);
}

#[test]
fn quit_is_case_insensitive() {
    assert_eq!(parse_choice("q", 3), Choice::Quit);
    assert_eq!(parse_choice("Q", 3), Choice::Quit);
}

#[test]
fn empty_line_quits() {
    assert_eq!(parse_choice("", 3), Choice::Quit);
}

#[test]
fn non_numeric_input_is_invalid() {
    assert_eq!(parse_choice("abc", 3), Choice::Invalid);
    assert_eq!(parse_choice("1x", 3), Choice::Invalid);
}

#[test]
fn playlist_line_names_the_playlist() {
    let entry = PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: "spotify:playlist:x".to_owned(),
        name: "Mix".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    };
    assert_eq!(
        format_playlist_line(2, &entry),
        "2. Mix - spotify:playlist:x"
    );
}

#[test]
fn track_line_names_the_track() {
    let track = TrackEntry {
        uri: "spotify:track:y".to_owned(),
        title: "Song".to_owned(),
        artists: "Band".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    };
    assert_eq!(
        format_track_line(1, &track),
        "1. Song - Band (spotify:track:y)"
    );
}
