//! An artist as its page draws it.
//!
//! Pure: a parsed [`Artist`] and the protocol albums one batched request
//! answered with in, an [`ArtistDetail`] out. The request itself is in
//! `remote::read_artist`.
//!
//! The artist payload names its releases by URI only, so everything a release
//! block says — its name, its kind, its year, its cover, its track list — comes
//! from the batched album metadata, not from the artist. Those answers arrive in
//! no particular order, so they are mapped back by `entity_uri` and then read in
//! the order the release list asked for.

use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Artist;
use librespot::protocol::extended_metadata::BatchedExtensionResponse;
use librespot::protocol::metadata::Album as AlbumMessage;
use librespot::protocol::metadata::album::Type as AlbumType;
use protobuf::Message as _;
use takofy_model::{ArtistDetail, CoverUrl, Release, ReleaseKind};

use crate::context::PageTrack;
use crate::cover::{pick_cover, pick_images_cover};

/// Why an artist read failed.
#[derive(Debug, thiserror::Error)]
pub enum ArtistError {
    /// The URI does not name an artist, so there is nothing to ask for.
    #[error("not an artist URI")]
    NotAnArtist,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// How many rows the Popular list draws.
///
/// The wire's top list is already ten entries, but a payload that carries more
/// is cut here: the reference's Popular list is a preview, not the discography.
pub(crate) const TOP_TRACKS: usize = 10;

/// How many release URIs one `ALBUM_V4` request asks for.
///
/// An artist payload can name a few hundred releases, and one request over all
/// of them can answer only part of the set. Fifty keeps each request inside what
/// the provider answers in full, so a short answer costs its own chunk rather
/// than the discography.
pub(crate) const RELEASE_CHUNK: usize = 50;

/// The release URIs split into the requests one read sends.
pub(crate) fn release_chunks(order: &[String]) -> impl Iterator<Item = &[String]> + '_ {
    order.chunks(RELEASE_CHUNK)
}

/// Every release URI the payload names, in the order the page draws them.
///
/// Albums, then singles, then compilations, each group's current variant only,
/// so a re-release does not appear beside the release it replaces. `appears_on`
/// is not the artist's own discography and is left out. A URI that turns up in
/// two groups is kept where it was first seen.
pub(crate) fn release_uris(artist: &Artist) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    artist
        .albums_current()
        .chain(artist.singles_current())
        .chain(artist.compilations_current())
        .filter_map(|uri| uri.to_uri().ok())
        .filter(|uri| seen.insert(uri.clone()))
        .collect()
}

/// The artist's top tracks for `country`, as playable positions.
///
/// The wire's country entry wins, the empty-country entry is the fallback, and
/// nothing is the answer when neither exists. An artist's own top list is not a
/// membership, so no position carries an added date.
pub(crate) fn top_track_positions(artist: &Artist, country: &str) -> Vec<PageTrack> {
    artist
        .top_tracks
        .for_country(country)
        .iter()
        .filter_map(|uri| match uri {
            SpotifyUri::Track { .. } => uri.to_uri().ok(),
            _ => None,
        })
        .take(TOP_TRACKS)
        .map(|uri| PageTrack {
            uri,
            added_at: None,
        })
        .collect()
}

/// The artist's page data, from the artist and its labelled top tracks.
///
/// The name falls back to the URI exactly as an album's does. A blank name is
/// the wire saying nothing rather than a name.
pub(crate) fn detail(
    uri: &str,
    artist: &Artist,
    top_tracks: Vec<takofy_model::TrackEntry>,
    releases: Vec<Release>,
) -> ArtistDetail {
    let name = if artist.name.trim().is_empty() {
        uri.to_owned()
    } else {
        artist.name.clone()
    };
    ArtistDetail {
        uri: uri.to_owned(),
        name,
        portrait: portrait_of(artist),
        top_tracks,
        releases,
    }
}

/// The portrait the hero draws.
///
/// Only `portrait_group` is read: `portraits` came back empty on every artist
/// the probe asked, and `is_portrait_album_cover` is a hint about one of the
/// group's tiles rather than a second source.
pub(crate) fn portrait_of(artist: &Artist) -> Option<CoverUrl> {
    pick_images_cover(&artist.portrait_group)
}

/// One release, from the URI the artist named and the album the batched answer
/// carried.
///
/// The name falls back to the URI; the kind is the wire's own, with the kinds
/// the reference has no word for collapsed into [`ReleaseKind::Other`]; the year
/// is refused when it is zero or negative, exactly as the album band does.
pub(crate) fn release_of(uri: &str, album: &AlbumMessage) -> Release {
    Release {
        uri: uri.to_owned(),
        name: album
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| uri.to_owned()),
        kind: kind_of(album.type_()),
        year: u16::try_from(album.date.as_ref().map_or(0, |date| date.year()))
            .ok()
            .filter(|year| *year != 0),
        cover: pick_cover(album.cover_group.as_ref()),
        tracks: track_uris(album),
    }
}

