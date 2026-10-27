use super::*;
use crate::wire::put_varint;

const USER: &str = "alice";
const ONE: &str = "spotify:artist:x";
const TWO: &str = "spotify:artist:y";
const SHOW: &str = "spotify:show:x";
const TRACK_ONE: &str = "spotify:track:a";
const TRACK_TWO: &str = "spotify:track:b";
/// The label a write names itself with; the endpoint accepts any string.
const UPDATE_ID: &str = "takofy-1";
/// 2023-11-14T22:13:20Z, the instant the probe wrote.
const ADDED_AT: i64 = 1_700_000_000;

#[test]
fn a_follow_write_carries_the_timestamp_and_no_removal() {
    assert_eq!(
        write_body(USER, ONE, Some(ADDED_AT), false),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, // field 1: the username
            0x12, 0x06, 0x61, 0x72, 0x74, 0x69, 0x73, 0x74, // field 2: "artist"
            0x1a, 0x18, // field 3: one item, 24 bytes
            0x0a, 0x10, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x61, 0x72, 0x74, 0x69,
            0x73, 0x74, 0x3a, 0x78, // item 1: the uri
            0x10, 0x80, 0xe2, 0xcf, 0xaa, 0x06, // item 2: added_at
        ]
    );
}

#[test]
fn an_unfollow_write_carries_no_timestamp_and_sets_is_removed() {
    assert_eq!(
        write_body(USER, ONE, None, true),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, //
            0x12, 0x06, 0x61, 0x72, 0x74, 0x69, 0x73, 0x74, //
            0x1a, 0x14, // one 20-byte item
            0x0a, 0x10, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x61, 0x72, 0x74, 0x69,
            0x73, 0x74, 0x3a, 0x78, //
            0x18, 0x01, // item 3: is_removed
        ]
    );
}

#[test]
fn a_contains_probe_names_every_uri_in_one_body() {
    assert_eq!(
        contains_body(USER, ARTIST_SET, &[ONE, TWO]),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, //
            0x12, 0x06, 0x61, 0x72, 0x74, 0x69, 0x73, 0x74, //
            0x1a, 0x12, 0x0a, 0x10, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x61, 0x72,
            0x74, 0x69, 0x73, 0x74, 0x3a, 0x78, //
            0x1a, 0x12, 0x0a, 0x10, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x61, 0x72,
            0x74, 0x69, 0x73, 0x74, 0x3a, 0x79,
        ]
    );
}

/// The set is the caller's: an album is asked about the collection, and the
/// body says so rather than naming the artist set out of habit.
#[test]
fn a_contains_probe_names_the_set_it_was_given() {
    let body = contains_body(USER, COLLECTION_SET, &["spotify:album:r"]);
    let artist = contains_body(USER, ARTIST_SET, &["spotify:album:r"]);
    assert_ne!(body, artist, "the set reached the body");
    assert!(
        body.windows(10).any(|run| run == COLLECTION_SET.as_bytes()),
        "the collection set is not in the body"
    );
}

#[test]
fn contains_reads_a_packed_bool_run() {
    // field 1, wire type 2, three bytes: true, false, true.
    assert_eq!(
        parse_contains(&[0x0a, 0x03, 0x01, 0x00, 0x01]),
        vec![true, false, true]
    );
}

#[test]
fn contains_reads_unpacked_bools() {
    assert_eq!(
        parse_contains(&[0x08, 0x01, 0x08, 0x00, 0x08, 0x01]),
        vec![true, false, true]
    );
}

#[test]
fn contains_reads_both_forms_one_after_the_other() {
    assert_eq!(
        parse_contains(&[0x08, 0x01, 0x0a, 0x02, 0x00, 0x01]),
        vec![true, false, true]
    );
}

#[test]
fn an_empty_contains_body_is_no_answer() {
    assert!(parse_contains(&[]).is_empty());
}

#[test]
fn a_truncated_contains_body_keeps_what_it_read() {
    assert_eq!(
        parse_contains(&[0x08, 0x01, 0x0a, 0x05, 0x01]),
        vec![true],
        "a body that promises more bytes than it has is not a reason to panic"
    );
    assert!(parse_contains(&[0x0a]).is_empty());
}

#[test]
fn a_field_this_page_does_not_read_is_skipped() {
    // field 2, wire type 2 then field 1 packed: an answer that grew a field.
    assert_eq!(
        parse_contains(&[0x12, 0x02, 0xaa, 0xbb, 0x0a, 0x02, 0x01, 0x00]),
        vec![true, false]
    );
}

