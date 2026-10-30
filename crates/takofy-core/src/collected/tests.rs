use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Artist;
use librespot::protocol::entity_extension_data::EntityExtensionData;
use librespot::protocol::extended_metadata::{BatchedExtensionResponse, EntityExtensionDataArray};
use librespot::protocol::metadata::{
    Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
    Date as DateMessage, Disc as DiscMessage, Image, ImageGroup, Track as TrackMessage,
};
use protobuf::Message as _;
use protobuf::MessageField;
use protobuf::well_known_types::any::Any;
use takofy_model::{CoverSet, CoverUrl, LikedTrack, SavedAlbum, SavedAt};

use super::{
    ArtistCard, album_dated, artist_card, artist_entries, artist_order, liked_tracks,
    saved_album_entries, stamp_albums, track_dated,
};
use crate::collection::Item;
use crate::followed::Dated;

/// A 16-byte gid. `SpotifyId::from_raw` rejects anything else.
fn gid(byte: u8) -> Vec<u8> {
    vec![byte; 16]
}

fn id(byte: u8) -> SpotifyId {
    SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id")
}

fn artist_uri(byte: u8) -> String {
    SpotifyUri::Artist { id: id(byte) }.to_uri().expect("a uri")
}

fn album_uri(byte: u8) -> String {
    SpotifyUri::Album { id: id(byte) }.to_uri().expect("a uri")
}

fn track_uri(byte: u8) -> String {
    SpotifyUri::Track { id: id(byte) }.to_uri().expect("a uri")
}

fn episode_uri(byte: u8) -> String {
    SpotifyUri::Episode { id: id(byte) }
        .to_uri()
        .expect("a uri")
}

fn item(uri: &str, added_at: Option<i64>, removed: bool) -> Item {
    Item {
        uri: uri.to_owned(),
        added_at,
        removed,
    }
}

#[test]
fn artist_order_drops_removed_and_non_artist_items_and_orders_newest_first() {
    assert_eq!(
        artist_order(vec![
            item(&track_uri(9), Some(500), false), // a liked song
            item(&artist_uri(1), Some(100), false),
            item(&artist_uri(2), Some(300), true), // a removed artist
            item(&artist_uri(3), None, false),
            item(&artist_uri(1), Some(2_000), false), // a duplicate
        ]),
        vec![artist_uri(1), artist_uri(3)],
        "a liked song and a removed artist are not saved artists, and the first duplicate's date sorts"
    );
}

fn dated(uri: String, added_at: Option<i64>) -> Dated {
    Dated { uri, added_at }
}

#[test]
fn album_dated_drops_liked_songs_and_episodes_and_orders_newest_first() {
    assert_eq!(
        album_dated(vec![
            item(&track_uri(9), Some(900), false), // a liked song
            item(&episode_uri(8), Some(800), false),
            item(&album_uri(1), Some(100), false),
            item(&album_uri(2), Some(400), true), // a removed album
            item(&album_uri(3), None, false),
        ]),
        vec![dated(album_uri(1), Some(100)), dated(album_uri(3), None)],
        "liked songs and episodes are not saved albums, and a removed one stays dropped"
    );
}

#[test]
fn album_dated_keeps_the_first_of_a_duplicate_pair() {
    assert_eq!(
        album_dated(vec![
            item(&album_uri(1), Some(100), false),
            item(&album_uri(1), Some(2_000), false),
        ]),
        vec![dated(album_uri(1), Some(100))],
        "the first occurrence's date sorts, so the later, newer duplicate does not win"
    );
}

#[test]
fn track_dated_keeps_only_live_track_items_with_their_dates() {
    assert_eq!(
        track_dated(vec![
            item(&track_uri(1), Some(100), false),
            item(&album_uri(2), Some(200), false),
            item(&episode_uri(3), Some(300), false),
            item(&track_uri(4), Some(400), true),
            item(&track_uri(5), None, false),
        ]),
        vec![dated(track_uri(1), Some(100)), dated(track_uri(5), None)],
        "an album, an episode and a removed track are not liked songs"
    );
}

