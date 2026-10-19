//! An album as its page draws it.
//!
//! Pure: a parsed [`Album`] and the labels for its track URIs in, an
//! [`AlbumDetail`] out. The request itself is in `remote::read_album`.
//!
//! An album's own order is not a playlist's: `Album::get` answers with discs,
//! each carrying its number and its tracks, so the positions are flattened by
//! disc number and numbered by their place in the result. A multi-disc album is
//! one continuous list, which is what the reference draws.

use librespot::core::SpotifyUri;
use librespot::metadata::Album;
use takofy_model::{AlbumDetail, TrackEntry};

use crate::context::PageTrack;
use crate::cover::pick_images_cover;

/// Why an album read failed.
#[derive(Debug, thiserror::Error)]
pub enum AlbumError {
    /// The URI does not name an album, so there is nothing to ask for.
    #[error("not an album URI")]
    NotAnAlbum,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// The album's playable positions, in its own order.
///
/// The discs are ordered by their own number, so a wire that lists disc two first
/// still reads one first; `sort_by_key` is stable, so discs sharing a number keep
/// the wire's order. Only `SpotifyUri::Track` positions are playable, and one
/// whose URI cannot be written is left out rather than guessed at.
///
/// No position carries an added date: an album is a record, not a membership.
pub(crate) fn positions(album: &Album) -> Vec<PageTrack> {
    let mut discs: Vec<_> = album.discs.iter().collect();
    discs.sort_by_key(|disc| disc.number);
    discs
        .into_iter()
        .flat_map(|disc| disc.tracks.iter())
        .filter(|uri| matches!(uri, SpotifyUri::Track { .. }))
        .filter_map(|uri| uri.to_uri().ok())
        .map(|uri| PageTrack {
            uri,
            added_at: None,
        })
        .collect()
}

/// The album's page data, from the album and its labelled tracks.
///
/// The name falls back to the URI exactly as a playlist's does, and a blank
/// artist is skipped rather than drawn as an empty name in the middle of a list.
pub(crate) fn detail(uri: &str, album: &Album, tracks: Vec<TrackEntry>) -> AlbumDetail {
    let name = if album.name.is_empty() {
        uri.to_owned()
    } else {
        album.name.clone()
    };
    let artists = album
        .artists
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    AlbumDetail {
        uri: uri.to_owned(),
        name,
        artists,
        artist_uris: artist_uris(album),
        year: year_of(album),
        cover: pick_images_cover(&album.cover_group),
        tracks,
    }
}

/// The album's artist URIs, in the same order as `detail`'s joined names.
///
/// The list stops at the first artist that contributes no name, so the subline's
/// link can never point at a different artist than the one named beside it. That
/// is [`takofy_model::TrackEntry::artist_uris`]' rule, applied to the album's own
/// credits.
fn artist_uris(album: &Album) -> Vec<String> {
    album
        .artists
        .iter()
        .map_while(|artist| {
            if artist.name.trim().is_empty() {
                return None;
            }
            artist.id.to_uri().ok()
        })
        .collect()
}

/// The album's release year, or `None` when it has none worth drawing.
///
/// A default date message reads as year zero, which is the wire saying nothing
/// rather than a record from year 1 BC. A negative year is representable in
/// `time::Date` and is refused here for the same reason. The year is a `u16` in
/// the model, so the calendar's own ceiling — `time::Date` rejects anything past
/// 9999 — is the only upper bound that can reach this.
fn year_of(album: &Album) -> Option<u16> {
    u16::try_from(album.date.year())
        .ok()
        .filter(|year| *year != 0)
}

#[cfg(test)]
mod tests {
    use librespot::core::{SpotifyId, SpotifyUri};
    use librespot::metadata::Album;
    use librespot::protocol::metadata::Album as AlbumMessage;
    use librespot::protocol::metadata::Artist as ArtistMessage;
    use librespot::protocol::metadata::Date as DateMessage;
    use librespot::protocol::metadata::Disc as DiscMessage;
    use librespot::protocol::metadata::Track as TrackMessage;
    use librespot::protocol::metadata::{Image, ImageGroup};
    use takofy_model::TrackEntry;

    use super::{detail, positions};

    /// A 16-byte gid. `SpotifyId::from_raw` rejects anything that is not exactly
    /// sixteen bytes, so every gid a fixture builds has to be one.
    fn gid(byte: u8) -> Vec<u8> {
        vec![byte; 16]
    }

    /// One disc-track message with the gid tree the live wire carries.
    ///
    /// `Disc::tracks` converts each track through its own `gid` alone, but the
    /// wire's message also nests its album and its artists, so the fixture is the
    /// shape the request answers with rather than the minimum that parses.
    fn wire_track(byte: u8, title: &str) -> TrackMessage {
        let mut artist = ArtistMessage::default();
        artist.set_gid(gid(byte.wrapping_add(64)));
        artist.set_name(format!("Artist {byte}"));
        let mut album = AlbumMessage::default();
        album.set_gid(gid(byte.wrapping_add(128)));
        album.set_name("Record".to_owned());
        album.artist.push(artist.clone());
        TrackMessage {
            gid: Some(gid(byte)),
            name: Some(title.to_owned()),
            album: Some(album).into(),
            artist: vec![artist],
            ..Default::default()
        }
    }

