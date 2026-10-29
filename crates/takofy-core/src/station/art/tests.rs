use takofy_model::home::StationArt;
use takofy_model::{COMPOSED_MAX, CoverSet, CoverUrl};

use super::*;

/// The two station answers the live probe recorded with a picture: a playlist
/// seed names a mosaic, an album seed a single cover.
const ALBUM: &str = include_str!("../fixtures/radio_stations_album.json");
const PLAYLIST: &str = include_str!("../fixtures/radio_stations_playlist.json");

/// One real 20-byte image id, as the wire writes it in hex.
const ID: &str = "ab67616d0000b273810dd84a63f327f269b14b42";

/// The `imageUri` a dump names.
fn image_uri(dump: &str) -> &str {
    const KEY: &str = "\"imageUri\":\"";
    let start = dump.find(KEY).expect("the dump names a picture") + KEY.len();
    let end = start + dump[start..].find('"').expect("the picture ends");
    &dump[start..end]
}

/// The single-cover set `id` names.
fn cover(id: &str) -> CoverSet {
    let bytes = decode(id).expect("the test's own id is hex");
    CoverSet::single(CoverUrl::from_file_id(&bytes).expect("a 20-byte id is a picture"))
}

#[test]
fn the_playlist_dump_names_a_four_part_mosaic() {
    assert_eq!(
        art_of(image_uri(PLAYLIST)),
        Some(StationArt::Mosaic(
            [
                "ab67616d00001e0201ac8e33bfce36799cb1edea",
                "ab67616d00001e023439491bc71aa89888adf038",
                "ab67616d00001e024d0b7388e14e7eeeff1fd5f1",
                "ab67616d00001e0260a9f8ddb4d41b6dc6db06fd",
            ]
            .map(cover)
            .to_vec()
        ))
    );
}

#[test]
fn the_album_dump_names_a_single_picture() {
    let uri = image_uri(ALBUM);
    let id = uri
        .strip_prefix(IMAGE)
        .expect("the album dump names one image");
    assert_eq!(art_of(uri), Some(StationArt::Single(cover(id))));
}

#[test]
fn one_valid_mosaic_part_is_a_single_picture() {
    assert_eq!(
        art_of(&format!("{MOSAIC}{ID}")),
        Some(StationArt::Single(cover(ID)))
    );
}

#[test]
fn two_mosaic_parts_are_a_mosaic() {
    let other = "ab67616d00001e023439491bc71aa89888adf038";
    assert_eq!(
        art_of(&format!("{MOSAIC}{ID}:{other}")),
        Some(StationArt::Mosaic(vec![cover(ID), cover(other)]))
    );
}

#[test]
fn five_mosaic_parts_keep_the_first_four() {
    let parts: Vec<String> = (1..=5)
        .map(|n| format!("ab67616d00001e02{n:024x}"))
        .collect();
    let art = art_of(&format!("{MOSAIC}{}", parts.join(":")));
    let expected: Vec<CoverSet> = parts
        .iter()
        .take(COMPOSED_MAX)
        .map(|id| cover(id))
        .collect();
    assert_eq!(expected.len(), COMPOSED_MAX);
    assert_eq!(art, Some(StationArt::Mosaic(expected)));
}

#[test]
fn a_malformed_part_is_left_out_of_a_mosaic() {
    let other = "ab67616d00001e023439491bc71aa89888adf038";
    assert_eq!(
        art_of(&format!("{MOSAIC}{ID}:nothex:{other}:")),
        Some(StationArt::Mosaic(vec![cover(ID), cover(other)]))
    );
}

#[test]
fn no_valid_part_is_no_picture() {
    for uri in [
        MOSAIC.to_owned(),
        format!("{MOSAIC}nothex"),
        format!("{MOSAIC}::::"),
        IMAGE.to_owned(),
        format!("{IMAGE}nothex"),
    ] {
        assert_eq!(art_of(&uri), None, "{uri}");
    }
}

#[test]
fn a_wrong_prefix_is_no_picture() {
    for uri in [
        &format!("https://i.scdn.co/image/{ID}")[..],
        &format!("spotify:imagex:{ID}")[..],
        &format!("spotify:mosaicx:{ID}")[..],
        ID,
    ] {
        assert_eq!(art_of(uri), None, "{uri}");
    }
}

#[test]
fn a_wrong_id_is_no_picture() {
    for id in [
        // An odd number of digits.
        "ab67616d0000b273810dd84a63f327f269b14b4",
        // Nineteen bytes, not twenty.
        "ab67616d0000b273810dd84a63f327f269b14b",
        // Not hex.
        "zb67616d0000b273810dd84a63f327f269b14b42",
        // Twenty zero bytes are a padded id, not an image.
        "0000000000000000000000000000000000000000",
    ] {
        assert_eq!(art_of(&format!("{IMAGE}{id}")), None, "{id}");
    }
}

#[test]
fn a_rows_own_cover_is_the_single_picture_the_wire_named() {
    assert_eq!(cover_of(&format!("{IMAGE}{ID}")), Some(cover(ID)));
    assert_eq!(cover_of(&format!("{MOSAIC}{ID}")), None);
    assert_eq!(cover_of(&format!("{IMAGE}nothex")), None);
}
