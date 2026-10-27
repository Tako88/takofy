use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Artist;
use librespot::protocol::entity_extension_data::EntityExtensionData;
use librespot::protocol::extended_metadata::{BatchedExtensionResponse, EntityExtensionDataArray};
use librespot::protocol::metadata::album::Type as AlbumType;
use librespot::protocol::metadata::{
    Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
    Date as DateMessage, Disc as DiscMessage, Image, ImageGroup, TopTracks as TopTracksMessage,
    Track as TrackMessage,
};
use protobuf::Message as _;
use protobuf::MessageField;
use protobuf::well_known_types::any::Any;
use takofy_model::{CoverSet, CoverUrl, ReleaseKind};

use super::{
    RELATED_MAX, RELEASE_CHUNK, TOP_TRACKS, detail, portrait_of, related_uris, release_chunks,
    release_of, release_uris, releases_from, releases_of, top_track_positions,
};

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

/// One related-artist entry carrying nothing but a gid.
fn related_message(byte: u8) -> ArtistMessage {
    let mut message = ArtistMessage::default();
    message.set_gid(gid(byte));
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

/// A top-tracks entry for one country.
fn top_tracks(country: &str, track_bytes: &[u8]) -> TopTracksMessage {
    let mut entry = TopTracksMessage::default();
    entry.set_country(country.to_owned());
    for byte in track_bytes {
        let mut track = TrackMessage::default();
        track.set_gid(gid(*byte));
        entry.track.push(track);
    }
    entry
}

/// The parsed artist the page reads.
fn artist(message: ArtistMessage) -> Artist {
    Artist::try_from(&message).expect("a valid artist message")
}

fn empty_artist_message() -> ArtistMessage {
    let mut message = ArtistMessage::default();
    message.set_gid(gid(1));
    message.set_name("Vela".to_owned());
    message
}

#[test]
fn release_uris_read_albums_then_singles_then_compilations() {
    let mut message = empty_artist_message();
    message.album_group.push(group(&[10]));
    message.single_group.push(group(&[20]));
    message.compilation_group.push(group(&[30]));
    message.appears_on_group.push(group(&[40]));

    assert_eq!(
        release_uris(&artist(message)),
        vec![album_uri(10), album_uri(20), album_uri(30)],
        "appears_on is not the artist's own discography"
    );
}

#[test]
fn release_uris_deduplicate_across_groups_and_variants() {
    let mut message = empty_artist_message();
    message.album_group.push(group(&[10, 11]));
    message.album_group.push(group(&[12]));
    message.single_group.push(group(&[10]));

    assert_eq!(
        release_uris(&artist(message)),
        vec![album_uri(10), album_uri(12)],
        "the current variant only, and the first sighting wins"
    );
}

#[test]
fn release_uris_of_an_artist_with_no_releases_is_empty() {
    assert!(release_uris(&artist(empty_artist_message())).is_empty());
}

#[test]
fn related_uris_deduplicate_by_first_sight() {
    let mut message = empty_artist_message();
    message.related.push(related_message(2));
    message.related.push(related_message(3));
    message.related.push(related_message(2));

    assert_eq!(
        related_uris(&artist(message)),
        vec![artist_uri(2), artist_uri(3)],
        "the first sighting wins"
    );
}

#[test]
fn related_uris_drop_the_artist_itself() {
    let mut message = empty_artist_message();
    message.related.push(related_message(1));
    message.related.push(related_message(2));

    assert_eq!(
        related_uris(&artist(message)),
        vec![artist_uri(2)],
        "an artist is not its own relative"
    );
}

#[test]
fn related_uris_drop_an_entry_that_does_not_name_an_artist() {
    let mut message = empty_artist_message();
    message.related.push(related_message(2));
    let mut artist = artist(message);
    artist.related.0[0].id = SpotifyUri::Album {
        id: SpotifyId::from_raw(&gid(2)).expect("a 16-byte id"),
    };

    assert!(
        related_uris(&artist).is_empty(),
        "a card that cannot open an artist is not one"
    );
}

#[test]
fn related_uris_stop_at_the_cap() {
    let mut message = empty_artist_message();
    for byte in 2..=21 {
        message.related.push(related_message(byte));
    }

    let uris = related_uris(&artist(message));
    assert_eq!(uris.len(), RELATED_MAX);
    assert_eq!(uris.first(), Some(&artist_uri(2)));
    assert_eq!(uris.last(), Some(&artist_uri(13)));
}

#[test]
fn related_uris_of_an_artist_with_no_relatives_are_empty() {
    assert!(related_uris(&artist(empty_artist_message())).is_empty());
}

#[test]
fn top_tracks_read_the_countrys_entry() {
    let mut message = empty_artist_message();
    message.top_track.push(top_tracks("US", &[10, 11]));
    message.top_track.push(top_tracks("", &[12]));

    let positions = top_track_positions(&artist(message), "US");
    assert_eq!(
        positions
            .into_iter()
            .map(|position| position.uri)
            .collect::<Vec<_>>(),
        vec![track_uri(10), track_uri(11)]
    );
}

#[test]
fn top_tracks_fall_back_to_the_empty_country_entry() {
    let mut message = empty_artist_message();
    message.top_track.push(top_tracks("", &[12]));

    let positions = top_track_positions(&artist(message), "DE");
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].uri, track_uri(12));
}

