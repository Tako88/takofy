//! The collection set's items, as the saved-artists and saved-albums lists draw them.
//!
//! Pure: the collection's paging items and the album metadata one batched read
//! answered with in, the ordered lists out. The request itself is not here.
//!
//! The set holds liked songs and saved albums together, so an order keeps only
//! the kind it names: an artist order keeps artist items, an album order keeps
//! album items. Removed items are dropped first, so a save undone and redone is
//! read once; among the remaining occurrences the first wire occurrence wins,
//! and items without a date come last, exactly as the followed shows' order.

use std::collections::HashMap;

use librespot::core::SpotifyUri;
use librespot::metadata::Artist;
use librespot::protocol::extended_metadata::BatchedExtensionResponse;
use librespot::protocol::metadata::Album as AlbumMessage;
use takofy_model::{CoverSet, FollowedArtist, SavedAlbum};

use crate::artist::{answered_albums, portrait_of, release_chunks, release_of, release_uris};
use crate::collection::Item;
use crate::followed::newest_first;

/// The saved artists' URIs, newest save first.
pub(crate) fn artist_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Artist { .. }))
}

/// The saved albums' URIs, newest save first.
///
/// Liked songs, episodes and everything else the set carries are dropped, so
/// only album items are kept.
pub(crate) fn album_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Album { .. }))
}

/// The liked tracks' URIs, newest save first.
///
/// The same set holds saved albums and episodes, so only track items are kept.
pub(crate) fn track_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Track { .. }))
}

/// An artist as its card draws it, before it is joined to the saved order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistCard {
    pub name: String,
    pub portrait: Option<CoverSet>,
    pub releases: u32,
}

/// The card the artist's payload answers with, when it names an artist.
///
/// `None` for a blank name: a card that cannot say who it is would draw a list
/// row the artist page cannot open. The count is the payload's own deduped
/// current releases.
pub(crate) fn artist_card(artist: &Artist) -> Option<ArtistCard> {
    if artist.name.trim().is_empty() {
        return None;
    }
    let releases = u32::try_from(release_uris(artist).len()).ok()?;
    Some(ArtistCard {
        name: artist.name.clone(),
        portrait: portrait_of(artist),
        releases,
    })
}

/// The saved artists the grid draws, in the order given.
///
/// An artist whose card is missing falls back to its URI and reads as unnamed
/// with no release count, so its card still opens the artist.
pub(crate) fn artist_entries(
    order: &[String],
    cards: &HashMap<String, ArtistCard>,
) -> Vec<FollowedArtist> {
    order
        .iter()
        .map(|uri| {
            let card = cards.get(uri);
            FollowedArtist {
                uri: uri.clone(),
                name: card.map_or_else(|| uri.clone(), |card| card.name.clone()),
                named: card.is_some(),
                portrait: card.and_then(|card| card.portrait.clone()),
                releases: card.map(|card| card.releases),
            }
        })
        .collect()
}

/// The saved albums the grid draws, in the order given.
///
/// Each chunk's answer is mapped against its own URIs, so a chunk that failed
/// or came back short costs only its albums: its URIs stay in the list, unnamed
/// and uncounted, while an answered album reads its name, its artists, its
/// year, its cover and its track count from the wire's message. A blank wire
/// name is unnamed, and the track count is the parseable track gids the wire's
/// discs carried: an album with no tracks, or whose discs failed to parse,
/// reads zero while an unanswered one reads none.
pub(crate) fn saved_album_entries(
    order: &[String],
    answers: Vec<Option<BatchedExtensionResponse>>,
) -> Vec<SavedAlbum> {
    let mut out = Vec::new();
    for (chunk, answer) in
        release_chunks(order).zip(answers.into_iter().chain(std::iter::repeat_with(|| None)))
    {
        let answered = answer.as_ref().map(answered_albums);
        for uri in chunk {
            match answered.as_ref().and_then(|albums| albums.get(uri)) {
                Some(album) => {
                    let release = release_of(uri, album);
                    out.push(SavedAlbum {
                        uri: uri.clone(),
                        name: release.name,
                        named: album.name.as_deref().is_some_and(|name| !name.is_empty()),
                        artists: join_artist_names(album),
                        year: release.year,
                        cover: release.cover,
                        tracks: u32::try_from(release.tracks.len()).ok(),
                    });
                }
                None => out.push(SavedAlbum {
                    uri: uri.clone(),
                    name: uri.clone(),
                    named: false,
                    artists: String::new(),
                    year: None,
                    cover: None,
                    tracks: None,
                }),
            }
        }
    }
    out
}

