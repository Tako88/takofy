//! Reading an artist's portrait from an `ARTIST_V4` answer.
//!
//! Pure: the wire's bytes in, a typed answer out, so the parse and the ordering
//! can be exercised without a session. The request itself is in `remote.rs`,
//! the one file that performs librespot I/O.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::metadata::Artist as ArtistMessage;
use protobuf::Message as _;
use takofy_model::{ArtistPortrait, CoverUrl};

use crate::cover::pick_cover;

/// One artist's answer, keyed by the artist URI the record itself names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    /// The artist URI, built from the record's own gid so the answer does not
    /// depend on how the server echoed the request. Falls back to the echoed
    /// URI, and is `None` when neither names one.
    pub artist: Option<String>,
    /// `None` when the record carries no portrait group.
    pub portrait: Option<CoverUrl>,
}

/// The portrait group's image, or `None` when the record carries none.
pub fn portrait_of(message: &ArtistMessage) -> Option<CoverUrl> {
    pick_cover(message.portrait_group.as_ref())
}

/// The portrait an encoded artist answer carries, or `None` when the bytes are
/// not an artist message.
pub fn portrait_from(bytes: &[u8], echoed: &str) -> Option<Portrait> {
    let message = ArtistMessage::parse_from_bytes(bytes).ok()?;
    let portrait = portrait_of(&message);
    // A record that names itself by nothing and carries no portrait is not an
    // answer; caching it would show the fallback for a record never received.
    if message.gid.is_none() && portrait.is_none() {
        return None;
    }
    let artist = artist_uri(&message, echoed);
    Some(Portrait { artist, portrait })
}

/// The artist's own URI: its gid names it, so the echo is only a fallback.
fn artist_uri(message: &ArtistMessage, echoed: &str) -> Option<String> {
    message
        .gid
        .as_deref()
        .and_then(|gid| SpotifyId::from_raw(gid).ok())
        .map(|id| SpotifyUri::Artist { id })
        .and_then(|uri| uri.to_uri().ok())
        .or_else(|| Some(echoed.to_owned()).filter(|uri| !uri.is_empty()))
}

/// One portrait per requested artist, in the requested order.
///
/// A requested URI that no answer names is omitted, and every request is
/// answered once however often it was asked for.
pub fn portraits_of(
    requested: &[String],
    found: &HashMap<String, Portrait>,
) -> Vec<ArtistPortrait> {
    let answered: HashMap<&str, &Portrait> = found
        .values()
        .filter_map(|portrait| Some((portrait.artist.as_deref()?, portrait)))
        .collect();
    let mut seen = HashSet::new();
    requested
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .filter_map(|uri| {
            let portrait = answered.get(uri.as_str())?;
            Some(ArtistPortrait {
                artist: uri.clone(),
                portrait: portrait.portrait.clone(),
            })
        })
        .collect()
}