#[test]
fn a_track_saved_removed_and_saved_again_keeps_the_live_date() {
    assert_eq!(
        track_dated(vec![
            item(&track_uri(1), Some(100), true),  // saved, then removed
            item(&track_uri(1), Some(200), false), // saved again
        ]),
        vec![dated(track_uri(1), Some(200))]
    );
}

#[test]
fn track_dated_keeps_the_first_duplicate() {
    assert_eq!(
        track_dated(vec![
            item(&track_uri(1), Some(100), false),
            item(&track_uri(1), Some(2_000), false),
        ]),
        vec![dated(track_uri(1), Some(100))],
        "the first occurrence's date sorts, so the later, newer duplicate does not win"
    );
}

#[test]
fn liked_tracks_read_each_date_as_epoch_seconds_and_zero_as_no_date() {
    assert_eq!(
        liked_tracks(vec![
            dated(track_uri(1), Some(1_700_000_000)),
            dated(track_uri(2), Some(0)),
            dated(track_uri(3), None),
        ]),
        vec![
            LikedTrack {
                uri: track_uri(1),
                saved_at: SavedAt::from_millis(1_700_000_000_000),
            },
            LikedTrack {
                uri: track_uri(2),
                saved_at: None,
            },
            LikedTrack {
                uri: track_uri(3),
                saved_at: None,
            },
        ]
    );
}

#[test]
fn stamp_albums_matches_by_uri_not_by_position() {
    let albums = saved_album_entries(&[album_uri(1), album_uri(2), album_uri(3)], Vec::new());
    let stamped = stamp_albums(
        albums,
        &[
            dated(album_uri(3), Some(300)),
            dated(album_uri(2), Some(0)),
            dated(album_uri(1), Some(100)),
        ],
    );
    let dates: Vec<_> = stamped.iter().map(|album| album.saved_at).collect();
    assert_eq!(
        dates,
        vec![SavedAt::from_secs(100), None, SavedAt::from_secs(300),],
        "a zero date is no date, and each album takes the date named for its own URI"
    );
}

#[test]
fn an_album_no_date_names_keeps_no_date() {
    let albums = saved_album_entries(&[album_uri(1)], Vec::new());
    let stamped = stamp_albums(albums, &[dated(album_uri(9), Some(900))]);
    assert_eq!(stamped[0].saved_at, None);
}

#[test]
fn an_empty_item_list_gives_an_empty_order() {
    assert!(artist_order(Vec::new()).is_empty());
    assert!(album_dated(Vec::new()).is_empty());
    assert!(track_dated(Vec::new()).is_empty());
}

/// The parsed artist the page reads.
fn artist(message: ArtistMessage) -> Artist {
    Artist::try_from(&message).expect("a valid artist message")
}

fn artist_message(byte: u8, name: &str) -> ArtistMessage {
    let mut message = ArtistMessage::default();
    message.set_gid(gid(byte));
    message.set_name(name.to_owned());
    message
}

/// One album message carrying nothing but a gid, as a release list has it.
fn release_message(byte: u8) -> AlbumMessage {
    let mut album = AlbumMessage::default();
    album.set_gid(gid(byte));
    album
}

/// One group of variants, the first of which is the current release.
fn group(bytes: &[u8]) -> AlbumGroupMessage {
    AlbumGroupMessage {
        album: bytes.iter().map(|byte| release_message(*byte)).collect(),
        ..Default::default()
    }
}

#[test]
fn a_named_artist_answers_with_a_card_and_a_blank_name_answers_with_none() {
    let card = artist_card(&artist(artist_message(1, "Vela"))).expect("a card");
    assert_eq!(card.name, "Vela");
    assert_eq!(card.releases, 0);
    assert_eq!(card.portrait, None);

    assert_eq!(
        artist_card(&artist(artist_message(1, "   "))),
        None,
        "a blank name is no card"
    );
}