#[test]
fn top_tracks_with_no_matching_or_empty_entry_are_empty() {
    let mut message = empty_artist_message();
    message.top_track.push(top_tracks("US", &[10]));

    assert!(top_track_positions(&artist(message), "DE").is_empty());
    assert!(top_track_positions(&artist(empty_artist_message()), "US").is_empty());
}

#[test]
fn top_tracks_carry_no_added_date_and_stop_at_ten() {
    let mut message = empty_artist_message();
    message
        .top_track
        .push(top_tracks("US", &(1..=12).collect::<Vec<u8>>()));

    let positions = top_track_positions(&artist(message), "US");
    assert_eq!(positions.len(), TOP_TRACKS);
    assert!(
        positions.iter().all(|position| position.added_at.is_none()),
        "an artist's top list is not a membership"
    );
}

/// A release message with everything a block draws.
fn wire_album(name: &str, kind: AlbumType, year: i32, discs: &[(i32, &[u8])]) -> AlbumMessage {
    let mut album = AlbumMessage::default();
    album.set_gid(gid(50));
    album.set_name(name.to_owned());
    album.set_type(kind);
    for (number, tracks) in discs {
        let mut disc = DiscMessage::default();
        disc.set_number(*number);
        for byte in *tracks {
            let mut track = TrackMessage::default();
            track.set_gid(gid(*byte));
            disc.track.push(track);
        }
        album.disc.push(disc);
    }
    let mut date = DateMessage::default();
    date.set_year(year);
    album.date = MessageField::some(date);
    album
}

#[test]
fn release_of_reads_the_wires_own_facts() {
    let album = wire_album("Slow Tide", AlbumType::ALBUM, 2024, &[(1, &[10, 11])]);
    let release = release_of(&album_uri(50), &album);

    assert_eq!(release.uri, album_uri(50));
    assert_eq!(release.name, "Slow Tide");
    assert_eq!(release.kind, ReleaseKind::Album);
    assert_eq!(release.year, Some(2024));
    assert_eq!(release.tracks, vec![track_uri(10), track_uri(11)]);
}

#[test]
fn an_empty_release_name_falls_back_to_the_uri() {
    let album = wire_album("", AlbumType::SINGLE, 2024, &[]);
    assert_eq!(
        release_of("spotify:album:x", &album).name,
        "spotify:album:x"
    );
}