/// One item body as the wire writes it.
fn item_body(uri: &str, added_at: Option<i64>, removed: bool) -> Vec<u8> {
    let mut item = Vec::new();
    put_str(1, uri, &mut item);
    if let Some(added_at) = added_at {
        put_varint_field(2, added_at as u64, &mut item);
    }
    if removed {
        put_varint_field(3, 1, &mut item);
    }
    item
}

#[test]
fn a_like_write_names_the_collection_set_one_item_per_uri_and_the_update_id() {
    assert_eq!(
        items_body(
            USER,
            COLLECTION_SET,
            &[TRACK_ONE, TRACK_TWO],
            Some(ADDED_AT),
            false,
            UPDATE_ID,
        ),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, // field 1: the username
            0x12, 0x0a, 0x63, 0x6f, 0x6c, 0x6c, 0x65, 0x63, 0x74, 0x69, 0x6f,
            0x6e, // field 2: "collection"
            0x1a, 0x17, // field 3: one 23-byte item
            0x0a, 0x0f, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x74, 0x72, 0x61, 0x63,
            0x6b, 0x3a, 0x61, // item 1: the uri
            0x10, 0x80, 0xe2, 0xcf, 0xaa, 0x06, // item 2: added_at
            0x1a, 0x17, // field 3: the second item
            0x0a, 0x0f, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x74, 0x72, 0x61, 0x63,
            0x6b, 0x3a, 0x62, // item 1: the uri
            0x10, 0x80, 0xe2, 0xcf, 0xaa, 0x06, // item 2: added_at
            0x22, 0x08, 0x74, 0x61, 0x6b, 0x6f, 0x66, 0x79, 0x2d, 0x31, // field 4: the id
        ]
    );
}

#[test]
fn a_like_item_carries_the_date_and_no_removal() {
    assert_eq!(
        items_body(
            USER,
            COLLECTION_SET,
            &[TRACK_ONE],
            Some(ADDED_AT),
            false,
            UPDATE_ID
        ),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, //
            0x12, 0x0a, 0x63, 0x6f, 0x6c, 0x6c, 0x65, 0x63, 0x74, 0x69, 0x6f, 0x6e, //
            0x1a, 0x17, //
            0x0a, 0x0f, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x74, 0x72, 0x61, 0x63,
            0x6b, 0x3a, 0x61, //
            0x10, 0x80, 0xe2, 0xcf, 0xaa, 0x06, //
            0x22, 0x08, 0x74, 0x61, 0x6b, 0x6f, 0x66, 0x79, 0x2d, 0x31,
        ]
    );
}

#[test]
fn an_unlike_item_carries_is_removed_and_no_date() {
    assert_eq!(
        items_body(USER, COLLECTION_SET, &[TRACK_ONE], None, true, UPDATE_ID),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, //
            0x12, 0x0a, 0x63, 0x6f, 0x6c, 0x6c, 0x65, 0x63, 0x74, 0x69, 0x6f, 0x6e, //
            0x1a, 0x13, //
            0x0a, 0x0f, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x74, 0x72, 0x61, 0x63,
            0x6b, 0x3a, 0x61, //
            0x18, 0x01, // item 3: is_removed
            0x22, 0x08, 0x74, 0x61, 0x6b, 0x6f, 0x66, 0x79, 0x2d, 0x31,
        ]
    );
}

#[test]
fn a_show_removal_names_the_show_set_with_no_date() {
    assert_eq!(
        items_body(USER, SHOW_SET, &[SHOW], None, true, UPDATE_ID),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, //
            0x12, 0x04, 0x73, 0x68, 0x6f, 0x77, // field 2: "show"
            0x1a, 0x12, // field 3: one 18-byte item
            0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77,
            0x3a, 0x78, // item 1: the uri
            0x18, 0x01, // item 3: is_removed
            0x22, 0x08, 0x74, 0x61, 0x6b, 0x6f, 0x66, 0x79, 0x2d, 0x31,
        ]
    );
}