/// The wire's album type as the block labels it.
fn kind_of(kind: AlbumType) -> ReleaseKind {
    match kind {
        AlbumType::ALBUM => ReleaseKind::Album,
        AlbumType::EP => ReleaseKind::Ep,
        AlbumType::SINGLE => ReleaseKind::Single,
        AlbumType::COMPILATION => ReleaseKind::Compilation,
        AlbumType::AUDIOBOOK | AlbumType::PODCAST => ReleaseKind::Other,
    }
}

/// The release's track URIs, flattened by disc number.
///
/// The discs are ordered by their own number with a stable sort, so a wire that
/// lists disc two first still reads one first, and the positions are numbered by
/// their place in the result. A gid that is not a track id is left out rather
/// than guessed at.
fn track_uris(album: &AlbumMessage) -> Vec<String> {
    let mut discs: Vec<_> = album.disc.iter().collect();
    discs.sort_by_key(|disc| disc.number);
    discs
        .into_iter()
        .flat_map(|disc| disc.track.iter())
        .filter_map(|track| {
            let id = SpotifyId::from_raw(track.gid.as_deref()?).ok()?;
            SpotifyUri::Track { id }.to_uri().ok()
        })
        .collect()
}

/// The albums a batched answer carries, keyed by the URI the answer echoes.
///
/// An entity that does not parse is skipped, so one unreadable album does not
/// cost the page the others in the same answer.
pub(crate) fn answered_albums(
    response: &BatchedExtensionResponse,
) -> HashMap<String, AlbumMessage> {
    let mut answered: HashMap<String, AlbumMessage> = HashMap::new();
    for array in &response.extended_metadata {
        for data in &array.extension_data {
            let Some(any) = data.extension_data.as_ref() else {
                continue;
            };
            let Ok(album) = AlbumMessage::parse_from_bytes(&any.value) else {
                continue;
            };
            answered.insert(data.entity_uri.clone(), album);
        }
    }
    answered
}

/// The releases the batched answer carries, in the request's own order.
///
/// The answer's order is not the request's, so each entity is keyed by the URI
/// the answer echoes and the request's order is walked. An entity that does not
/// parse, and a URI nobody answered, are both skipped: one bad album must not
/// cost the page the other forty-seven.
pub(crate) fn releases_from(response: &BatchedExtensionResponse, order: &[String]) -> Vec<Release> {
    let mut answered = answered_albums(response);
    order
        .iter()
        .filter_map(|uri| answered.remove(uri).map(|album| release_of(uri, &album)))
        .collect()
}

/// The releases a chunked read answered, in the request's own order.
///
/// Each answer is mapped against its own chunk, so a chunk that failed or came
/// back short costs only its releases rather than the whole discography.
pub(crate) fn releases_of(
    order: &[String],
    answers: Vec<Option<BatchedExtensionResponse>>,
) -> Vec<Release> {
    release_chunks(order)
        .zip(answers)
        .flat_map(|(chunk, answer)| match answer {
            Some(response) => releases_from(&response, chunk),
            None => Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use librespot::core::{SpotifyId, SpotifyUri};
    use librespot::metadata::Artist;
    use librespot::protocol::entity_extension_data::EntityExtensionData;
    use librespot::protocol::extended_metadata::{
        BatchedExtensionResponse, EntityExtensionDataArray,
    };
    use librespot::protocol::metadata::album::Type as AlbumType;
    use librespot::protocol::metadata::{
        Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
        Date as DateMessage, Disc as DiscMessage, Image, ImageGroup, TopTracks as TopTracksMessage,
        Track as TrackMessage,
    };
    use protobuf::Message as _;
    use protobuf::MessageField;
    use protobuf::well_known_types::any::Any;
    use takofy_model::{CoverUrl, ReleaseKind};

    use super::{
        RELEASE_CHUNK, TOP_TRACKS, detail, portrait_of, release_chunks, release_of, release_uris,
        releases_from, releases_of, top_track_positions,
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
            cover.as_ref().is_some_and(|cover| cover
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
        portrait.set_file_id(vec![9; 20]);
        message.portrait.push(portrait.clone());
        message.portrait_group = MessageField::some(ImageGroup {
            image: vec![portrait],
            ..Default::default()
        });

        let products = portrait_of(&artist(message));
        assert_eq!(
            products.map(|cover: CoverUrl| cover.as_str().to_owned()),
            Some(format!("https://i.scdn.co/image/{}", "09".repeat(20)))
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
    }
}
