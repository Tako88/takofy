use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{Artist as ArtistMessage, Image, ImageGroup};
use protobuf::Message as _;
use takofy_model::{CoverSet, CoverUrl};

use super::{Portrait, portrait_from, portrait_of, portraits_of};

fn image(size: Size, width: i32, id: &[u8]) -> Image {
    let mut image = Image::new();
    image.set_size(size);
    image.set_width(width);
    image.set_height(width);
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

fn set(id: u8, size: u32) -> CoverSet {
    CoverSet::from_sizes([(
        Some((size, size)),
        CoverUrl::from_wire(&url(id)).expect("a cover url"),
    )])
    .expect("a cover set")
}

#[test]
fn a_record_keeps_every_portrait_size() {
    let message = with_group(vec![
        image(Size::LARGE, 640, &[1; 20]),
        image(Size::DEFAULT, 300, &[9; 20]),
    ]);
    assert_eq!(
        portrait_of(&message),
        CoverSet::from_sizes([
            (
                Some((640, 640)),
                CoverUrl::from_wire(&url(1)).expect("a cover url")
            ),
            (
                Some((300, 300)),
                CoverUrl::from_wire(&url(9)).expect("a cover url")
            ),
        ])
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
    assert_eq!(portrait_of(&usable_after), Some(set(9, 640)));
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
            portrait: Some(set(9, 300)),
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
    assert_eq!(portrait.portrait, Some(set(9, 300)));
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

fn found(entries: &[(&str, Option<CoverSet>)]) -> HashMap<String, Portrait> {
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
        ("spotify:artist:a", Some(set(1, 300))),
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
    assert_eq!(portraits[1].portrait, Some(set(1, 300)));
}

#[test]
fn portraits_of_omits_a_uri_no_answer_names() {
    let found = found(&[("spotify:artist:a", None)]);
    assert!(portraits_of(&artist_uris(&["spotify:artist:z"]), &found).is_empty());
    assert!(portraits_of(&[], &found).is_empty());
}