/// The album's artists, the album page's own rule: each name trimmed, blanks
/// dropped, the rest joined with ", ". An album that named no artists joins to
/// the empty string rather than an invention.
fn join_artist_names(album: &AlbumMessage) -> String {
    album
        .artist
        .iter()
        .filter_map(|artist| {
            artist
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use librespot::core::{SpotifyId, SpotifyUri};
    use librespot::metadata::Artist;
    use librespot::protocol::entity_extension_data::EntityExtensionData;
    use librespot::protocol::extended_metadata::{
        BatchedExtensionResponse, EntityExtensionDataArray,
    };
    use librespot::protocol::metadata::{
        Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
        Date as DateMessage, Disc as DiscMessage, Image, ImageGroup, Track as TrackMessage,
    };
    use protobuf::Message as _;
    use protobuf::MessageField;
    use protobuf::well_known_types::any::Any;
    use takofy_model::{CoverSet, CoverUrl};

    use super::{
        ArtistCard, album_order, artist_card, artist_entries, artist_order, saved_album_entries,
        track_order,
    };
    use crate::collection::Item;

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

    #[test]
    fn album_order_drops_liked_songs_and_episodes_and_orders_newest_first() {
        assert_eq!(
            album_order(vec![
                item(&track_uri(9), Some(900), false), // a liked song
                item(&episode_uri(8), Some(800), false),
                item(&album_uri(1), Some(100), false),
                item(&album_uri(2), Some(400), true), // a removed album
                item(&album_uri(3), None, false),
            ]),
            vec![album_uri(1), album_uri(3)],
            "liked songs and episodes are not saved albums, and a removed one stays dropped"
        );
    }

    #[test]
    fn album_order_keeps_the_first_of_a_duplicate_pair() {
        assert_eq!(
            album_order(vec![
                item(&album_uri(1), Some(100), false),
                item(&album_uri(1), Some(2_000), false),
            ]),
            vec![album_uri(1)],
            "the first occurrence's date sorts, so the later, newer duplicate does not win"
        );
    }

    #[test]
    fn track_order_keeps_only_live_track_items() {
        assert_eq!(
            track_order(vec![
                item(&track_uri(1), Some(100), false),
                item(&album_uri(2), Some(200), false),
                item(&episode_uri(3), Some(300), false),
                item(&track_uri(4), Some(400), true),
                item(&track_uri(5), None, false),
            ]),
            vec![track_uri(1), track_uri(5)],
            "an album, an episode and a removed track are not liked songs"
        );
    }

    #[test]
    fn a_track_saved_removed_and_saved_again_is_one_track() {
        assert_eq!(
            track_order(vec![
                item(&track_uri(1), Some(100), true),  // saved, then removed
                item(&track_uri(1), Some(200), false), // saved again
            ]),
            vec![track_uri(1)]
        );
    }

    #[test]
    fn track_order_keeps_the_first_duplicate() {
        assert_eq!(
            track_order(vec![
                item(&track_uri(1), Some(100), false),
                item(&track_uri(1), Some(2_000), false),
            ]),
            vec![track_uri(1)],
            "the first occurrence's date sorts, so the later, newer duplicate does not win"
        );
    }

    #[test]
    fn an_empty_item_list_gives_an_empty_order() {
        assert!(artist_order(Vec::new()).is_empty());
        assert!(album_order(Vec::new()).is_empty());
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
}
