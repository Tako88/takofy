use super::super::*;
use super::test_support::*;

use crate::playlist_edit::owned as owned_by;
use librespot::core::{SpotifyId, SpotifyUri};

const PLAYLIST_ID: &str = "37i9dQZF1DXcBWIGoYBM5M";

fn playlist_uri(user: Option<&str>) -> SpotifyUri {
    SpotifyUri::Playlist {
        user: user.map(str::to_owned),
        id: SpotifyId::from_base62(PLAYLIST_ID).expect("a playlist id"),
    }
}

#[test]
fn non_blank_collapses_whitespace_and_drops_a_blank() {
    assert_eq!(non_blank(""), None);
    assert_eq!(non_blank("  \n"), None);
    assert_eq!(non_blank(" x "), Some("x".to_owned()));
    assert_eq!(non_blank("a\n b"), Some("a b".to_owned()));
}

#[test]
fn playlist_headers_name_the_protobuf_media_type() {
    let headers = playlist_headers().expect("headers");
    assert_eq!(
        headers
            .get(http::header::CONTENT_TYPE)
            .expect("a content type"),
        "application/x-protobuf"
    );
}

#[test]
fn playlist_owner_reads_a_named_owner_and_skips_a_blank_one() {
    assert_eq!(
        playlist_owner(&playlist_uri(Some("alice"))),
        Some("alice".to_owned())
    );
    assert_eq!(playlist_owner(&playlist_uri(Some(""))), None);
    assert_eq!(playlist_owner(&playlist_uri(Some("  "))), None);
    assert_eq!(playlist_owner(&playlist_uri(None)), None);
    assert_eq!(playlist_owner(&first_track()), None);
}

#[test]
fn a_playlist_uri_owner_is_compared_as_playlist_owner_spells_it() {
    assert!(owned_by(
        playlist_owner(&playlist_uri(Some("alice"))).as_deref(),
        "alice"
    ));
    assert!(!owned_by(
        playlist_owner(&playlist_uri(Some("Alice"))).as_deref(),
        "alice"
    ));
    assert!(!owned_by(
        playlist_owner(&playlist_uri(Some(""))).as_deref(),
        ""
    ));
    assert!(!owned_by(
        playlist_owner(&playlist_uri(None)).as_deref(),
        "alice"
    ));
    assert!(!owned_by(
        playlist_owner(&first_track()).as_deref(),
        "alice"
    ));
}

#[test]
fn a_collapsed_owner_name_is_not_owned() {
    // `owned` compares byte-exact against the collapsed owner `playlist_owner` yields.
    assert!(!owned_by(
        playlist_owner(&playlist_uri(Some("a  b"))).as_deref(),
        "a  b"
    ));
    assert!(owned_by(
        playlist_owner(&playlist_uri(Some("a  b"))).as_deref(),
        "a b"
    ));
}

#[test]
fn empty_playlist_meta_has_no_description_owner_or_access() {
    let meta = PlaylistMeta::empty();
    assert_eq!(meta.description, None);
    assert_eq!(meta.owner, None);
    assert_eq!(meta.tracks, None);
    assert_eq!(meta.access, None, "a meta that read nothing writes nothing");
}

#[test]
fn a_declared_length_is_the_count_and_a_negative_one_is_not() {
    assert_eq!(playlist_length(62), Some(62));
    assert_eq!(playlist_length(0), Some(0), "an empty playlist counts zero");
    assert_eq!(playlist_length(-1), None, "a wire error is not a length");
}