#[test]
fn saved_set_maps_each_collectable_kind_to_its_set() {
    let set = |uri: &str| saved_set(uri).ok();
    assert_eq!(
        set("spotify:album:4uLU6hMCjMI75M1A2tKUQC"),
        Some(COLLECTION_SET)
    );
    assert_eq!(set("spotify:show:4uLU6hMCjMI75M1A2tKUQC"), Some(SHOW_SET));
    for refused in [
        "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:episode:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:artist:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:user:alice",
        "spotify:local:A:B:C:1000",
        "",
        "spotify:",
        "album:a",
    ] {
        assert_eq!(set(refused), None, "{refused:?} was given a set");
    }
}

#[test]
fn require_tracks_rejects_an_album_an_artist_an_episode_and_garbage_before_any_request() {
    use librespot::core::{SpotifyId, SpotifyUri};

    let uri = |uri: SpotifyUri| -> String { uri.to_uri().expect("a uri") };
    let id = SpotifyId::from_raw(&[1; 16]).expect("a 16-byte id");
    let good = [
        uri(SpotifyUri::Track { id }),
        uri(SpotifyUri::Track {
            id: SpotifyId::from_raw(&[2; 16]).expect("a 16-byte id"),
        }),
    ];
    assert!(require_tracks(&good).is_ok());

    for bad in [
        uri(SpotifyUri::Album { id }),
        uri(SpotifyUri::Artist { id }),
        uri(SpotifyUri::Episode { id }),
        "spotify:local:A:B:C:1000".to_owned(),
        "track:a".to_owned(),
        String::new(),
        "spotify:track:".to_owned(),
    ] {
        let uris = [good[0].clone(), bad.clone()];
        assert!(
            require_tracks(&uris).is_err(),
            "{bad:?} must be refused whole"
        );
    }
    let empty: [String; 0] = [];
    assert!(require_tracks(&empty).is_ok());
}

#[test]
fn like_batches_split_in_order_without_losing_or_repeating_a_uri() {
    assert!(
        like_batches(&[], 100).is_empty(),
        "an empty pick is no write"
    );

    let all: Vec<String> = (0..250).map(|n| format!("spotify:track:{n}")).collect();
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    let batches = like_batches(&refs, 100);
    assert_eq!(
        batches.iter().map(|batch| batch.len()).collect::<Vec<_>>(),
        vec![100, 100, 50]
    );
    let flat: Vec<&str> = batches.into_iter().flatten().copied().collect();
    assert_eq!(flat, refs, "every uri once, in the order it was picked");
}

#[test]
fn a_paging_request_without_a_token_carries_user_set_and_limit() {
    assert_eq!(
        paging_body(USER, SHOW_SET, None, 500),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, // field 1: the username
            0x12, 0x04, 0x73, 0x68, 0x6f, 0x77, // field 2: "show"
            0x20, 0xf4, 0x03, // field 4: the limit, 500
        ]
    );
}

#[test]
fn a_collection_paging_request_names_the_collection_set() {
    assert_eq!(
        paging_body(USER, COLLECTION_SET, None, 500),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, // field 1: the username
            0x12, 0x0a, 0x63, 0x6f, 0x6c, 0x6c, 0x65, 0x63, 0x74, 0x69, 0x6f,
            0x6e, // field 2:
            // "collection"
            0x20, 0xf4, 0x03, // field 4: the limit, 500
        ]
    );
}

#[test]
fn a_paging_request_with_a_token_inserts_it_before_the_limit() {
    assert_eq!(
        paging_body(USER, SHOW_SET, Some("t1"), 500),
        vec![
            0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65, //
            0x12, 0x04, 0x73, 0x68, 0x6f, 0x77, //
            0x1a, 0x02, 0x74, 0x31, // field 3: the token "t1"
            0x20, 0xf4, 0x03, //
        ]
    );
}

#[test]
fn a_page_reads_items_and_the_next_token() {
    let page = parse_page(&[
        0x0a, 0x16, // field 1: one 22-byte item
        0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77, 0x3a,
        0x78, // item 1: "spotify:show:x"
        0x10, 0x80, 0xe2, 0xcf, 0xaa, 0x06, // item 2: added_at
        0x12, 0x02, 0x74, 0x31, // field 2: the next token "t1"
    ])
    .expect("a page");
    assert_eq!(page.next, Some("t1".to_owned()));
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].uri, "spotify:show:x");
    assert_eq!(page.items[0].added_at, Some(ADDED_AT));
    assert!(!page.items[0].removed);
}

