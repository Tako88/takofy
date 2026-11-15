use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Artist;
use librespot::protocol::metadata::{
    Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
    Date as DateMessage, Image, ImageGroup, TopTracks as TopTracksMessage,
};
use protobuf::Message as _;
use protobuf::MessageField;
use takofy_model::home::DiscoveryArtist;
use takofy_model::{Release, ReleaseKind, TrackEntry};

use super::{album_from, artist_from, assemble, newest_year, own_releases, top_titles_of};

/// A 16-byte gid. `SpotifyId::from_raw` rejects anything else.
fn gid(byte: u8) -> Vec<u8> {
    vec![byte; 16]
}

/// The URI a track gid byte resolves to.
fn track_uri(byte: u8) -> String {
    SpotifyUri::Track {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

/// The URI an album gid byte resolves to.
fn album_uri(byte: u8) -> String {
    SpotifyUri::Album {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

/// The URI an artist gid byte resolves to.
fn artist_uri(byte: u8) -> String {
    SpotifyUri::Artist {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

/// One top-tracks entry for one country.
fn top_tracks(country: &str, track_bytes: &[u8]) -> TopTracksMessage {
    let mut entry = TopTracksMessage::default();
    entry.set_country(country.to_owned());
    for byte in track_bytes {
        let mut track = librespot::protocol::metadata::Track::default();
        track.set_gid(gid(*byte));
        entry.track.push(track);
    }
    entry
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

/// An artist message naming one artist, and nothing else.
fn artist_message() -> ArtistMessage {
    let mut message = ArtistMessage::default();
    message.set_gid(gid(1));
    message.set_name("Vela".to_owned());
    message
}

/// The parsed artist the card path reads.
fn artist(message: ArtistMessage) -> Artist {
    Artist::try_from(&message).expect("a valid artist message")
}

/// One portrait tile in the group the card draws.
fn portrait(byte: u8) -> ImageGroup {
    let mut image = Image::default();
    image.set_file_id(vec![byte; 20]);
    image.set_width(300);
    image.set_height(300);
    ImageGroup {
        image: vec![image],
        ..Default::default()
    }
}

/// An album message with a name and a date, as one batched answer carries it.
///
/// A month and a day of zero is the wire's partial date: the year is known and
/// the rest is not.
fn dated(year: i32, month: i32, day: i32) -> AlbumMessage {
    let mut date = DateMessage::default();
    date.set_year(year);
    date.set_month(month);
    date.set_day(day);
    let mut album = AlbumMessage::default();
    album.set_name("Record".to_owned());
    album.date = MessageField::some(date);
    album
}

/// A dated release, for the year rule alone.
fn release(uri: &str, year: Option<u16>) -> Release {
    Release {
        uri: uri.to_owned(),
        name: "Record".to_owned(),
        kind: ReleaseKind::Album,
        year,
        cover: None,
        tracks: Vec::new(),
    }
}

/// One labelled top track, as `label_tracks` hands it back.
fn entry(uri: &str, title: &str, named: bool) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: "Vela".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named,
        cover: None,
    }
}

/// An artist whose only top track is `track_bytes` for `country`.
fn artist_with_top(country: &str, track_bytes: &[u8]) -> Artist {
    let mut message = artist_message();
    message.top_track.push(top_tracks(country, track_bytes));
    artist(message)
}

#[test]
fn artist_from_reads_the_artist_one_batched_answer_carries() {
    let mut message = artist_message();
    message.portrait_group = MessageField::some(portrait(7));
    let bytes = message.write_to_bytes().expect("an encoded artist");

    let artist = artist_from(&bytes, "spotify:artist:whatever").expect("an artist");

    assert_eq!(artist.name, "Vela");
    assert!(
        !artist.portrait_group.is_empty(),
        "the card's picture comes from this answer"
    );
}

#[test]
fn artist_from_refuses_bytes_that_are_not_an_artist() {
    assert!(
        artist_from(&[0xff, 0x00], "spotify:artist:x").is_none(),
        "an answer that does not parse is no artist"
    );
}

#[test]
fn a_card_without_a_playable_top_track_is_dropped() {
    let years = HashMap::new();
    assert_eq!(
        assemble(
            &artist_uri(1),
            &artist(artist_message()),
            "US",
            &years,
            &HashMap::new()
        ),
        None,
        "a recommendation that cannot be played is not drawn"
    );
}

#[test]
fn a_card_carries_at_most_three_tracks_in_the_wires_order() {
    let mut message = artist_message();
    message
        .top_track
        .push(top_tracks("US", &[10, 11, 12, 13, 14]));
    let artist = artist(message);
    let labelled = HashMap::from([
        (track_uri(10), entry(&track_uri(10), "One", true)),
        (track_uri(11), entry(&track_uri(11), "Two", true)),
        (track_uri(12), entry(&track_uri(12), "Three", true)),
        (track_uri(13), entry(&track_uri(13), "Four", true)),
    ]);

    let card = assemble(&artist_uri(1), &artist, "US", &HashMap::new(), &labelled)
        .expect("a playable card");

    assert_eq!(
        card.tracks
            .iter()
            .map(|track| track.uri.clone())
            .collect::<Vec<_>>(),
        vec![track_uri(10), track_uri(11), track_uri(12)],
        "the card keeps the wire's first three positions and no more"
    );
    assert_eq!(card.tracks[0].title, "One");
}

#[test]
fn a_position_with_no_label_keeps_an_unnamed_entry_in_place() {
    let mut message = artist_message();
    message.top_track.push(top_tracks("US", &[10, 11, 12]));
    let artist = artist(message);
    let labelled = HashMap::from([
        (track_uri(10), entry(&track_uri(10), "One", true)),
        (track_uri(12), entry(&track_uri(12), "Three", true)),
    ]);

    let card = assemble(&artist_uri(1), &artist, "US", &HashMap::new(), &labelled)
        .expect("a playable card");

    assert_eq!(card.tracks.len(), 3, "a playable position is never lost");
    assert_eq!(card.tracks[1].uri, track_uri(11));
    assert!(
        !card.tracks[1].named,
        "the missing label is the URI fallback"
    );
}

#[test]
fn the_country_entry_beats_the_empty_country_one() {
    let mut message = artist_message();
    message.top_track.push(top_tracks("US", &[10]));
    message.top_track.push(top_tracks("", &[12]));
    let artist = artist(message);

    let card = assemble(
        &artist_uri(1),
        &artist,
        "US",
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("a playable card");

    assert_eq!(card.tracks[0].uri, track_uri(10));
}

#[test]
fn the_empty_country_entry_stands_in_when_the_country_has_none() {
    let mut message = artist_message();
    message.top_track.push(top_tracks("", &[12]));
    let artist = artist(message);

    let card = assemble(
        &artist_uri(1),
        &artist,
        "GB",
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("a playable card");

    assert_eq!(card.tracks[0].uri, track_uri(12));
}

#[test]
fn a_card_carries_the_name_portrait_track_title_and_year() {
    let mut message = artist_message();
    message.portrait_group = MessageField::some(portrait(7));
    message.top_track.push(top_tracks("US", &[10, 11]));
    let artist = artist(message);
    let years = HashMap::from([(artist_uri(1), 2001)]);
    let labelled = HashMap::from([
        (track_uri(10), entry(&track_uri(10), "Song", true)),
        (track_uri(11), entry(&track_uri(11), "Other", true)),
    ]);

    let card = assemble(&artist_uri(1), &artist, "US", &years, &labelled).expect("a playable card");

    assert_eq!(
        card.artist,
        DiscoveryArtist {
            uri: artist_uri(1),
            name: "Vela".to_owned(),
        }
    );
    assert!(card.portrait.is_some(), "the group is the card's picture");
    assert_eq!(
        card.tracks[0].uri,
        track_uri(10),
        "the first playable position"
    );
    assert_eq!(card.tracks[0].title, "Song");
    assert_eq!(card.tracks[1].uri, track_uri(11));
    assert_eq!(card.latest_year, Some(2001));
}

#[test]
fn an_artist_with_no_portrait_group_has_no_portrait() {
    let artist = artist_with_top("US", &[10]);

    let card = assemble(
        &artist_uri(1),
        &artist,
        "US",
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("a playable card");

    assert_eq!(card.portrait, None);
}

#[test]
fn a_blank_name_falls_back_to_the_uri() {
    let mut message = artist_message();
    message.set_name("   ".to_owned());
    message.top_track.push(top_tracks("US", &[10]));
    let artist = artist(message);

    let card = assemble(
        &artist_uri(1),
        &artist,
        "US",
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("a playable card");

    assert_eq!(card.artist.name, artist_uri(1));
}

#[test]
fn a_partial_date_gives_its_year() {
    let bytes = dated(1996, 0, 0)
        .write_to_bytes()
        .expect("an encoded album");

    let release = album_from(&bytes, "spotify:album:x").expect("a release");

    assert_eq!(
        release.year,
        Some(1996),
        "1996-00-00 is a year with no month, not no year"
    );
    assert_eq!(newest_year(&[release]), Some(1996));
}

#[test]
fn a_zero_year_is_ignored() {
    let bytes = dated(0, 0, 0).write_to_bytes().expect("an encoded album");

    assert_eq!(
        album_from(&bytes, "spotify:album:x")
            .expect("a release")
            .year,
        None
    );
    assert_eq!(
        newest_year(&[release("spotify:album:x", Some(0))]),
        None,
        "zero is the wire saying nothing, not a record"
    );
}

#[test]
fn newest_year_takes_the_maximum_and_skips_undated_releases() {
    let releases = [
        release(&album_uri(1), Some(1996)),
        release(&album_uri(2), None),
        release(&album_uri(3), Some(2019)),
    ];

    assert_eq!(newest_year(&releases), Some(2019));
}

#[test]
fn newest_year_of_no_releases_is_none() {
    assert_eq!(newest_year(&[]), None);
}

#[test]
fn an_album_the_artist_only_appears_on_is_never_dated() {
    let mut message = artist_message();
    message.appears_on_group.push(group(&[40]));
    let artist = artist(message);
    let mut albums = HashMap::new();
    albums.insert(album_uri(40), release(&album_uri(40), Some(2001)));

    assert!(own_releases(&artist, &albums).is_empty());
    assert_eq!(newest_year(&own_releases(&artist, &albums)), None);
}

#[test]
fn own_releases_keep_the_artists_order_and_skip_what_nobody_answered() {
    let mut message = artist_message();
    message.album_group.push(group(&[10]));
    message.album_group.push(group(&[11]));
    message.single_group.push(group(&[20]));
    let artist = artist(message);
    let mut albums = HashMap::new();
    albums.insert(album_uri(10), release(&album_uri(10), Some(1996)));
    albums.insert(album_uri(20), release(&album_uri(20), Some(2019)));

    let own = own_releases(&artist, &albums);

    assert_eq!(
        own.iter()
            .map(|release| release.uri.clone())
            .collect::<Vec<_>>(),
        vec![album_uri(10), album_uri(20)],
        "the artist's own order, with an unanswered album left out"
    );
    assert_eq!(newest_year(&own), Some(2019));
}

#[test]
fn an_artist_with_no_releases_has_no_latest_year() {
    let artist = artist_with_top("US", &[10]);

    let card = assemble(
        &artist_uri(1),
        &artist,
        "US",
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("a playable card");

    assert_eq!(card.latest_year, None);
}

#[test]
fn top_titles_keep_the_wires_order() {
    let mut message = artist_message();
    message.top_track.push(top_tracks("US", &[12, 10]));
    let artist = artist(message);
    let labelled = HashMap::from([
        (track_uri(10), entry(&track_uri(10), "Second", true)),
        (track_uri(12), entry(&track_uri(12), "First", true)),
    ]);

    assert_eq!(
        top_titles_of(&artist, "US", &labelled),
        vec!["First".to_owned(), "Second".to_owned()]
    );
}

#[test]
fn top_titles_skip_a_position_the_labels_could_not_name() {
    let mut message = artist_message();
    message.top_track.push(top_tracks("US", &[10, 11, 12]));
    let artist = artist(message);
    let labelled = HashMap::from([
        (track_uri(10), entry(&track_uri(10), "First", true)),
        // Unnamed: its title is the URI fallback, which matches nothing outside.
        (track_uri(11), entry(&track_uri(11), &track_uri(11), false)),
        (track_uri(12), entry(&track_uri(12), "Third", true)),
    ]);

    assert_eq!(
        top_titles_of(&artist, "US", &labelled),
        vec!["First".to_owned(), "Third".to_owned()]
    );
}

#[test]
fn top_titles_skip_a_title_that_is_only_whitespace() {
    let mut message = artist_message();
    message.top_track.push(top_tracks("US", &[10]));
    let artist = artist(message);
    let labelled = HashMap::from([(track_uri(10), entry(&track_uri(10), "  ", true))]);

    assert!(top_titles_of(&artist, "US", &labelled).is_empty());
}

#[test]
fn top_titles_of_an_artist_with_no_top_list_are_empty() {
    assert!(top_titles_of(&artist(artist_message()), "US", &HashMap::new()).is_empty());
}