#[test]
fn the_card_counts_the_deduped_current_releases_and_carries_the_portrait() {
    let mut message = artist_message(1, "Vela");
    message.album_group.push(group(&[10]));
    message.album_group.push(group(&[11]));
    message.single_group.push(group(&[10, 12]));
    let mut portrait = Image::default();
    portrait.set_width(300);
    portrait.set_file_id(vec![9; 20]);
    message.portrait_group = MessageField::some(ImageGroup {
        image: vec![portrait],
        ..Default::default()
    });

    let card = artist_card(&artist(message)).expect("a card");
    assert_eq!(
        card.releases, 2,
        "the current variant only, deduped across groups: three groups, two current URIs, the single's second variant not current"
    );
    assert!(card.portrait.is_some());
}

#[test]
fn a_missing_card_falls_back_to_the_uri_unnamed_and_uncounted() {
    let entry = &artist_entries(&[artist_uri(1)], &HashMap::new())[0];
    assert_eq!(entry.uri, artist_uri(1));
    assert_eq!(entry.name, artist_uri(1));
    assert!(!entry.named);
    assert_eq!(entry.portrait, None);
    assert_eq!(entry.releases, None);
}

#[test]
fn a_carded_artist_reads_named_with_its_portrait_and_count() {
    let card = ArtistCard {
        name: "Vela".to_owned(),
        portrait: Some(CoverSet::single(
            CoverUrl::from_file_id(&[1; 20]).expect("a file id"),
        )),
        releases: 7,
    };
    let mut cards = HashMap::new();
    cards.insert(artist_uri(1), card);

    let entry = &artist_entries(&[artist_uri(1)], &cards)[0];
    assert!(entry.named);
    assert_eq!(entry.name, "Vela");
    assert_eq!(entry.releases, Some(7));
    assert!(entry.portrait.is_some());
}

/// One disc with the given track gid bytes.
fn disc(number: i32, tracks: &[u8]) -> DiscMessage {
    let mut message = DiscMessage::default();
    message.set_number(number);
    for byte in tracks {
        let mut track = TrackMessage::default();
        track.set_gid(gid(*byte));
        message.track.push(track);
    }
    message
}

/// One album message with a name, artist names and discs.
fn wire_album(name: &str, artists: &[&str], discs: &[DiscMessage]) -> AlbumMessage {
    let mut album = AlbumMessage::default();
    album.set_gid(gid(50));
    album.set_name(name.to_owned());
    for artist_name in artists {
        let mut artist = ArtistMessage::default();
        artist.set_gid(gid(60));
        artist.set_name((*artist_name).to_owned());
        album.artist.push(artist);
    }
    album.disc.extend(discs.iter().cloned());
    album
}

/// One batched answer holding `(entity uri, album)` pairs.
fn response(entities: &[(&str, AlbumMessage)]) -> BatchedExtensionResponse {
    let mut array = EntityExtensionDataArray::default();
    for (uri, album) in entities {
        let any = Any {
            value: album.write_to_bytes().expect("a serialised album"),
            ..Default::default()
        };
        array.extension_data.push(EntityExtensionData {
            entity_uri: (*uri).to_owned(),
            extension_data: MessageField::some(any),
            ..Default::default()
        });
    }
    BatchedExtensionResponse {
        extended_metadata: vec![array],
        ..Default::default()
    }
}

#[test]
fn an_unanswered_chunk_keeps_its_uris_unnamed_and_uncounted() {
    let order = vec![album_uri(10), album_uri(20)];
    let entries = saved_album_entries(&order, vec![None]);
    assert_eq!(entries.len(), 2, "every saved album still has a card");
    for entry in &entries {
        assert_eq!(entry.name, entry.uri);
        assert!(!entry.named);
        assert_eq!(entry.artists, "");
        assert_eq!(entry.year, None);
        assert_eq!(entry.cover, None);
        assert_eq!(entry.tracks, None);
    }
}