    /// A protocol album: `discs` are `(number, [(gid byte, title)])`.
    fn wire_album(
        name: &str,
        artists: &[(&str, u8)],
        discs: &[(i32, &[(u8, &str)])],
        year: i32,
    ) -> AlbumMessage {
        let mut album = AlbumMessage::default();
        album.set_gid(gid(1));
        album.set_name(name.to_owned());
        for (artist_name, byte) in artists {
            let mut artist = ArtistMessage::default();
            artist.set_gid(gid(*byte));
            artist.set_name((*artist_name).to_owned());
            album.artist.push(artist);
        }
        for (number, tracks) in discs {
            let mut disc = DiscMessage::default();
            disc.set_number(*number);
            for (byte, title) in tracks.iter() {
                disc.track.push(wire_track(*byte, title));
            }
            album.disc.push(disc);
        }
        let mut date = DateMessage::default();
        date.set_year(year);
        album.date = Some(date).into();
        album
    }

    /// The parsed album the wire's message converts to.
    fn album(name: &str, artists: &[(&str, u8)], discs: &[(i32, &[(u8, &str)])]) -> Album {
        Album::try_from(&wire_album(name, artists, discs, 1997)).expect("a valid album message")
    }

    /// Two discs, each with two tracks.
    fn two_discs() -> Album {
        album(
            "Record",
            &[("Band", 2)],
            &[
                (1, &[(10, "One"), (11, "Two")]),
                (2, &[(12, "Three"), (13, "Four")]),
            ],
        )
    }

    /// The URI the wire's gid byte resolves to.
    fn uri(byte: u8) -> String {
        SpotifyUri::Track {
            id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
        }
        .to_uri()
        .expect("a uri")
    }

    fn uris(album: &Album) -> Vec<String> {
        positions(album).into_iter().map(|item| item.uri).collect()
    }

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn labelled(uris: &[String]) -> Vec<TrackEntry> {
        uris.iter().map(|uri| track(uri)).collect()
    }

    #[test]
    fn two_discs_flatten_in_album_order() {
        let album = two_discs();
        assert_eq!(
            uris(&album),
            vec![uri(10), uri(11), uri(12), uri(13)],
            "the album's own order is disc one then disc two"
        );
    }

    #[test]
    fn discs_listed_out_of_order_read_in_number_order() {
        let album = album(
            "Record",
            &[("Band", 2)],
            &[(2, &[(12, "Three")]), (1, &[(10, "One")])],
        );
        assert_eq!(
            uris(&album),
            vec![uri(10), uri(12)],
            "disc 1 must come first however the wire listed them"
        );
    }

    #[test]
    fn equal_disc_numbers_keep_the_wire_order() {
        let album = album(
            "Record",
            &[("Band", 2)],
            &[(1, &[(10, "One")]), (1, &[(11, "Two")])],
        );
        assert_eq!(
            uris(&album),
            vec![uri(10), uri(11)],
            "the sort is stable, so equal numbers stay as they arrived"
        );
    }

    #[test]
    fn a_non_track_position_is_skipped() {
        let mut album = two_discs();
        album.discs.0[0].tracks.0.push(SpotifyUri::Episode {
            id: SpotifyId::from_raw(&gid(99)).expect("a 16-byte id"),
        });
        assert_eq!(
            uris(&album),
            vec![uri(10), uri(11), uri(12), uri(13)],
            "only tracks are playable positions"
        );
    }

    /// The conversion is per album, not per disc: one unusable gid fails the
    /// whole read, which the page shows as a failure with a retry.
    #[test]
    fn a_malformed_disc_gid_fails_the_whole_album() {
        let mut message = wire_album(
            "Record",
            &[("Band", 2)],
            &[(1, &[(10, "One")]), (2, &[(11, "Two")])],
            1997,
        );
        message.disc[1].track[0].gid = Some(vec![7; 15]);
        assert!(
            Album::try_from(&message).is_err(),
            "a 15-byte gid is not a Spotify id, so nothing parses"
        );
    }

    #[test]
    fn an_album_position_carries_no_added_date() {
        let album = two_discs();
        let positions = positions(&album);
        assert_eq!(positions.len(), 4);
        assert!(
            positions.iter().all(|item| item.added_at.is_none()),
            "an album is a record, not a membership"
        );

        let tracks = labelled(&uris(&album));
        let detail = detail("spotify:album:x", &album, tracks.clone());
        assert_eq!(detail.tracks, tracks);
        assert!(detail.tracks.iter().all(|track| track.added_at.is_none()));
    }

