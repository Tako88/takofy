use super::super::*;
use super::test_support::*;

use crate::playlist_edit::owned as owned_by;
use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::playlist_permission::Capabilities;
use librespot::protocol::playlist4_external::{
    Item, ListAttributes, ListItems, SelectedListContent,
};
use protobuf::MessageField;

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

const PICTURE: [u8; 20] = [7; 20];

fn track_uri(index: u8) -> String {
    let mut raw = [0u8; 16];
    raw[15] = index + 1;
    SpotifyUri::Track {
        id: SpotifyId::from_raw(&raw).expect("a track id"),
    }
    .to_uri()
    .expect("a uri")
}

fn episode_uri() -> String {
    SpotifyUri::Episode {
        id: SpotifyId::from_base62(EPISODE_ID).expect("an episode id"),
    }
    .to_uri()
    .expect("a uri")
}

fn local_uri() -> String {
    "spotify:local:Artist:Album:Title:3".to_owned()
}

fn item(uri: &str) -> Item {
    Item {
        uri: Some(uri.to_owned()),
        ..Default::default()
    }
}

/// A decorated window naming `uris`, with every field the sidebar draws set.
fn window(uris: &[String]) -> SelectedListContent {
    let mut content = SelectedListContent::new();
    content.set_revision(b"rev".to_vec());
    content.set_length(uris.len() as i32);
    content.set_owner_username("alice".to_owned());
    content.attributes = MessageField::some(ListAttributes {
        name: Some("Night Drive".to_owned()),
        description: Some("late hours".to_owned()),
        picture: None,
        ..Default::default()
    });
    content.contents = MessageField::some(ListItems {
        items: uris.iter().map(|uri| item(uri)).collect(),
        ..Default::default()
    });
    content.capabilities = MessageField::some(Capabilities {
        can_edit_items: Some(true),
        ..Default::default()
    });
    content
}

fn revision(bytes: &[u8]) -> Revision {
    Revision::from_wire(bytes).expect("a revision")
}

#[test]
fn the_metadata_path_asks_for_a_decorated_head_window() {
    assert_eq!(
        playlist_meta_path(PLAYLIST_ID),
        format!(
            "/playlist/v2/playlist/{PLAYLIST_ID}?decorate=revision,attributes,length,owner,capabilities,status_code,timestamp&from=0&length={META_WINDOW}"
        )
    );
    assert_eq!(
        META_WINDOW, 24,
        "twice the head cap, so the cap is reachable"
    );
    assert!(
        META_WINDOW >= HEAD_TRACKS as u32,
        "a smaller window could not fill the head"
    );
}

#[test]
fn a_decorated_window_maps_every_field() {
    let uris = vec![track_uri(0), track_uri(1)];
    let mut content = window(&uris);
    content.attributes = MessageField::some(ListAttributes {
        name: Some("Night Drive".to_owned()),
        description: Some("  late\n hours ".to_owned()),
        picture: Some(PICTURE.to_vec()),
        ..Default::default()
    });
    content.set_length(46);

    let meta = playlist_meta_from_content(&content);

    assert_eq!(meta.name, Some("Night Drive".to_owned()));
    assert_eq!(meta.description, Some("late hours".to_owned()));
    assert_eq!(meta.tracks, Some(46), "the declared length, not the window");
    assert_eq!(meta.owner, Some("alice".to_owned()));
    assert_eq!(
        meta.access,
        Some(PlaylistAccess {
            revision: revision(b"rev"),
            editable: true,
        })
    );
    assert_eq!(
        meta.picture,
        CoverUrl::from_file_id(&PICTURE).map(CoverSet::single)
    );
    assert!(
        meta.head.is_empty(),
        "a chosen picture costs no head labels"
    );
}

#[test]
fn a_pictureless_window_skips_non_tracks_and_still_fills_the_head() {
    // The server sends `META_WINDOW` items; one episode and one local file sit
    // inside the first thirteen slots.
    let mut uris = vec![episode_uri()];
    uris.extend((0..=10).map(track_uri));
    uris.push(local_uri());
    uris.extend((11..=21).map(track_uri));
    assert_eq!(uris.len(), META_WINDOW as usize);

    let meta = playlist_meta_from_content(&window(&uris));

    let expected: Vec<String> = (0..HEAD_TRACKS as u8).map(track_uri).collect();
    assert_eq!(
        meta.head, expected,
        "an episode and a local file are not tracks, and the cap keeps twelve"
    );
}

#[test]
fn a_window_naming_no_attributes_maps_to_an_empty_meta() {
    let mut content = window(&[track_uri(0)]);
    content.attributes = MessageField::none();

    assert_eq!(playlist_meta_from_content(&content), PlaylistMeta::empty());
}

#[test]
fn a_window_naming_no_length_counts_zero() {
    let mut content = window(&[track_uri(0)]);
    content.length = None;

    assert_eq!(
        playlist_meta_from_content(&content).tracks,
        Some(0),
        "an absent length is the protobuf default, not a missing count"
    );
}

#[test]
fn a_window_naming_no_name_or_owner_maps_to_none() {
    let mut content = window(&[track_uri(0)]);
    content.attributes = MessageField::some(ListAttributes::default());
    content.owner_username = None;

    let meta = playlist_meta_from_content(&content);

    assert_eq!(meta.name, None);
    assert_eq!(meta.owner, None);
}

#[test]
fn a_blank_owner_maps_to_none() {
    let mut content = window(&[track_uri(0)]);
    content.set_owner_username("   ".to_owned());

    assert_eq!(playlist_meta_from_content(&content).owner, None);
}

#[test]
fn a_window_naming_no_revision_has_no_access() {
    let mut content = window(&[track_uri(0)]);
    content.revision = None;

    assert_eq!(playlist_meta_from_content(&content).access, None);
}

#[test]
fn a_window_naming_no_capabilities_is_not_editable() {
    let mut content = window(&[track_uri(0)]);
    content.capabilities = MessageField::none();

    let access = playlist_meta_from_content(&content)
        .access
        .expect("a revision still names an access");

    assert_eq!(access.revision, revision(b"rev"));
    assert!(!access.editable);
}
