use super::*;
use takofy_model::{PlaylistAccess, Revision};

fn account() -> Account {
    Account::from_wire("someone").expect("a name")
}

fn account_hex(account: Account) -> String {
    format!("{:016x}", account.key())
}

fn cdn(n: u8) -> CoverUrl {
    let mut id = [0u8; 20];
    id[0] = n;
    CoverUrl::from_file_id(&id).expect("a file id")
}

fn cover(variants: &[(Option<(u32, u32)>, u8)]) -> CoverSet {
    CoverSet::from_sizes(variants.iter().map(|(size, n)| (*size, cdn(*n)))).expect("a cover set")
}

fn named(name: &str) -> OwnerName {
    OwnerName::from_wire(name).expect("a name")
}

fn saved(millis: i64) -> SavedAt {
    SavedAt::from_millis(millis).expect("a date")
}

/// A snapshot holding every field, in both its filled and its empty spelling.
fn full() -> Snapshot {
    Snapshot {
        account: account(),
        playlists: Some(vec![
            PlaylistEntry {
                uri: "spotify:playlist:0a1".to_owned(),
                name: "A\tname\nwith\\everything\r".to_owned(),
                named: true,
                cover: Some(PlaylistCover::chosen(cover(&[
                    (Some((640, 640)), 1),
                    (None, 2),
                ]))),
                composed: vec![
                    cover(&[(Some((64, 64)), 3)]),
                    cover(&[(Some((300, 300)), 4), (Some((640, 640)), 5)]),
                ],
                description: Some("a description\twith a tab and \u{7}".to_owned()),
                tracks: Some(42),
                owner: Some(Owner::Named(named("Ana ✨"))),
                access: None,
                saved_at: Some(saved(1_700_000_000_000)),
            },
            PlaylistEntry {
                uri: "spotify:playlist:0a2".to_owned(),
                name: "spotify:playlist:0a2".to_owned(),
                named: false,
                cover: Some(PlaylistCover::derived(cover(&[(Some((300, 300)), 1)]))),
                composed: Vec::new(),
                description: None,
                tracks: None,
                owner: Some(Owner::You),
                access: None,
                saved_at: None,
            },
        ]),
        shows: Some(vec![
            FollowedShow {
                uri: "spotify:show:1".to_owned(),
                name: "A show".to_owned(),
                named: true,
                publisher: Some("A publisher".to_owned()),
                episodes: Some(12),
                cover: Some(cover(&[(None, 6)])),
            },
            FollowedShow {
                uri: "spotify:show:2".to_owned(),
                name: "spotify:show:2".to_owned(),
                named: false,
                publisher: None,
                episodes: None,
                cover: None,
            },
        ]),
        artists: Some(vec![FollowedArtist {
            uri: "spotify:artist:1".to_owned(),
            name: "An artist".to_owned(),
            named: true,
            portrait: Some(cover(&[(Some((160, 160)), 7), (Some((640, 640)), 8)])),
            releases: Some(4),
        }]),
        albums: Some(vec![SavedAlbum {
            uri: "spotify:album:1".to_owned(),
            name: "An album".to_owned(),
            named: true,
            artists: "Ana, Bo".to_owned(),
            artist_uris: vec!["spotify:artist:1".to_owned(), "spotify:artist:2".to_owned()],
            artist_names: vec!["Ana".to_owned(), "Bo".to_owned()],
            year: Some(1999),
            cover: Some(cover(&[(Some((300, 300)), 9)])),
            tracks: Some(10),
            saved_at: Some(saved(1_600_000_000_000)),
        }]),
        liked: Some(vec![
            LikedTrack {
                uri: "spotify:track:1".to_owned(),
                saved_at: Some(saved(1_500_000_000_000)),
            },
            LikedTrack {
                uri: "spotify:track:2".to_owned(),
                saved_at: None,
            },
        ]),
    }
}

/// A file whose sections are listed as `(name, declared count, records)`.
fn file(sections: &[(&str, usize, &[&str])]) -> String {
    let mut text = format!("takofy-library\t1\naccount\t{}\n", account_hex(account()));
    for (name, count, rows) in sections {
        let _ = writeln!(text, "section\t{name}\t{count}");
        for row in *rows {
            text.push_str(row);
            text.push('\n');
        }
    }
    text.push_str("end\n");
    text
}

