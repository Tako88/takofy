use super::{
    AddedAt, CoverImage, CoverOrigin, CoverSet, CoverUrl, Episode, Move, Owner, OwnerName,
    PlaylistAccess, PlaylistCover, PlaylistEntry, PublishedAt, Removal, Revision, SavedAt,
    TrackEntry, TrackLyrics, composed_from,
};

fn revision(byte: u8) -> Revision {
    Revision::from_wire(&[byte]).expect("a revision")
}

fn playlist(tracks: Option<usize>, access: Option<PlaylistAccess>) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        uri: "spotify:playlist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks,
        owner: None,
        access,
    }
}

fn access(editable: bool) -> Option<PlaylistAccess> {
    Some(PlaylistAccess {
        revision: revision(1),
        editable,
    })
}

fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: "A Title".to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

#[test]
fn add_basis_needs_an_editable_revision_and_a_count() {
    assert_eq!(playlist(Some(4), None).add_basis(), None, "no access");
    assert_eq!(
        playlist(Some(4), access(false)).add_basis(),
        None,
        "not editable"
    );
    assert_eq!(playlist(None, access(true)).add_basis(), None, "no count");
    assert_eq!(playlist(None, None).add_basis(), None, "neither");

    assert_eq!(
        playlist(Some(4), access(true)).add_basis(),
        Some((&revision(1), 4))
    );

    // A count of zero is a real count: an append to an empty list names it.
    assert_eq!(
        playlist(Some(0), access(true)).add_basis(),
        Some((&revision(1), 0))
    );
}

#[test]
fn a_chosen_cover_and_a_derived_cover_differ_only_in_origin() {
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    assert_eq!(
        PlaylistCover::chosen(CoverSet::single(url.clone())),
        PlaylistCover {
            set: CoverSet::single(url.clone()),
            origin: CoverOrigin::Chosen,
        }
    );
    assert_eq!(
        PlaylistCover::derived(CoverSet::single(url.clone())),
        PlaylistCover {
            set: CoverSet::single(url),
            origin: CoverOrigin::Derived,
        }
    );
}

#[test]
fn a_playlist_entry_hands_out_its_cover_set() {
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    let mut entry = playlist(Some(1), access(true));
    assert_eq!(entry.cover_set(), None, "no cover named");
    entry.cover = Some(PlaylistCover::derived(CoverSet::single(url.clone())));
    assert_eq!(entry.cover_set(), Some(&CoverSet::single(url)));
}