    #[test]
    fn the_year_is_read_from_the_wire_date() {
        let album = Album::try_from(&wire_album("Record", &[("Band", 2)], &[], 1997))
            .expect("a valid album message");
        assert_eq!(
            detail("spotify:album:x", &album, Vec::new()).year,
            Some(1997)
        );
    }

    #[test]
    fn a_zero_year_is_no_year() {
        let album = Album::try_from(&wire_album("Record", &[("Band", 2)], &[], 0))
            .expect("a default date is year zero");
        assert_eq!(detail("spotify:album:x", &album, Vec::new()).year, None);
    }

    #[test]
    fn a_negative_wire_year_is_no_year() {
        let album = Album::try_from(&wire_album("Record", &[("Band", 2)], &[], -1))
            .expect("time's calendar holds a negative year");
        assert_eq!(detail("spotify:album:x", &album, Vec::new()).year, None);
    }

    /// The year is a `u16` in the model, so the ceiling that matters is the
    /// calendar's own: `time::Date` refuses a year past 9999 before any mapping
    /// here could see it.
    #[test]
    fn a_year_past_the_calendar_cannot_be_read() {
        assert!(
            Album::try_from(&wire_album("Record", &[("Band", 2)], &[], 10_000)).is_err(),
            "the wire date's year must fit the calendar"
        );
    }

    #[test]
    fn an_empty_name_falls_back_to_the_uri() {
        let album = Album::try_from(&wire_album("", &[("Band", 2)], &[], 1997))
            .expect("a valid album message");
        let detail = detail("spotify:album:x", &album, Vec::new());
        assert_eq!(detail.name, "spotify:album:x");
        assert_eq!(detail.uri, "spotify:album:x");
    }

    #[test]
    fn artists_join_and_a_blank_is_skipped() {
        let album = Album::try_from(&wire_album(
            "Record",
            &[("Band", 2), ("  ", 3), ("Guest", 4)],
            &[],
            1997,
        ))
        .expect("a valid album message");
        assert_eq!(
            detail("spotify:album:x", &album, Vec::new()).artists,
            "Band, Guest"
        );
    }

    #[test]
    fn no_artists_reads_as_no_names() {
        let album =
            Album::try_from(&wire_album("Record", &[], &[], 1997)).expect("a valid album message");
        assert_eq!(detail("spotify:album:x", &album, Vec::new()).artists, "");
        assert_eq!(
            detail("spotify:album:x", &album, Vec::new()).artist_uris,
            Vec::<String>::new(),
            "no artist means no link either"
        );
    }

    #[test]
    fn artist_uris_follow_the_joined_names() {
        let album = Album::try_from(&wire_album(
            "Record",
            &[("Band", 2), ("Guest", 3)],
            &[],
            1997,
        ))
        .expect("a valid album message");
        let detail = detail("spotify:album:x", &album, Vec::new());
        assert_eq!(detail.artists, "Band, Guest");
        assert_eq!(
            detail.artist_uris,
            vec![
                SpotifyUri::Artist {
                    id: SpotifyId::from_raw(&gid(2)).expect("a 16-byte id"),
                }
                .to_uri()
                .expect("a uri"),
                SpotifyUri::Artist {
                    id: SpotifyId::from_raw(&gid(3)).expect("a 16-byte id"),
                }
                .to_uri()
                .expect("a uri"),
            ]
        );
    }

    #[test]
    fn a_blank_artist_ends_the_artist_uris() {
        let album = Album::try_from(&wire_album(
            "Record",
            &[("Band", 2), ("  ", 3), ("Guest", 4)],
            &[],
            1997,
        ))
        .expect("a valid album message");
        let detail = detail("spotify:album:x", &album, Vec::new());
        assert_eq!(detail.artists, "Band, Guest", "the names skip the blank");
        assert_eq!(
            detail.artist_uris.len(),
            1,
            "the uri list stops where the name list cannot name one"
        );
    }

    #[test]
    fn a_cover_comes_from_the_cover_group() {
        let mut message = wire_album("Record", &[("Band", 2)], &[], 1997);
        let mut image = Image::default();
        image.set_file_id((1..=20).collect::<Vec<u8>>());
        image.set_width(300);
        message.cover_group = Some(ImageGroup {
            image: vec![image],
            ..Default::default()
        })
        .into();
        let album = Album::try_from(&message).expect("a valid album message");
        let cover = detail("spotify:album:x", &album, Vec::new()).cover;
        assert!(
            cover.as_ref().is_some_and(|cover| cover
                .as_str()
                .ends_with("0102030405060708090a0b0c0d0e0f1011121314")),
            "{cover:?}"
        );
    }

    #[test]
    fn no_cover_group_is_no_cover() {
        let album = Album::try_from(&wire_album("Record", &[("Band", 2)], &[], 1997))
            .expect("a valid album message");
        assert_eq!(detail("spotify:album:x", &album, Vec::new()).cover, None);
    }
}