#[test]
fn a_present_but_empty_answer_keeps_its_uris_unnamed_and_uncounted() {
    let order = vec![album_uri(10), album_uri(20)];
    let entries = saved_album_entries(&order, vec![Some(response(&[]))]);
    assert_eq!(entries.len(), 2, "every saved album still has a card");
    for entry in &entries {
        assert_eq!(entry.name, entry.uri);
        assert!(!entry.named);
        assert_eq!(entry.artists, "");
        assert_eq!(entry.year, None);
        assert_eq!(entry.cover, None);
        assert_eq!(entry.tracks, None);
    }
}

#[test]
fn more_than_fifty_uris_span_two_chunks_and_a_failed_chunk_costs_only_its_albums() {
    let order: Vec<String> = (1..=51u8).map(album_uri).collect();
    let answer = response(&[(&album_uri(1), wire_album("First", &[], &[]))]);
    let entries = saved_album_entries(&order, vec![Some(answer), None]);

    assert_eq!(entries.len(), 51, "every saved album still has a card");
    assert_eq!(entries[0].name, "First");
    assert!(entries[0].named);
    for entry in &entries[1..] {
        assert_eq!(
            entry.name, entry.uri,
            "an unanswered album stays in the list, unnamed"
        );
        assert!(!entry.named);
        assert_eq!(entry.tracks, None);
    }
}

#[test]
fn a_short_answers_vector_pads_the_remaining_chunks_as_unanswered() {
    let order: Vec<String> = (1..=51u8).map(album_uri).collect();
    let answer = response(&[(&album_uri(1), wire_album("First", &[], &[]))]);
    // One answer for two chunks: the missing one must not cost its chunk.
    let entries = saved_album_entries(&order, vec![Some(answer)]);

    assert_eq!(
        entries.len(),
        51,
        "a short answers vector must not drop its tail"
    );
    assert_eq!(entries[0].name, "First");
    assert!(entries[0].named);
    for entry in &entries[1..] {
        assert_eq!(entry.name, entry.uri);
        assert!(!entry.named);
        assert_eq!(entry.tracks, None);
    }
}

#[test]
fn an_answered_album_reads_name_artists_year_cover_and_tracks() {
    let mut album = wire_album("Slow Tide", &["Vela"], &[disc(1, &[11, 12])]);
    let mut date = DateMessage::default();
    date.set_year(2024);
    album.date = MessageField::some(date);
    let mut image = Image::default();
    image.set_width(300);
    image.set_file_id((1..=20).collect::<Vec<u8>>());
    album.cover_group = MessageField::some(ImageGroup {
        image: vec![image],
        ..Default::default()
    });

    let order = vec![album_uri(50)];
    let entry = saved_album_entries(&order, vec![Some(response(&[(&album_uri(50), album)]))])
        .pop()
        .expect("an entry");

    assert_eq!(entry.name, "Slow Tide");
    assert!(entry.named);
    assert_eq!(entry.artists, "Vela");
    assert_eq!(entry.year, Some(2024));
    assert!(entry.cover.is_some());
    assert_eq!(entry.tracks, Some(2));
}

#[test]
fn a_disc_less_album_counts_zero_tracks_but_an_unanswered_one_counts_none() {
    let order = vec![album_uri(10), album_uri(20)];
    let answer = response(&[(&album_uri(10), wire_album("Record", &[], &[]))]);
    let entries = saved_album_entries(&order, vec![Some(answer)]);

    assert_eq!(entries[0].tracks, Some(0), "no discs is a count of zero");
    assert_eq!(entries[0].name, "Record");
    assert_eq!(entries[1].tracks, None, "unanswered is no count");
    assert_eq!(entries[1].name, album_uri(20));
    assert!(!entries[1].named);
}