fn cover(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

/// `composed_from` over a head whose entries are a cover tail or a label
/// that did not arrive.
fn composed(origin: Option<CoverOrigin>, head: &[Option<&str>]) -> Vec<CoverSet> {
    let covers: Vec<Option<CoverSet>> = head
        .iter()
        .map(|tail| tail.map(|tail| CoverSet::single(cover(tail))))
        .collect();
    composed_from(origin, covers.iter().map(Option::as_ref))
}

fn set(tail: &str) -> CoverSet {
    CoverSet::single(cover(tail))
}

#[test]
fn a_chosen_picture_composes_nothing() {
    let head = [Some("a"), Some("b"), Some("c"), Some("d")];
    assert!(composed(Some(CoverOrigin::Chosen), &head).is_empty());
    assert_eq!(
        composed(Some(CoverOrigin::Derived), &head),
        vec![set("a"), set("b"), set("c"), set("d")],
        "a derived cover is not the list's own"
    );
    assert_eq!(composed(None, &head).len(), 4);
}

#[test]
fn fewer_than_two_distinct_covers_compose_nothing() {
    assert!(composed(None, &[]).is_empty(), "an empty head");
    assert!(composed(None, &[Some("a")]).is_empty(), "one cover");
    assert!(
        composed(None, &[Some("a"), Some("a")]).is_empty(),
        "one album repeated"
    );
    assert!(composed(None, &[None, None]).is_empty(), "no labels");
}

#[test]
fn a_composed_head_stops_at_four() {
    let head = [
        Some("a"),
        Some("b"),
        Some("c"),
        Some("d"),
        Some("e"),
        Some("f"),
    ];
    assert_eq!(
        composed(None, &head),
        vec![set("a"), set("b"), set("c"), set("d")]
    );
}

#[test]
fn composed_covers_keep_their_head_order() {
    let head = [Some("c"), None, Some("a"), Some("b")];
    assert_eq!(composed(None, &head), vec![set("c"), set("a"), set("b")]);
}

#[test]
fn a_repeated_album_composes_once() {
    let head = [Some("a"), Some("b"), Some("a"), Some("c"), Some("b")];
    assert_eq!(composed(None, &head), vec![set("a"), set("b"), set("c")]);
}

#[test]
fn a_failed_label_is_skipped() {
    let head = [Some("a"), None, Some("b"), None];
    assert_eq!(composed(None, &head), vec![set("a"), set("b")]);
    assert!(
        composed(None, &[Some("a"), None]).is_empty(),
        "one named cover cannot tile"
    );
}

#[test]
fn an_episode_or_local_uri_is_not_a_track() {
    for uri in ["spotify:episode:1", "spotify:local:a:b:c:1", "", "list:p0"] {
        assert!(!track(uri).is_track(), "must refuse {uri:?}");
    }
    assert!(track("spotify:track:4uLU6hMCjMI75M1A2tKUQC").is_track());
}

#[test]
fn a_bare_track_uri_is_not_a_track() {
    assert!(!track("spotify:track:").is_track());
    assert!(!track("spotify:track").is_track());
}

#[test]
fn an_empty_revision_is_none() {
    assert_eq!(Revision::from_wire(&[]), None);
    assert!(
        Revision::from_wire(&[0]).is_some(),
        "a zero byte is still a byte"
    );
}

#[test]
fn a_revision_keeps_its_bytes() {
    let bytes = [0x00, 0x00, 0x00, 0x06, 0xc1, 0x52];
    let revision = Revision::from_wire(&bytes).expect("a revision");
    assert_eq!(revision.as_bytes(), &bytes);
}

#[test]
fn a_revision_never_prints_its_bytes() {
    let secret = format!(
        "{:?}",
        Revision::from_wire(&[0xde, 0xad]).expect("a revision")
    );
    assert_eq!(secret, "Revision(..)");

    let access = PlaylistAccess {
        revision: Revision::from_wire(&[0xde, 0xad]).expect("a revision"),
        editable: true,
    };
    let printed = format!("{access:?}");
    assert!(printed.contains("Revision(..)"), "{printed}");
    assert!(
        !printed.contains("222"),
        "the decimal bytes leaked: {printed}"
    );
}

fn episode() -> Episode {
    Episode {
        uri: "spotify:episode:1".to_owned(),
        title: "An episode".to_owned(),
        show: "Lateral with Tom Scott".to_owned(),
        show_uri: Some("spotify:show:abc".to_owned()),
        description: Some("A description".to_owned()),
        published: PublishedAt::from_millis(1_761_134_400_000),
        duration_ms: Some(2_773_461),
        cover: Some(CoverSet::single(
            CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
        )),
        named: true,
    }
}

#[test]
fn default_lyrics_are_untimed_and_empty() {
    let lyrics = TrackLyrics::default();
    assert!(lyrics.lines.is_empty());
    assert!(!lyrics.synced);
}

#[test]
fn published_at_rejects_zero_and_negative_and_keeps_positive() {
    assert_eq!(PublishedAt::from_millis(0), None);
    assert_eq!(PublishedAt::from_millis(-1), None);
    let published = PublishedAt::from_millis(1_761_134_400_000).expect("a positive instant");
    assert_eq!(published.as_millis(), 1_761_134_400_000);
    assert_eq!(PublishedAt::from_millis(i64::MIN), None);
    let widest = PublishedAt::from_millis(i64::MAX).expect("the widest instant");
    assert_eq!(widest.as_millis(), i64::MAX as u64);
}

#[test]
fn saved_at_from_millis_rejects_zero_and_negative_and_keeps_positive() {
    assert_eq!(SavedAt::from_millis(0), None);
    assert_eq!(SavedAt::from_millis(-1), None);
    let saved = SavedAt::from_millis(1_700_000_000_000).expect("a positive instant");
    assert_eq!(saved.as_millis(), 1_700_000_000_000);
}

#[test]
fn saved_at_from_secs_rejects_zero_and_negative() {
    assert_eq!(SavedAt::from_secs(0), None);
    assert_eq!(SavedAt::from_secs(-1), None);
}

#[test]
fn saved_at_from_secs_reads_seconds_as_milliseconds() {
    let saved = SavedAt::from_secs(1_700_000_000).expect("a positive instant");
    assert_eq!(saved.as_millis(), 1_700_000_000_000);
}

#[test]
fn saved_at_from_secs_rejects_an_overflowing_count() {
    assert_eq!(SavedAt::from_secs(i64::MAX), None);
}

#[test]
fn episode_entry_puts_the_show_name_in_artists_and_the_show_uri_in_artist_uris() {
    let entry = episode().entry();
    assert_eq!(entry.artists, "Lateral with Tom Scott");
    assert_eq!(entry.artist_uris, vec!["spotify:show:abc".to_owned()]);
    assert_eq!(
        entry.artist_names,
        vec!["Lateral with Tom Scott".to_owned()]
    );
}

#[test]
fn episode_entry_has_no_artist_uris_when_the_show_name_is_empty() {
    let mut value = episode();
    value.show = String::new();
    assert!(value.entry().artist_uris.is_empty());
    assert!(value.entry().artist_names.is_empty());
}

#[test]
fn episode_entry_keeps_the_show_name_without_artist_uris_when_the_show_uri_is_missing() {
    let mut value = episode();
    value.show_uri = None;
    let entry = value.entry();
    assert_eq!(entry.artists, "Lateral with Tom Scott");
    assert!(entry.artist_uris.is_empty());
    assert!(entry.artist_names.is_empty());
}

#[test]
fn episode_entry_carries_named_duration_and_cover_and_no_album_or_added_at() {
    let entry = episode().entry();
    assert_eq!(entry.uri, "spotify:episode:1");
    assert_eq!(entry.title, "An episode");
    assert_eq!(entry.album, None);
    assert_eq!(entry.album_uri, None);
    assert_eq!(entry.added_at, None);
    assert_eq!(entry.duration_ms, Some(2_773_461));
    assert_eq!(entry.cover, episode().cover);
    assert!(entry.named);

    // An unnamed episode must not come out of the entry named.
    let mut unnamed = episode();
    unnamed.named = false;
    assert!(!unnamed.entry().named);
}

#[test]
fn added_at_from_wire_reads_milliseconds_in_a_string() {
    let added = AddedAt::from_wire("1792357200000").expect("the issue's sample");
    assert_eq!(added.as_millis(), 1_792_357_200_000);
}

#[test]
fn added_at_from_wire_rejects_what_is_not_a_plain_count() {
    for wire in [
        "",
        " 1792357200000",
        "+1792357200000",
        "-1",
        "1.7e12",
        "abc",
        "1792357200000ms",
        "99999999999999999999999",
    ] {
        assert_eq!(AddedAt::from_wire(wire), None, "must reject {wire:?}");
    }
}

#[test]
fn added_at_from_wire_rejects_a_value_in_seconds() {
    assert_eq!(AddedAt::from_wire("1792357200"), None);
}

#[test]
fn added_at_floor_is_the_inclusive_boundary() {
    assert_eq!(AddedAt::from_wire("999999999999"), None);
    assert!(AddedAt::from_wire("1000000000000").is_some());
}

#[test]
fn a_20_byte_file_id_becomes_a_cdn_url() {
    let id: Vec<u8> = (1..=20).collect();
    let url = CoverUrl::from_file_id(&id).expect("a valid id");
    assert_eq!(
        &url.as_str()["https://i.scdn.co/image/".len()..],
        "0102030405060708090a0b0c0d0e0f1011121314"
    );
    assert_eq!(
        url.as_str().len(),
        "https://i.scdn.co/image/".len() + 40,
        "the hex is forty characters"
    );
}

#[test]
fn a_file_id_containing_a_zero_byte_is_still_a_file_id() {
    // Only an all-zero id is padding; a real id may hold a zero byte.
    let mut id = [1u8; 20];
    id[3] = 0;
    assert!(CoverUrl::from_file_id(&id).is_some());
}

#[test]
fn a_file_id_of_the_wrong_length_is_rejected() {
    assert_eq!(CoverUrl::from_file_id(&[]), None);
    assert_eq!(CoverUrl::from_file_id(&[1; 16]), None);
    assert_eq!(CoverUrl::from_file_id(&[1; 21]), None);
}

#[test]
fn an_all_zero_file_id_is_rejected() {
    assert_eq!(CoverUrl::from_file_id(&[0; 20]), None);
}

#[test]
fn from_wire_accepts_the_known_hosts() {
    for url in [
        "https://i.scdn.co/image/abc",
        "https://pickasso.spotifycdn.com/image/ab67c0de",
    ] {
        assert_eq!(
            CoverUrl::from_wire(url).map(|cover| cover.as_str().to_owned()),
            Some(url.to_owned())
        );
    }
}

#[test]
fn from_wire_rejects_everything_else() {
    for url in [
        "http://i.scdn.co/image/abc",
        "/image/abc",
        "https://i.scdn.co.example.com/image/abc",
        "https://i.scdn.co@evil.example/image/abc",
        "https://evil.example\\@i.scdn.co/x",
        "https://i.scdn.co\\@evil.example/x",
        "https://i.scdn.co\t.evil/x",
        "https://evil.example/i.scdn.co.png",
        "https://i.scdn.co",
    ] {
        assert_eq!(CoverUrl::from_wire(url), None, "must reject {url}");
    }
}

#[test]
fn cover_image_requires_a_positive_shape() {
    assert!(CoverImage::new(2, 2, vec![0; 16]).is_some());
    assert_eq!(CoverImage::new(0, 2, vec![0; 0]), None);
    assert_eq!(CoverImage::new(2, 0, vec![0; 0]), None);
}

#[test]
fn cover_image_rejects_a_mismatched_buffer() {
    assert_eq!(CoverImage::new(2, 2, vec![0; 15]), None);
    assert_eq!(CoverImage::new(2, 2, vec![0; 17]), None);
}

#[test]
fn a_move_is_a_value_the_whole_write_can_share() {
    let mv = Move {
        from: 1,
        length: 3,
        to: 6,
    };
    let copy = mv;
    assert_eq!(copy, mv);
    assert_ne!(mv, Move { length: 1, ..mv });
}

#[test]
fn a_removal_is_a_value_the_chain_can_share() {
    let removal = Removal { from: 5, length: 2 };
    let copy = removal;
    assert_eq!(copy, removal);
    assert_ne!(removal, Removal { from: 4, ..removal });
}

#[test]
fn owner_name_rejects_blank_and_whitespace() {
    assert_eq!(OwnerName::from_wire(""), None);
    assert_eq!(OwnerName::from_wire("   "), None);
    assert_eq!(OwnerName::from_wire(" \n\t "), None);
}

#[test]
fn owner_name_rejects_a_bare_number() {
    assert_eq!(OwnerName::from_wire("31"), None);
    assert_eq!(OwnerName::from_wire("31415926535"), None);
    assert_eq!(OwnerName::from_wire("  42  "), None);
}

#[test]
fn owner_name_keeps_a_name_with_digits_and_letters() {
    assert!(OwnerName::from_wire("31l6").is_some());
    assert!(OwnerName::from_wire("alice123").is_some());
    assert!(OwnerName::from_wire("Vela Moreno").is_some());
}

#[test]
fn owner_name_is_trimmed() {
    let name = OwnerName::from_wire("  Vela Moreno \n").expect("a name");
    assert_eq!(name.as_str(), "Vela Moreno");
}

#[test]
fn owner_name_reads_back() {
    let name = OwnerName::from_wire("Vela Moreno").expect("a name");
    assert_eq!(name.as_str(), "Vela Moreno");
}

#[test]
fn owner_name_method_is_none_for_you() {
    assert_eq!(Owner::You.name(), None);
    let named = Owner::Named(OwnerName::from_wire("Vela Moreno").expect("a name"));
    assert_eq!(named.name(), Some("Vela Moreno"));
}