#[test]
fn a_removed_item_reads_as_removed() {
    let page = parse_page(&[
        0x0a, 0x12, // field 1: one 18-byte item
        0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77, 0x3a,
        0x78, // item 1: "spotify:show:x"
        0x18, 0x01, // item 3: is_removed
    ])
    .expect("a page");
    assert!(page.items[0].removed);
    assert_eq!(page.items[0].added_at, None);
}

#[test]
fn an_item_without_a_date_has_no_added_at() {
    let page = parse_page(&[
        0x0a, 0x10, // field 1: one 16-byte item
        0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77, 0x3a,
        0x78, // item 1: "spotify:show:x"
    ])
    .expect("a page");
    assert_eq!(page.items[0].added_at, None);
    assert!(!page.items[0].removed);
}

#[test]
fn an_empty_next_token_is_no_next_page() {
    let mut bytes = Vec::new();
    put_message(
        1,
        &item_body("spotify:show:x", Some(ADDED_AT), false),
        &mut bytes,
    );
    put_str(2, "", &mut bytes);
    let page = parse_page(&bytes).expect("a page");
    assert_eq!(page.next, None);
    assert_eq!(page.items.len(), 1);
}

#[test]
fn unknown_fields_in_a_page_and_an_item_are_skipped() {
    let mut item = item_body("spotify:show:x", Some(ADDED_AT), false);
    put_str(4, "spotify:user:alice:collection", &mut item); // the context uri
    put_varint_field(9, 42, &mut item); // a varint field this codec does not know

    let mut bytes = Vec::new();
    put_varint_field(3, 7, &mut bytes); // an unknown varint field
    bytes.extend_from_slice(&[0x31, 1, 2, 3, 4, 5, 6, 7, 8]); // field 6, 64-bit
    bytes.extend_from_slice(&[0x3d, 1, 2, 3, 4]); // field 7, 32-bit
    put_message(1, &item, &mut bytes);
    put_str(2, "t1", &mut bytes);

    let page = parse_page(&bytes).expect("a page");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].uri, "spotify:show:x");
    assert_eq!(page.items[0].added_at, Some(ADDED_AT));
    assert_eq!(page.next, Some("t1".to_owned()));
}

#[test]
fn a_truncated_page_is_not_a_page() {
    assert!(
        parse_page(&[0x0a, 0x16, 0x0a]).is_none(),
        "an item that promises 22 bytes and has one is not a page"
    );
    assert!(parse_page(&[0x12, 0x05, 0x74]).is_none(), "a short token");
}

#[test]
fn a_truncated_item_is_not_a_page() {
    assert!(
        parse_page(&[0x0a, 0x03, 0x0a, 0x05, 0x78]).is_none(),
        "an item whose uri promises five bytes and has one is not a page"
    );
}

#[test]
fn a_group_wire_type_is_not_a_page() {
    assert!(
        parse_page(&[0x0b]).is_none(),
        "a start group is not a field"
    );
    assert!(parse_page(&[0x0c]).is_none(), "an end group is not a field");
    assert!(
        parse_page(&[0x0a, 0x01, 0x0b]).is_none(),
        "a group inside an item"
    );
}

#[test]
fn an_empty_body_is_an_empty_last_page() {
    let page = parse_page(&[]).expect("a page");
    assert!(page.items.is_empty());
    assert_eq!(page.next, None);
}

/// One page whose only field is one item made of the given bytes.
fn page_of(item: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    put_message(1, item, &mut bytes);
    bytes
}

#[test]
fn an_added_at_that_does_not_fit_an_i64_is_no_date() {
    let mut item = Vec::new();
    put_str(1, "spotify:show:x", &mut item);
    put_varint_field(2, u64::MAX, &mut item); // `ff ff ff ff ff ff ff ff ff 01`
    let page = parse_page(&page_of(&item)).expect("a page");
    assert_eq!(
        page.items[0].added_at, None,
        "a wrapped negative would sort as a very old follow"
    );
}

#[test]
fn a_known_page_field_with_the_wrong_wire_type_is_not_a_page() {
    assert!(
        parse_page(&[0x08, 0x01]).is_none(),
        "an item written as a varint"
    );
    assert!(
        parse_page(&[0x10, 0x01]).is_none(),
        "a token written as a varint"
    );
    assert!(
        parse_page(&[0x0e]).is_none(),
        "an item written as a 64-bit field"
    );
    assert!(
        parse_page(&[0x0f, 0x01, 0x02, 0x03, 0x04]).is_none(),
        "a token written as a 32-bit field"
    );
}