/// The future's answer, or `None` when `limit` passes first.
///
/// librespot's HTTP client has no request timeout, so this is the only bound a
/// speculative read has.
pub async fn bounded<T>(future: impl Future<Output = T>, limit: Duration) -> Option<T> {
    tokio::time::timeout(limit, future).await.ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use librespot::core::{SpotifyId, SpotifyUri};
    use librespot::protocol::metadata::image::Size;
    use librespot::protocol::metadata::{Artist as ArtistMessage, Image, ImageGroup};
    use protobuf::Message as _;
    use takofy_model::CoverUrl;

    use super::{Portrait, bounded, portrait_from, portrait_of, portraits_of};

    fn image(size: Size, width: i32, id: &[u8]) -> Image {
        let mut image = Image::new();
        image.set_size(size);
        image.set_width(width);
        image.set_file_id(id.to_vec());
        image
    }

    fn with_group(images: Vec<Image>) -> ArtistMessage {
        let mut message = ArtistMessage::new();
        message.portrait_group = protobuf::MessageField::some(ImageGroup {
            image: images,
            ..Default::default()
        });
        message
    }

    fn url(id: u8) -> String {
        format!("https://i.scdn.co/image/{}", format!("{id:02x}").repeat(20))
    }

    #[test]
    fn the_default_size_tile_wins() {
        let message = with_group(vec![
            image(Size::LARGE, 640, &[1; 20]),
            image(Size::DEFAULT, 300, &[9; 20]),
        ]);
        assert_eq!(
            portrait_of(&message).map(|cover| cover.as_str().to_owned()),
            Some(url(9))
        );
    }

    #[test]
    fn a_record_without_a_portrait_group_has_no_portrait() {
        assert_eq!(portrait_of(&ArtistMessage::new()), None);
    }

    #[test]
    fn a_short_or_zero_file_id_is_skipped() {
        let short = with_group(vec![image(Size::DEFAULT, 300, &[1; 3])]);
        assert_eq!(portrait_of(&short), None);
        let zero = with_group(vec![image(Size::DEFAULT, 300, &[0; 20])]);
        assert_eq!(portrait_of(&zero), None);
        let usable_after = with_group(vec![
            image(Size::DEFAULT, 300, &[0; 20]),
            image(Size::LARGE, 640, &[9; 20]),
        ]);
        assert_eq!(
            portrait_of(&usable_after).map(|cover| cover.as_str().to_owned()),
            Some(url(9))
        );
    }

    #[test]
    fn portrait_from_reads_the_records_own_gid_and_its_portrait() {
        let gid: Vec<u8> = (1..=16).collect();
        let mut message = with_group(vec![image(Size::DEFAULT, 300, &[9; 20])]);
        message.set_gid(gid.clone());
        let bytes = message.write_to_bytes().expect("a serialised artist");
        let portrait = portrait_from(&bytes, "spotify:artist:echoed").expect("a portrait");
        let artist_uri = SpotifyUri::Artist {
            id: SpotifyId::from_raw(&gid).expect("a valid id"),
        }
        .to_uri()
        .expect("an artist uri");
        assert_eq!(
            portrait,
            Portrait {
                artist: Some(artist_uri),
                portrait: CoverUrl::from_wire(&url(9)),
            }
        );
    }

    #[test]
    fn portrait_from_falls_back_to_the_echoed_uri_when_the_record_has_no_gid() {
        let bytes = with_group(vec![image(Size::DEFAULT, 300, &[9; 20])])
            .write_to_bytes()
            .expect("a serialised artist");
        let portrait = portrait_from(&bytes, "spotify:artist:echoed").expect("a portrait");
        assert_eq!(portrait.artist.as_deref(), Some("spotify:artist:echoed"));
        assert_eq!(portrait.portrait, CoverUrl::from_wire(&url(9)));
    }

    #[test]
    fn an_empty_record_is_not_an_answer() {
        let bytes = ArtistMessage::new()
            .write_to_bytes()
            .expect("a serialised artist");
        assert_eq!(portrait_from(&bytes, "spotify:artist:echoed"), None);
    }

    #[test]
    fn a_record_with_a_gid_and_no_portrait_is_an_absence() {
        let gid: Vec<u8> = (1..=16).collect();
        let mut message = ArtistMessage::new();
        message.set_gid(gid);
        let bytes = message.write_to_bytes().expect("a serialised artist");
        let portrait = portrait_from(&bytes, "").expect("an absence, not nothing");
        assert!(portrait.artist.is_some());
        assert_eq!(portrait.portrait, None);
    }

    #[test]
    fn garbage_bytes_are_not_a_portrait() {
        assert_eq!(portrait_from(&[0xff, 0x00, 0x13], "spotify:artist:a"), None);
    }

    fn found(entries: &[(&str, Option<CoverUrl>)]) -> HashMap<String, Portrait> {
        entries
            .iter()
            .map(|(artist, portrait)| {
                (
                    format!("echoed:{artist}"),
                    Portrait {
                        artist: Some((*artist).to_owned()),
                        portrait: portrait.clone(),
                    },
                )
            })
            .collect()
    }

    fn artist_uris(uris: &[&str]) -> Vec<String> {
        uris.iter().map(|uri| (*uri).to_owned()).collect()
    }

    #[test]
    fn portraits_of_keeps_the_requested_order_and_dedups() {
        let found = found(&[
            ("spotify:artist:a", CoverUrl::from_wire(&url(1))),
            ("spotify:artist:b", None),
        ]);
        let requested = artist_uris(&["spotify:artist:b", "spotify:artist:a", "spotify:artist:b"]);
        let portraits = portraits_of(&requested, &found);
        let order: Vec<&str> = portraits
            .iter()
            .map(|portrait| portrait.artist.as_str())
            .collect();
        assert_eq!(order, vec!["spotify:artist:b", "spotify:artist:a"]);
        assert_eq!(portraits[0].portrait, None);
        assert_eq!(portraits[1].portrait, CoverUrl::from_wire(&url(1)));
    }

    #[test]
    fn portraits_of_omits_a_uri_no_answer_names() {
        let found = found(&[("spotify:artist:a", None)]);
        assert!(portraits_of(&artist_uris(&["spotify:artist:z"]), &found).is_empty());
        assert!(portraits_of(&[], &found).is_empty());
    }

    #[tokio::test]
    async fn bounded_gives_none_for_a_future_that_never_completes() {
        let pending = std::future::pending::<u8>();
        assert_eq!(bounded(pending, Duration::from_millis(1)).await, None);
    }

    #[tokio::test]
    async fn bounded_gives_some_for_a_future_that_does_complete() {
        assert_eq!(
            bounded(std::future::ready(7_u8), Duration::from_secs(30)).await,
            Some(7)
        );
    }
}
