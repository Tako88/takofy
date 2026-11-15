use chrono::{Datelike, NaiveDate};
use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Artist;
use librespot::protocol::metadata::album::Type as AlbumType;
use librespot::protocol::metadata::{
    Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
    Date as DateMessage, Image, ImageGroup,
};
use protobuf::Message as _;
use protobuf::MessageField;
use takofy_model::ReleaseKind;
use takofy_model::home::{Day, ReleaseDate};

use super::{
    civil_day, date_of, dated_album_from, dated_album_of, new_release_of, new_release_uris,
};

/// A 16-byte gid. `SpotifyId::from_raw` rejects anything else.
fn gid(byte: u8) -> Vec<u8> {
    vec![byte; 16]
}

/// The URI an album gid byte resolves to.
fn album_uri(byte: u8) -> String {
    SpotifyUri::Album {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

/// One wire date, with every part the test names.
fn wire_date(year: i32, month: i32, day: i32) -> DateMessage {
    let mut date = DateMessage::default();
    date.set_year(year);
    date.set_month(month);
    date.set_day(day);
    date
}

/// One album answer carrying a name, a kind and a date.
fn wire_album(name: &str, kind: AlbumType, date: DateMessage) -> AlbumMessage {
    let mut album = AlbumMessage::default();
    album.set_gid(gid(50));
    album.set_name(name.to_owned());
    album.set_type(kind);
    album.date = MessageField::some(date);
    album
}

/// One dated entry of a released discography.
fn dated(year: u16, month: Option<u8>, day: Option<u8>) -> ReleaseDate {
    ReleaseDate { year, month, day }
}

/// The day number of a calendar date, as chrono counts them.
fn calendar(year: i32, month: u32, day: u32) -> i32 {
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("a real date")
        .num_days_from_ce()
}

#[test]
fn civil_day_agrees_with_the_calendar_over_fifty_years() {
    let mut date = NaiveDate::from_ymd_opt(1990, 1, 1).expect("a real date");
    let end = NaiveDate::from_ymd_opt(2040, 12, 31).expect("a real date");
    while date <= end {
        let parts = dated(
            u16::try_from(date.year()).expect("a year in range"),
            Some(u8::try_from(date.month()).expect("a month")),
            Some(u8::try_from(date.day()).expect("a day")),
        );
        assert_eq!(
            civil_day(&parts).map(Day::days),
            Some(date.num_days_from_ce()),
            "{date} did not agree with the calendar"
        );
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }
}

#[test]
fn civil_day_refuses_a_date_the_calendar_does_not_have() {
    for parts in [
        dated(2025, Some(0), Some(14)),
        dated(2025, Some(13), Some(14)),
        dated(2025, Some(10), Some(0)),
        dated(2025, Some(10), Some(32)),
        dated(2025, Some(2), Some(31)),
        dated(2024, Some(2), Some(30)),
        dated(2025, None, Some(14)),
        dated(2025, Some(10), None),
    ] {
        assert_eq!(
            civil_day(&parts),
            None,
            "{parts:?} was placed on the calendar"
        );
    }
    assert_eq!(
        civil_day(&dated(2024, Some(2), Some(29))).map(Day::days),
        Some(calendar(2024, 2, 29)),
        "a leap day is a day"
    );
}

#[test]
fn a_year_only_date_keeps_its_year_and_nothing_else() {
    assert_eq!(
        date_of(Some(&wire_date(1996, 0, 0))),
        Some(dated(1996, None, None)),
        "a wire zero is the field saying nothing rather than January"
    );
}

#[test]
fn a_year_and_month_date_keeps_the_month() {
    assert_eq!(
        date_of(Some(&wire_date(2025, 11, 0))),
        Some(dated(2025, Some(11), None))
    );
}

#[test]
fn a_full_date_keeps_every_part() {
    assert_eq!(
        date_of(Some(&wire_date(2025, 10, 14))),
        Some(dated(2025, Some(10), Some(14)))
    );
}

#[test]
fn a_date_with_no_year_is_no_date() {
    assert_eq!(date_of(Some(&wire_date(0, 10, 14))), None);
    assert_eq!(date_of(Some(&wire_date(-1, 10, 14))), None);
    assert_eq!(date_of(None), None, "no date message is no date");
}

#[test]
fn a_day_with_no_month_is_dropped_with_it() {
    assert_eq!(
        date_of(Some(&wire_date(2025, 13, 14))),
        Some(dated(2025, None, None)),
        "a month outside the calendar cannot place the day it came with"
    );
    assert_eq!(
        date_of(Some(&wire_date(2025, 10, 32))),
        Some(dated(2025, Some(10), None))
    );
}

#[test]
fn dated_album_of_reads_the_wires_own_facts() {
    let album = wire_album("Slow Tide", AlbumType::ALBUM, wire_date(2025, 10, 14));
    let release = dated_album_of(&album_uri(50), &album);

    assert_eq!(release.uri, album_uri(50));
    assert_eq!(release.name, "Slow Tide");
    assert_eq!(release.kind, ReleaseKind::Album);
    assert_eq!(release.date, Some(dated(2025, Some(10), Some(14))));
    assert_eq!(release.cover, None, "the wire sent no cover group");
}

#[test]
fn dated_album_of_reads_each_kind_the_wire_names() {
    for (wire, kind) in [
        (AlbumType::ALBUM, ReleaseKind::Album),
        (AlbumType::EP, ReleaseKind::Ep),
        (AlbumType::SINGLE, ReleaseKind::Single),
        (AlbumType::COMPILATION, ReleaseKind::Compilation),
        (AlbumType::AUDIOBOOK, ReleaseKind::Other),
        (AlbumType::PODCAST, ReleaseKind::Other),
    ] {
        let album = wire_album("Slow Tide", wire, wire_date(2025, 10, 14));
        assert_eq!(
            dated_album_of("spotify:album:x", &album).kind,
            kind,
            "{wire:?} was read as the wrong kind"
        );
    }
}

#[test]
fn an_empty_release_name_falls_back_to_the_uri() {
    let album = wire_album("", AlbumType::SINGLE, wire_date(2025, 0, 0));
    assert_eq!(
        dated_album_of("spotify:album:x", &album).name,
        "spotify:album:x"
    );
}

#[test]
fn a_cover_group_round_trips() {
    let mut image = Image::default();
    image.set_file_id(vec![7u8; 20]);
    image.set_width(300);
    image.set_height(300);
    let mut group = ImageGroup::default();
    group.image.push(image);

    let mut album = wire_album("Slow Tide", AlbumType::ALBUM, wire_date(2025, 10, 14));
    album.cover_group = MessageField::some(group);

    assert!(
        dated_album_of("spotify:album:x", &album).cover.is_some(),
        "the release's own picture was dropped"
    );
}

#[test]
fn dated_album_from_parses_the_bytes_the_answer_carries() {
    let album = wire_album("Slow Tide", AlbumType::SINGLE, wire_date(2025, 10, 14));
    let bytes = album.write_to_bytes().expect("an encodable album");

    assert_eq!(
        dated_album_from(&bytes, "spotify:album:x"),
        Some(dated_album_of("spotify:album:x", &album))
    );
}

#[test]
fn dated_album_from_refuses_bytes_that_are_not_an_album() {
    assert_eq!(dated_album_from(&[0xff, 0x00], "spotify:album:x"), None);
}

fn artist(message: ArtistMessage) -> Artist {
    Artist::try_from(&message).expect("a valid artist message")
}

/// One group of variants, the first of which is the current release.
fn group(bytes: &[u8]) -> AlbumGroupMessage {
    AlbumGroupMessage {
        album: bytes
            .iter()
            .map(|byte| {
                let mut album = AlbumMessage::default();
                album.set_gid(gid(*byte));
                album
            })
            .collect(),
        ..Default::default()
    }
}

fn empty_artist_message() -> ArtistMessage {
    let mut message = ArtistMessage::default();
    message.set_gid(gid(1));
    message.set_name("Vela".to_owned());
    message
}

#[test]
fn new_release_uris_are_the_artists_albums_and_singles() {
    let mut message = empty_artist_message();
    message.album_group.push(group(&[10]));
    message.single_group.push(group(&[20]));
    message.compilation_group.push(group(&[30]));
    message.appears_on_group.push(group(&[40]));

    assert_eq!(
        new_release_uris(&artist(message)),
        vec![album_uri(10), album_uri(20)],
        "a compilation is not new and an appearance is not the artist's"
    );
}

#[test]
fn new_release_uris_deduplicate_across_groups_and_variants() {
    let mut message = empty_artist_message();
    message.album_group.push(group(&[10, 11]));
    message.single_group.push(group(&[12]));
    message.album_group.push(group(&[12]));

    assert_eq!(
        new_release_uris(&artist(message)),
        vec![album_uri(10), album_uri(12)],
        "the current variant only, and the first sighting wins"
    );
}

fn album(kind: ReleaseKind, date: Option<ReleaseDate>) -> super::DatedAlbum {
    super::DatedAlbum {
        uri: "spotify:album:r".to_owned(),
        name: "Slow Tide".to_owned(),
        kind,
        date,
        cover: None,
    }
}

#[test]
fn a_release_the_shelf_can_place_becomes_one() {
    let release = new_release_of(
        "Vela Moreno",
        &album(ReleaseKind::Album, Some(dated(2025, Some(10), Some(14)))),
        Day::from_days(calendar(2025, 9, 14)),
    )
    .expect("a dated release inside the window is one");

    assert_eq!(release.artist, "Vela Moreno");
    assert_eq!(release.uri, "spotify:album:r");
    assert_eq!(release.name, "Slow Tide");
    assert_eq!(release.date, dated(2025, Some(10), Some(14)));
    assert_eq!(release.day.days(), calendar(2025, 10, 14));
}

#[test]
fn a_compilation_or_an_audiobook_is_not_a_new_release() {
    for kind in [ReleaseKind::Compilation, ReleaseKind::Other] {
        assert_eq!(
            new_release_of(
                "Vela Moreno",
                &album(kind, Some(dated(2025, Some(10), Some(14)))),
                Day::from_days(0)
            ),
            None,
            "{kind:?} was drawn as a new release"
        );
    }
}

#[test]
fn a_release_with_a_partial_date_is_not_a_new_release() {
    for date in [dated(2025, None, None), dated(2025, Some(10), None)] {
        assert_eq!(
            new_release_of(
                "Vela Moreno",
                &album(ReleaseKind::Album, Some(date)),
                Day::from_days(0)
            ),
            None,
            "{date:?} names no day to place"
        );
    }
    assert_eq!(
        new_release_of(
            "Vela Moreno",
            &album(ReleaseKind::Album, None),
            Day::from_days(0)
        ),
        None,
        "a release with no date at all is not one"
    );
}

#[test]
fn a_release_before_the_window_is_dropped_and_the_edge_is_kept() {
    let since = Day::from_days(calendar(2025, 9, 14));

    let before = album(ReleaseKind::Album, Some(dated(2025, Some(9), Some(13))));
    assert_eq!(new_release_of("Vela Moreno", &before, since), None);

    let edge = album(ReleaseKind::Album, Some(dated(2025, Some(9), Some(14))));
    assert!(
        new_release_of("Vela Moreno", &edge, since).is_some(),
        "the window includes the day it starts on"
    );
}