#[test]
fn a_blank_artist_name_is_skipped_in_the_join() {
    let order = vec![album_uri(10)];
    let answer = response(&[(
        &album_uri(10),
        wire_album("Record", &["  ", "Vela", "Kite"], &[]),
    )]);
    let entry = saved_album_entries(&order, vec![Some(answer)])
        .pop()
        .expect("an entry");
    assert_eq!(entry.artists, "Vela, Kite", "blanks do not join");
}

#[test]
fn a_blank_wire_name_reads_unnamed_with_the_uri() {
    let order = vec![album_uri(10)];
    let answer = response(&[(&album_uri(10), wire_album("", &[], &[]))]);
    let entry = saved_album_entries(&order, vec![Some(answer)])
        .pop()
        .expect("an entry");
    assert_eq!(entry.name, album_uri(10));
    assert!(!entry.named);
    assert_eq!(entry.artists, "");
    assert_eq!(entry.tracks, Some(0));
}

/// One album message naming the artists `(gid byte, name)` in wire order; a
/// missing gid is an artist the wire named but did not identify.
fn credited_album(artists: &[(Option<u8>, &str)]) -> AlbumMessage {
    let mut album = AlbumMessage::default();
    album.set_gid(gid(50));
    album.set_name("Record".to_owned());
    for (byte, name) in artists {
        let mut artist = ArtistMessage::default();
        if let Some(byte) = byte {
            artist.set_gid(gid(*byte));
        }
        artist.set_name((*name).to_owned());
        album.artist.push(artist);
    }
    album
}

/// The saved-album entry one answered album message gives.
fn saved_album(album: AlbumMessage) -> SavedAlbum {
    let order = vec![album_uri(50)];
    saved_album_entries(&order, vec![Some(response(&[(&album_uri(50), album)]))])
        .pop()
        .expect("an entry")
}

#[test]
fn a_blank_artist_name_keeps_the_lists_aligned() {
    let entry = saved_album(credited_album(&[
        (Some(1), "Vela"),
        (Some(2), "   "),
        (Some(3), "Kite"),
    ]));

    assert_eq!(entry.artists, "Vela, Kite");
    assert_eq!(entry.artist_uris, vec![artist_uri(1), artist_uri(3)]);
    assert_eq!(
        entry.artist_names,
        vec!["Vela".to_owned(), "Kite".to_owned()],
        "the unnamed artist is in neither list, so the two stay indexed alike"
    );
    assert_eq!(entry.artist_uris.len(), entry.artist_names.len());
}

#[test]
fn an_artist_with_no_gid_is_in_neither_list() {
    let entry = saved_album(credited_album(&[(Some(1), "Vela"), (None, "Kite")]));

    assert_eq!(entry.artists, "Vela, Kite", "a named artist still reads");
    assert_eq!(entry.artist_uris, vec![artist_uri(1)]);
    assert_eq!(
        entry.artist_names,
        vec!["Vela".to_owned()],
        "an unresolvable id gives no link, and no name either"
    );
}

#[test]
fn the_joined_names_are_unchanged() {
    let entry = saved_album(credited_album(&[
        (Some(1), " Vela "),
        (Some(2), ""),
        (Some(3), "Kite"),
    ]));

    assert_eq!(
        entry.artists, "Vela, Kite",
        "trimmed, blanks dropped, the rest joined with a comma and a space"
    );
}

#[test]
fn two_artists_give_uris_in_wire_order() {
    let entry = saved_album(credited_album(&[(Some(7), "Kite"), (Some(3), "Vela")]));

    assert_eq!(entry.artist_uris, vec![artist_uri(7), artist_uri(3)]);
    assert_eq!(
        entry.artist_names,
        vec!["Kite".to_owned(), "Vela".to_owned()],
        "each name keeps the wire's order, not the id's"
    );
}

#[test]
fn an_unanswered_album_has_empty_lists() {
    let order = vec![album_uri(10)];
    let entry = saved_album_entries(&order, vec![None])
        .pop()
        .expect("an entry");

    assert_eq!(entry.artists, "");
    assert!(entry.artist_uris.is_empty());
    assert!(entry.artist_names.is_empty());
}