#[test]
fn release_of_names_every_kind_it_can_draw() {
    for (kind, expected) in [
        (AlbumType::ALBUM, ReleaseKind::Album),
        (AlbumType::EP, ReleaseKind::Ep),
        (AlbumType::SINGLE, ReleaseKind::Single),
        (AlbumType::COMPILATION, ReleaseKind::Compilation),
        (AlbumType::AUDIOBOOK, ReleaseKind::Other),
        (AlbumType::PODCAST, ReleaseKind::Other),
    ] {
        let album = wire_album("Release", kind, 2024, &[]);
        assert_eq!(
            release_of("spotify:album:x", &album).kind,
            expected,
            "{kind:?}"
        );
    }
}

#[test]
fn a_zero_or_negative_release_year_is_no_year() {
    for year in [0, -1] {
        let album = wire_album("Release", AlbumType::ALBUM, year, &[]);
        assert_eq!(release_of("spotify:album:x", &album).year, None, "{year}");
    }
}

#[test]
fn a_release_cover_comes_from_the_cover_group() {
    let mut album = wire_album("Release", AlbumType::ALBUM, 2024, &[]);
    let mut image = Image::default();
    image.set_file_id((1..=20).collect::<Vec<u8>>());
    image.set_width(300);
    album.cover_group = MessageField::some(ImageGroup {
        image: vec![image],
        ..Default::default()
    });

    let cover = release_of("spotify:album:x", &album).cover;
    assert!(
        cover
            .as_ref()
            .and_then(|cover| cover.variants().first())
            .is_some_and(|variant| variant
                .url
                .as_str()
                .ends_with("0102030405060708090a0b0c0d0e0f1011121314")),
        "{cover:?}"
    );
}

#[test]
fn release_tracks_flatten_in_disc_number_order() {
    let album = wire_album(
        "Release",
        AlbumType::ALBUM,
        2024,
        &[(2, &[12, 13]), (1, &[10, 11])],
    );
    assert_eq!(
        release_of("spotify:album:x", &album).tracks,
        vec![track_uri(10), track_uri(11), track_uri(12), track_uri(13)],
        "disc one reads first however the wire listed the discs"
    );
}

#[test]
fn a_malformed_disc_track_gid_is_skipped() {
    let mut album = wire_album("Release", AlbumType::ALBUM, 2024, &[(1, &[10])]);
    album.disc[0].track.push(TrackMessage {
        gid: Some(vec![7; 15]),
        ..Default::default()
    });

    assert_eq!(
        release_of("spotify:album:x", &album).tracks,
        vec![track_uri(10)],
        "one unreadable gid is not a reason to lose the release"
    );
}

#[test]
fn the_portrait_comes_from_the_portrait_group_only() {
    let mut message = empty_artist_message();
    let mut portrait = Image::default();
    portrait.set_size(librespot::protocol::metadata::image::Size::DEFAULT);
    portrait.set_width(300);
    portrait.set_height(300);
    portrait.set_file_id(vec![9; 20]);
    message.portrait.push(portrait.clone());
    message.portrait_group = MessageField::some(ImageGroup {
        image: vec![portrait],
        ..Default::default()
    });

    let products = portrait_of(&artist(message));
    assert_eq!(
        products,
        CoverSet::from_sizes([(
            Some((300, 300)),
            CoverUrl::from_wire(&format!("https://i.scdn.co/image/{}", "09".repeat(20)))
                .expect("a cover url"),
        )])
    );
}

