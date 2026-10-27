//! The kind a phone command names: its endpoint, and the context it points at.

use librespot::core::dealer::protocol::Command;

use super::super::*;
use super::test_support::*;

#[test]
fn each_endpoint_has_its_kind() {
    let table = commands();
    assert_eq!(table.len(), 14, "the table covers every known endpoint");

    for (command, expected) in table {
        assert_eq!(kind_of(&command), expected, "{command}");
    }
}

#[test]
fn an_unknown_endpoint_is_unknown() {
    let payload = serde_json::json!({"endpoint": "something_new", "value": 1});

    assert_eq!(kind_of(&Command::Unknown(payload)), Kind::Unknown);
}

#[test]
fn kind_words_are_fixed_and_distinct() {
    let cases = [
        (Kind::Transfer, "transfer"),
        (Kind::Play, "play"),
        (Kind::Pause, "pause"),
        (Kind::Resume, "resume"),
        (Kind::SeekTo, "seek_to"),
        (Kind::SkipNext, "skip_next"),
        (Kind::SkipPrev, "skip_prev"),
        (Kind::SetShufflingContext, "set_shuffling_context"),
        (Kind::SetRepeatingContext, "set_repeating_context"),
        (Kind::SetRepeatingTrack, "set_repeating_track"),
        (Kind::SetOptions, "set_options"),
        (Kind::SetQueue, "set_queue"),
        (Kind::AddToQueue, "add_to_queue"),
        (Kind::UpdateContext, "update_context"),
        (Kind::Unknown, "unknown"),
    ];

    let mut words: Vec<&str> = Vec::new();
    for (kind, expected) in cases {
        assert_eq!(kind.word(), expected);
        assert!(
            !words.contains(&expected),
            "{expected} names more than one kind"
        );
        words.push(expected);
    }
}

#[test]
fn context_kind_table() {
    let cases = [
        (
            Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            ContextKind::Playlist,
        ),
        (
            Some("spotify:user:spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            ContextKind::Playlist,
        ),
        (
            Some("spotify:album:1DFixLWuPkv3KT3TnV35m3"),
            ContextKind::Album,
        ),
        (
            Some("spotify:show:5CfCWKI5pZ28U0uOJ0ZZAK"),
            ContextKind::Show,
        ),
        (
            Some("spotify:user:someone:collection"),
            ContextKind::Collection,
        ),
        (
            Some("spotify:artist:0OdUWJ0sBjDrqHygGUXeCF"),
            ContextKind::Artist,
        ),
        (
            Some("spotify:station:artist:0OdUWJ0sBjDrqHygGUXeCF"),
            ContextKind::Station,
        ),
        (Some("spotify:search:radiohead"), ContextKind::Search),
        (
            Some("spotify:local:artist:album:title:200"),
            ContextKind::Local,
        ),
        (
            // An artist's popular tracks: the phone's "Popular" list.
            Some("spotify:list:popular-release-segments-main-roles:artist_0OdUWJ0sBjDrqHygGUXeCF"),
            ContextKind::List,
        ),
        (
            Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC"),
            ContextKind::Other,
        ),
        (Some("https://example.test/list"), ContextKind::Other),
        (Some(""), ContextKind::None),
        (Some("-"), ContextKind::None),
        (None, ContextKind::None),
    ];

    for (uri, expected) in cases {
        assert_eq!(ContextKind::of_uri(uri), expected, "{uri:?}");
    }
}

#[test]
fn every_flat_context_is_honoured_and_the_rest_are_not() {
    let cases = [
        (ContextKind::Playlist, true),
        (ContextKind::Album, true),
        (ContextKind::Show, true),
        (ContextKind::Collection, true),
        (ContextKind::List, true),
        (ContextKind::Artist, false),
        (ContextKind::Station, false),
        (ContextKind::Search, false),
        (ContextKind::Local, false),
        (ContextKind::Other, false),
        (ContextKind::None, false),
    ];

    for (kind, expected) in cases {
        assert_eq!(kind.honoured(), expected, "{kind:?}");
    }
}