#[test]
fn a_known_item_field_with_the_wrong_wire_type_is_not_a_page() {
    assert!(
        parse_page(&page_of(&[0x08, 0x01])).is_none(),
        "a uri written as a varint"
    );
    assert!(
        parse_page(&page_of(&[0x12, 0x01, 0x00])).is_none(),
        "a date written as a message"
    );
    assert!(
        parse_page(&page_of(&[0x1a, 0x01, 0x00])).is_none(),
        "a removal flag written as a message"
    );
    assert!(
        parse_page(&page_of(&[0x20, 0x01])).is_some(),
        "the context uri is skipped whatever its wire type"
    );
    assert!(
        parse_page(&page_of(&[0x22, 0x02, 0xaa, 0xbb])).is_some(),
        "the context uri is skipped whatever its wire type"
    );
}

#[test]
fn a_second_next_token_does_not_erase_the_first() {
    let mut bytes = Vec::new();
    put_str(2, "t1", &mut bytes);
    put_str(2, "t2", &mut bytes);
    assert_eq!(
        parse_page(&bytes).expect("a page").next,
        Some("t1".to_owned()),
        "the first token is the one the next request carries"
    );
}

#[test]
fn two_items_in_one_page_are_both_read_in_wire_order() {
    let mut bytes = Vec::new();
    put_message(1, &item_body("spotify:show:y", None, false), &mut bytes);
    put_message(
        1,
        &item_body("spotify:show:x", Some(ADDED_AT), false),
        &mut bytes,
    );
    let page = parse_page(&bytes).expect("a page");
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].uri, "spotify:show:y");
    assert_eq!(page.items[0].added_at, None);
    assert_eq!(page.items[1].uri, "spotify:show:x");
    assert_eq!(page.items[1].added_at, Some(ADDED_AT));
    assert_eq!(page.next, None);
}

#[test]
fn an_item_nested_in_an_item_is_a_garbage_uri_not_a_panic() {
    let inner = item_body("spotify:show:x", None, false);
    let mut nested = Vec::new();
    put_message(1, &inner, &mut nested); // item field 1, the uri, carries an item
    let page = parse_page(&page_of(&nested)).expect("the outer item is still an item");
    let uri = &page.items[0].uri;
    assert!(
        librespot::core::SpotifyUri::from_uri(uri).is_err(),
        "{uri:?} is not a uri, and a later filter drops it"
    );
    assert_eq!(page.items[0].added_at, None);
}

#[test]
fn a_uri_or_a_token_that_is_not_utf8_is_not_a_page() {
    assert!(
        parse_page(&page_of(&[0x0a, 0x01, 0xff])).is_none(),
        "an item whose uri is not utf-8"
    );
    let mut bytes = Vec::new();
    put_message(1, &item_body("spotify:show:x", None, false), &mut bytes);
    bytes.extend_from_slice(&[0x12, 0x01, 0xff]); // field 2: a token that is not utf-8
    assert!(parse_page(&bytes).is_none(), "a token that is not utf-8");
}

#[test]
fn a_varint_that_never_ends_is_not_a_page() {
    let mut item = vec![0x0a]; // field 1: an item, then a length that runs on
    item.extend_from_slice(&[0x80; 11]);
    assert!(parse_page(&item).is_none());
    assert!(
        parse_page(&[0x80; 11]).is_none(),
        "a key that runs on is not a field"
    );
}

#[test]
fn a_length_prefix_of_u64_max_is_not_a_page() {
    let mut bytes = vec![0x0a]; // field 1: an item
    put_varint(u64::MAX, &mut bytes);
    bytes.extend_from_slice(b"spotify:show:x");
    assert!(parse_page(&bytes).is_none(), "22 bytes promised, 14 sent");
    let mut token = vec![0x12]; // field 2: the next token
    put_varint(u64::MAX, &mut token);
    token.extend_from_slice(b"t1");
    assert!(parse_page(&token).is_none());
}

#[test]
fn wire_types_six_and_seven_are_not_a_page() {
    assert!(
        parse_page(&[0x4e]).is_none(),
        "field 9 with wire type 6 is not a field"
    );
    assert!(
        parse_page(&[0x4f]).is_none(),
        "field 9 with wire type 7 is not a field"
    );
    assert!(
        parse_page(&page_of(&[0x4e])).is_none(),
        "wire type 6 inside an item is not a field either"
    );
}