#[test]
fn an_artist_without_a_portrait_group_has_no_portrait() {
    let mut message = empty_artist_message();
    message.portrait.push(Image::default());
    assert_eq!(portrait_of(&artist(message)), None);
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
fn releases_from_reads_the_request_order_not_the_answers() {
    let order = vec![album_uri(10), album_uri(20), album_uri(30)];
    let answer = response(&[
        (
            &album_uri(30),
            wire_album("Third", AlbumType::ALBUM, 2020, &[]),
        ),
        (
            &album_uri(10),
            wire_album("First", AlbumType::ALBUM, 2018, &[]),
        ),
        (
            &album_uri(20),
            wire_album("Second", AlbumType::EP, 2019, &[]),
        ),
    ]);

    let names: Vec<String> = releases_from(&answer, &order)
        .into_iter()
        .map(|release| release.name)
        .collect();
    assert_eq!(names, vec!["First", "Second", "Third"]);
}

#[test]
fn a_non_parsing_entity_and_an_unanswered_uri_are_skipped() {
    let order = vec![album_uri(10), album_uri(20), album_uri(30)];
    let mut array = EntityExtensionDataArray::default();

    let good = Any {
        value: wire_album("First", AlbumType::ALBUM, 2018, &[])
            .write_to_bytes()
            .expect("a serialised album"),
        ..Default::default()
    };
    array.extension_data.push(EntityExtensionData {
        entity_uri: album_uri(10),
        extension_data: MessageField::some(good),
        ..Default::default()
    });

    // A truncated varint field: not an album message.
    let broken = Any {
        value: vec![0x08],
        ..Default::default()
    };
    array.extension_data.push(EntityExtensionData {
        entity_uri: album_uri(20),
        extension_data: MessageField::some(broken),
        ..Default::default()
    });

    let answer = BatchedExtensionResponse {
        extended_metadata: vec![array],
        ..Default::default()
    };

    let releases = releases_from(&answer, &order);
    assert_eq!(releases.len(), 1, "the unanswered URI is skipped too");
    assert_eq!(releases[0].name, "First");
}

#[test]
fn three_hundred_release_uris_are_split_into_full_chunks() {
    let order: Vec<String> = (0..300)
        .map(|index| format!("spotify:album:{index}"))
        .collect();

    let chunks: Vec<&[String]> = release_chunks(&order).collect();

    assert_eq!(chunks.len(), 300 / RELEASE_CHUNK, "one request per chunk");
    assert_eq!(chunks.len(), 6, "300 URIs at {RELEASE_CHUNK} each");
    assert!(
        chunks.iter().all(|chunk| !chunk.is_empty()),
        "a request is never sent empty"
    );
    assert!(
        chunks.iter().all(|chunk| chunk.len() <= RELEASE_CHUNK),
        "no request asks for more than the bound allows"
    );
    assert_eq!(
        chunks.iter().map(|chunk| chunk.len()).sum::<usize>(),
        300,
        "every URI is asked for exactly once"
    );
}

#[test]
fn a_short_answer_still_maps_the_releases_that_answered() {
    let order = vec![album_uri(10), album_uri(20), album_uri(30)];
    let answer = response(&[
        (
            &album_uri(10),
            wire_album("First", AlbumType::ALBUM, 2018, &[]),
        ),
        (
            &album_uri(20),
            wire_album("Second", AlbumType::ALBUM, 2019, &[]),
        ),
    ]);

    let names: Vec<String> = releases_of(&order, vec![Some(answer)])
        .into_iter()
        .map(|release| release.name)
        .collect();

    assert_eq!(
        names,
        vec!["First", "Second"],
        "the unanswered URI costs only its own release"
    );
}

#[test]
fn detail_falls_back_to_the_uri_for_a_blank_name() {
    let mut message = empty_artist_message();
    message.set_name("   ".to_owned());
    let detail = detail("spotify:artist:x", &artist(message), Vec::new(), Vec::new());
    assert_eq!(detail.name, "spotify:artist:x");
    assert_eq!(detail.uri, "spotify:artist:x");
    assert!(detail.releases.is_empty());
    assert!(detail.related.is_empty());
}

#[test]
fn detail_carries_the_payloads_related_uris() {
    let mut message = empty_artist_message();
    message.related.push(related_message(2));
    message.related.push(related_message(1));

    let detail = detail("spotify:artist:x", &artist(message), Vec::new(), Vec::new());
    assert_eq!(
        detail.related,
        vec![artist_uri(2)],
        "the artist itself is not a relative"
    );
}