fn reason(text: &str) -> Reason {
    parse(text).expect_err("a refusal").reason
}

fn playlist_file(record: &str) -> String {
    file(&[("playlists", 1, &[record])])
}

fn show_file(record: &str) -> String {
    file(&[("shows", 1, &[record])])
}

fn liked_file(record: &str) -> String {
    file(&[("liked", 1, &[record])])
}

fn album_file(record: &str) -> String {
    file(&[("albums", 1, &[record])])
}

#[test]
fn every_field_round_trips() {
    let snapshot = full();
    assert_eq!(parse(&render(&snapshot)).expect("a read copy"), snapshot);
}

#[test]
fn an_empty_list_is_not_an_absent_list() {
    let snapshot = Snapshot {
        playlists: Some(Vec::new()),
        shows: Some(Vec::new()),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(text.contains("section\tplaylists\t0\n"));
    assert!(text.contains("section\tshows\t0\n"));
    assert!(!text.contains("section\tliked"));
    assert_eq!(parse(&text).expect("a read copy"), snapshot);
}

#[test]
fn awkward_text_round_trips() {
    let awkward = "a\\tb\tc\nd\\e\rf\u{0}g🎵\\";
    let snapshot = Snapshot {
        playlists: Some(vec![PlaylistEntry {
            uri: "spotify:playlist:1".to_owned(),
            name: awkward.to_owned(),
            named: true,
            cover: None,
            composed: Vec::new(),
            description: Some(awkward.to_owned()),
            tracks: None,
            owner: None,
            access: None,
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(!text.contains('\r'));
    assert_eq!(parse(&text).expect("a read copy"), snapshot);
}

#[test]
fn cover_origins_sizes_and_composed_round_trip() {
    let snapshot = Snapshot {
        playlists: Some(vec![PlaylistEntry {
            uri: "spotify:playlist:1".to_owned(),
            name: "One".to_owned(),
            named: true,
            cover: Some(PlaylistCover::derived(cover(&[
                (None, 1),
                (Some((64, 64)), 2),
            ]))),
            composed: vec![cover(&[(Some((300, 300)), 3)])],
            description: None,
            tracks: None,
            owner: None,
            access: None,
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(text.contains("derived:64x64@https://i.scdn.co/image/"));
    assert!(text.contains(" -@https://i.scdn.co/image/"));
    assert_eq!(parse(&text).expect("a read copy"), snapshot);
}

#[test]
fn owners_round_trip() {
    let owner = |owner| PlaylistEntry {
        uri: "spotify:playlist:1".to_owned(),
        name: "One".to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner,
        access: None,
        saved_at: None,
    };
    let snapshot = Snapshot {
        playlists: Some(vec![
            owner(None),
            owner(Some(Owner::You)),
            owner(Some(Owner::Named(named("Ana Bo")))),
        ]),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(text.contains("\t-\t-\t-\n"));
    assert!(text.contains("\t-\t-\tyou\t-\n"));
    assert!(text.contains("\t-\t-\tnamed:Ana Bo\t-\n"));
    assert_eq!(parse(&text).expect("a read copy"), snapshot);
}

#[test]
fn an_albums_artist_lists_round_trip() {
    let album = |uris: Vec<String>, names: Vec<String>| SavedAlbum {
        uri: "spotify:album:1".to_owned(),
        name: "One".to_owned(),
        named: true,
        artists: names.join(", "),
        artist_uris: uris,
        artist_names: names,
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    };
    let snapshot = Snapshot {
        albums: Some(vec![
            album(
                vec!["spotify:artist:1".to_owned(), "spotify:artist:2".to_owned()],
                vec!["Ana".to_owned(), "Bo".to_owned()],
            ),
            album(Vec::new(), Vec::new()),
        ]),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(text.contains("\t2\tspotify:artist:1\tspotify:artist:2\t2\tAna\tBo\t"));
    assert_eq!(parse(&text).expect("a read copy"), snapshot);
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(render(&full()), render(&full()));
}

/// Every cover field a copy can hold, with each set built in the wire's order:
/// the largest variant first, and an unsized one ahead of the sized ones.
fn wire_order() -> Snapshot {
    let playlist =
        |uri: &str, cover: Option<PlaylistCover>, composed: Vec<CoverSet>| PlaylistEntry {
            uri: uri.to_owned(),
            name: "One".to_owned(),
            named: true,
            cover,
            composed,
            description: None,
            tracks: None,
            owner: None,
            access: None,
            saved_at: None,
        };
    Snapshot {
        playlists: Some(vec![playlist(
            "spotify:playlist:1",
            Some(PlaylistCover::chosen(cover(&[
                (None, 1),
                (Some((640, 640)), 2),
            ]))),
            vec![
                cover(&[(Some((640, 640)), 3), (None, 4)]),
                cover(&[(Some((640, 300)), 5), (Some((300, 300)), 6)]),
                cover(&[(Some((1280, 720)), 7), (Some((640, 360)), 8)]),
            ],
        )]),
        shows: Some(vec![FollowedShow {
            uri: "spotify:show:1".to_owned(),
            name: "A show".to_owned(),
            named: true,
            publisher: None,
            episodes: None,
            cover: Some(cover(&[
                (None, 9),
                (Some((640, 640)), 10),
                (Some((300, 300)), 11),
            ])),
        }]),
        artists: Some(vec![FollowedArtist {
            uri: "spotify:artist:1".to_owned(),
            name: "An artist".to_owned(),
            named: true,
            portrait: Some(cover(&[
                (Some((640, 640)), 12),
                (Some((160, 160)), 13),
                (Some((300, 200)), 14),
            ])),
            releases: None,
        }]),
        albums: Some(vec![SavedAlbum {
            uri: "spotify:album:1".to_owned(),
            name: "An album".to_owned(),
            named: true,
            artists: "Ana".to_owned(),
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            year: None,
            cover: Some(cover(&[(Some((640, 640)), 15), (Some((64, 64)), 16)])),
            tracks: None,
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    }
}

#[test]
fn a_wire_order_copy_renders_like_its_round_trip() {
    let snapshot = wire_order();
    let text = render(&snapshot);
    let read = parse(&text).expect("a read copy");
    assert_eq!(render(&read), text);
    assert_eq!(render(&parse(&render(&read)).expect("a read copy")), text);
}

#[test]
fn cover_variants_are_written_by_size_and_unsized_last() {
    let text = render(&wire_order());
    let portrait = format!(
        "160x160@{small} 300x200@{wide} 640x640@{large}",
        small = cdn(13).as_str(),
        wide = cdn(14).as_str(),
        large = cdn(12).as_str(),
    );
    assert!(text.contains(&portrait), "{text}");
    let with_unsized = format!("640x640@{} -@{}", cdn(2).as_str(), cdn(1).as_str());
    assert!(text.contains(&with_unsized), "{text}");
}

#[test]
fn access_is_not_written() {
    let revision = Revision::from_wire(b"rev-42").expect("a revision");
    let snapshot = Snapshot {
        playlists: Some(vec![PlaylistEntry {
            uri: "spotify:playlist:1".to_owned(),
            name: "One".to_owned(),
            named: true,
            cover: None,
            composed: Vec::new(),
            description: None,
            tracks: Some(3),
            owner: None,
            access: Some(PlaylistAccess {
                revision,
                editable: true,
            }),
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(!text.contains("rev-42"));
    let read = parse(&text).expect("a read copy");
    assert!(read.playlists.expect("a list")[0].access.is_none());
}

#[test]
fn the_account_name_is_not_written() {
    let text = render(&Snapshot::new(account()));
    assert!(!text.contains("someone"));
    assert!(text.contains(&format!("account\t{}\n", account_hex(account()))));
}

#[test]
fn the_header_is_the_version_and_a_hex_account() {
    let text = render(&Snapshot::new(account()));
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("takofy-library\t1"));
    let account_line = lines.next().expect("an account line");
    let hex = account_line.strip_prefix("account\t").expect("an account");
    assert_eq!(hex, account_hex(account()));
    assert_eq!(hex.len(), 16);
    assert!(
        hex.bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );
    assert_eq!(lines.next(), Some("end"));
    assert!(lines.next().is_none());
}

#[test]
fn the_grammar_holds_no_track_rows() {
    let text = render(&full());
    assert!(!text.contains("section\ttracks"));
    assert!(!text.contains("section\taccount"));
}

#[test]
fn a_playlist_record_is_the_fields_in_order() {
    let snapshot = Snapshot {
        playlists: Some(vec![PlaylistEntry {
            uri: "spotify:playlist:1".to_owned(),
            name: "One".to_owned(),
            named: true,
            cover: None,
            composed: Vec::new(),
            description: None,
            tracks: None,
            owner: None,
            access: None,
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    };
    let text = render(&snapshot);
    assert!(text.contains("\nspotify:playlist:1\tOne\t1\t-\t0\t-\t-\t-\t-\n"));
}

#[test]
fn a_liked_record_is_the_fields_in_order() {
    let snapshot = Snapshot {
        liked: Some(vec![LikedTrack {
            uri: "spotify:track:1".to_owned(),
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    };
    assert!(render(&snapshot).contains("\nspotify:track:1\t-\n"));
}

#[test]
fn an_empty_file_is_refused() {
    assert_eq!(reason(""), Reason::Empty);
}

#[test]
fn a_foreign_header_is_refused() {
    assert_eq!(
        reason(&file(&[]).replace("takofy-library", "takofy-librarv")),
        Reason::BadHeader
    );
    assert_eq!(
        reason(&format!(
            "takofy-library\naccount\t{}\nend\n",
            account_hex(account())
        )),
        Reason::BadHeader
    );
}

#[test]
fn another_version_is_refused() {
    for version in ["0", "2", "one"] {
        let text = file(&[]).replace("takofy-library\t1", &format!("takofy-library\t{version}"));
        assert_eq!(reason(&text), Reason::BadVersion, "version {version}");
    }
}

#[test]
fn an_account_that_is_not_16_lowercase_hex_is_refused() {
    for account in [
        "ABCDEF0123456789",
        "0123456789abcde",
        "0123456789abcdef0",
        "gggggggggggggggg",
        "",
    ] {
        let text = format!("takofy-library\t1\naccount\t{account}\nend\n");
        assert_eq!(reason(&text), Reason::BadAccount, "account {account:?}");
    }
}

#[test]
fn a_missing_account_line_is_refused() {
    assert_eq!(reason("takofy-library\t1\nend\n"), Reason::MissingAccount);
}

#[test]
fn a_missing_final_newline_is_refused() {
    let mut text = file(&[]);
    text.pop();
    assert_eq!(reason(&text), Reason::NoFinalNewline);
}

#[test]
fn a_missing_end_is_refused() {
    let text = format!("takofy-library\t1\naccount\t{}\n", account_hex(account()));
    assert_eq!(reason(&text), Reason::MissingEnd);
}

#[test]
fn data_after_end_is_refused() {
    let mut text = file(&[]);
    text.push_str("more\n");
    assert_eq!(reason(&text), Reason::DataAfterEnd);
}

#[test]
fn a_carriage_return_is_refused() {
    assert_eq!(
        reason(&file(&[]).replace('\n', "\r\n")),
        Reason::CarriageReturn
    );
}

#[test]
fn a_byte_order_mark_is_refused() {
    assert_eq!(
        reason(&format!("\u{feff}{}", file(&[]))),
        Reason::ByteOrderMark
    );
}

#[test]
fn every_truncation_is_refused() {
    let text = render(&full());
    for (at, _) in text.char_indices() {
        assert!(
            parse(&text[..at]).is_err(),
            "a prefix of {at} bytes parsed as a whole file"
        );
    }
}

#[test]
fn a_count_above_the_lines_left_is_refused() {
    assert_eq!(reason(&file(&[("liked", 2, &[])])), Reason::BadCount);
    assert_eq!(
        reason(&format!(
            "takofy-library\t1\naccount\t{}\nsection\tliked\t\nend\n",
            account_hex(account())
        )),
        Reason::BadCount
    );
}

#[test]
fn a_count_below_the_records_is_refused() {
    let text = file(&[("liked", 1, &["spotify:track:1\t-", "spotify:track:2\t-"])]);
    assert_eq!(reason(&text), Reason::Unexpected);
}

#[test]
fn a_huge_count_is_refused_without_allocating() {
    assert_eq!(
        reason(&file(&[("liked", usize::MAX, &[])])),
        Reason::BadCount
    );
    let text = format!(
        "takofy-library\t1\naccount\t{}\nsection\tliked\t99999999999999999999999999\nend\n",
        account_hex(account())
    );
    assert_eq!(reason(&text), Reason::BadCount);
}

#[test]
fn a_short_record_is_refused() {
    assert_eq!(reason(&liked_file("spotify:track:1")), Reason::RecordFields);
}

#[test]
fn a_long_record_is_refused() {
    assert_eq!(
        reason(&liked_file("spotify:track:1\t-\textra")),
        Reason::RecordFields
    );
}

#[test]
fn a_flag_that_is_not_a_bit_is_refused() {
    assert_eq!(
        reason(&playlist_file(
            "spotify:playlist:1\tOne\t2\t-\t0\t-\t-\t-\t-"
        )),
        Reason::BadBool
    );
}

#[test]
fn a_number_that_is_not_digits_is_refused() {
    assert_eq!(
        reason(&liked_file("spotify:track:1\t+5")),
        Reason::BadNumber
    );
    assert_eq!(
        reason(&playlist_file(
            "spotify:playlist:1\tOne\t1\t-\t0\t-\t-5\t-\t-"
        )),
        Reason::BadNumber
    );
}

#[test]
fn a_date_of_zero_is_refused() {
    assert_eq!(reason(&liked_file("spotify:track:1\t0")), Reason::BadDate);
}

#[test]
fn an_unknown_escape_is_refused() {
    assert_eq!(
        reason(&liked_file("spotify:track:1\\q\t-")),
        Reason::BadEscape
    );
}

#[test]
fn a_foreign_cover_host_is_refused() {
    assert_eq!(
        reason(&show_file(
            "spotify:show:1\tShow\t1\t-\t-\t-@https://evil.example/x"
        )),
        Reason::BadCoverUrl
    );
}

#[test]
fn an_unusable_cover_size_is_refused() {
    let record = format!("spotify:show:1\tShow\t1\t-\t-\t0x640@{}", cdn(1).as_str());
    assert_eq!(reason(&show_file(&record)), Reason::BadSize);
    let record = format!("spotify:show:1\tShow\t1\t-\t-\t-{}", cdn(1).as_str());
    assert_eq!(reason(&show_file(&record)), Reason::BadCover);
}

#[test]
fn a_digits_only_owner_is_refused() {
    assert_eq!(
        reason(&playlist_file(
            "spotify:playlist:1\tOne\t1\t-\t0\t-\t-\tnamed:12345\t-"
        )),
        Reason::BadOwner
    );
}

#[test]
fn a_bad_optional_text_is_refused() {
    assert_eq!(
        reason(&playlist_file(
            "spotify:playlist:1\tOne\t1\t-\t0\tdesc\t-\t-\t-"
        )),
        Reason::BadText
    );
}

#[test]
fn a_bad_playlist_cover_is_refused() {
    let record = format!(
        "spotify:playlist:1\tOne\t1\t640x640@{}\t0\t-\t-\t-\t-",
        cdn(1).as_str()
    );
    assert_eq!(reason(&playlist_file(&record)), Reason::BadCover);
    let record = "spotify:playlist:1\tOne\t1\t-\t1\t-\t-\t-\t-\t-";
    assert_eq!(reason(&playlist_file(record)), Reason::BadCover);
}

#[test]
fn a_repeated_section_is_refused() {
    assert_eq!(
        reason(&file(&[("liked", 0, &[]), ("liked", 0, &[])])),
        Reason::DuplicateSection
    );
}

#[test]
fn an_unknown_section_is_refused() {
    assert_eq!(reason(&file(&[("tracks", 0, &[])])), Reason::UnknownSection);
}

#[test]
fn a_section_out_of_order_is_refused() {
    assert_eq!(
        reason(&file(&[("shows", 0, &[]), ("playlists", 0, &[])])),
        Reason::SectionOrder
    );
}

#[test]
fn a_malformed_section_line_is_refused() {
    assert_eq!(
        reason(&format!(
            "takofy-library\t1\naccount\t{}\nsection\tliked\nend\n",
            account_hex(account())
        )),
        Reason::BadSection
    );
}

#[test]
fn an_album_whose_artist_lists_differ_is_refused() {
    let record = "spotify:album:1\tAlbum\t1\tAna, Bo\t2\tspotify:artist:1\tspotify:artist:2\t1\tAna\t-\t-\t-\t-";
    assert_eq!(reason(&album_file(record)), Reason::ArtistLists);
}
