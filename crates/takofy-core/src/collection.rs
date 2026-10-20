//! The collection-v2 request and answer codecs.
//!
//! Spotify's collection sets are written and paged with hand-encoded protobuf,
//! because librespot compiles no message for the collection endpoints. So each
//! byte is decided here and pinned by a test: a mistake is a 4xx at best, and a
//! silently changed write at worst.
//!
//! Only what the app needs is encoded: one artist per follow write, the presence
//! probe the hero reads its label from, and the paged read of the followed
//! shows.

/// The media type both collection endpoints want on `Content-Type` and `Accept`.
pub(crate) const VENDOR: &str = "application/vnd.collection-v2.spotify.proto";

/// The set an artist is followed into.
const SET: &str = "artist";

/// The set the followed shows are collected into.
#[allow(
    dead_code,
    reason = "the collection paging read in `remote` pages this set"
)]
pub(crate) const SHOW_SET: &str = "show";

/// One write request: the username, the set, and one item.
///
/// A follow item carries the instant it was added; an unfollow carries no
/// instant and sets `is_removed`. Both are what the probe observed the endpoint
/// accepting.
pub(crate) fn write_body(
    username: &str,
    uri: &str,
    added_at: Option<i64>,
    removed: bool,
) -> Vec<u8> {
    let mut item = Vec::new();
    put_str(1, uri, &mut item);
    if let Some(added_at) = added_at {
        put_varint_field(2, added_at as u64, &mut item);
    }
    if removed {
        put_varint_field(3, 1, &mut item);
    }

    let mut body = Vec::new();
    put_str(1, username, &mut body);
    put_str(2, SET, &mut body);
    put_message(3, &item, &mut body);
    body
}

/// One contains request: the username, the set, and one item per URI.
pub(crate) fn contains_body(username: &str, uris: &[&str]) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(1, username, &mut body);
    put_str(2, SET, &mut body);
    for uri in uris {
        let mut item = Vec::new();
        put_str(1, uri, &mut item);
        put_message(3, &item, &mut body);
    }
    body
}

/// One paging request: the username, the set, the token to continue from, and
/// how many items to ask for at most.
#[allow(
    dead_code,
    reason = "the collection paging read in `remote` pages with it"
)]
pub(crate) fn paging_body(username: &str, set: &str, token: Option<&str>, limit: u32) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(1, username, &mut body);
    put_str(2, set, &mut body);
    if let Some(token) = token {
        put_str(3, token, &mut body);
    }
    put_varint_field(4, u64::from(limit), &mut body);
    body
}

/// One item of a paging answer: a URI, the instant it was added, and whether the
/// user has since removed it.
#[allow(
    dead_code,
    reason = "the collection paging read in `remote` pages with it"
)]
#[derive(Debug)]
pub(crate) struct Item {
    pub(crate) uri: String,
    pub(crate) added_at: Option<i64>,
    pub(crate) removed: bool,
}

/// One page of a paging answer: its items, and the token that asks for the next.
#[allow(
    dead_code,
    reason = "the collection paging read in `remote` pages with it"
)]
#[derive(Debug)]
pub(crate) struct Page {
    pub(crate) items: Vec<Item>,
    pub(crate) next: Option<String>,
}

/// One page of a paged collection set.
///
/// Deliberately stricter than [`parse_contains`]: a truncated or unreadable page
/// is no page at all, so a paged read fails rather than drawing a short list as
/// if it were complete. A field this codec knows, written with another wire type
/// than this schema gives it, is no page either: skipping it would drop an item,
/// or let an empty URI pass for one. A field this codec does not read is skipped
/// by its wire type; a group, which this endpoint does not send, is not a page.
/// An empty body is an empty last page, because a set with nothing in it answers
/// exactly that.
///
/// The first next-token field is the one read. A page that repeated the field
/// does not rename the page after it, so a second token is ignored rather than
/// letting a stray later value redirect the read.
#[allow(
    dead_code,
    reason = "the collection paging read in `remote` pages with it"
)]
pub(crate) fn parse_page(bytes: &[u8]) -> Option<Page> {
    let mut items = Vec::new();
    let mut next = None;
    let mut next_seen = false;
    let mut at = 0;
    while at < bytes.len() {
        let key = read_varint(bytes, &mut at)?;
        match (key >> 3, key & 7) {
            (1, 2) => items.push(parse_item(read_bytes(bytes, &mut at)?)?),
            (2, 2) => {
                let token = read_bytes(bytes, &mut at)?;
                if !next_seen {
                    next_seen = true;
                    next = match token {
                        [] => None,
                        token => Some(std::str::from_utf8(token).ok()?.to_owned()),
                    };
                }
            }
            (1 | 2, _) => return None,
            _ => skip_field(bytes, key, &mut at)?,
        }
    }
    Some(Page { items, next })
}

/// One item of a page: its URI, its date and its removal flag.
///
/// The context URI an item may carry is skipped, whatever wire type it comes
/// with: the followed list is not drawn by where a show was saved from. The three
/// fields this codec does read must come with the wire type this schema gives
/// them; anything else is no item, so a malformed item cannot vanish from the
/// list or turn up as an empty URI.
fn parse_item(bytes: &[u8]) -> Option<Item> {
    let mut uri = String::new();
    let mut added_at = None;
    let mut removed = false;
    let mut at = 0;
    while at < bytes.len() {
        let key = read_varint(bytes, &mut at)?;
        match (key >> 3, key & 7) {
            (1, 2) => {
                uri = std::str::from_utf8(read_bytes(bytes, &mut at)?)
                    .ok()?
                    .to_owned()
            }
            // A date is a varint, and one wider than an `i64` holds is no date at
            // all: read as a wrapped negative it would sort as a very old follow.
            (2, 0) => added_at = i64::try_from(read_varint(bytes, &mut at)?).ok(),
            (3, 0) => removed = read_varint(bytes, &mut at)? != 0,
            (1..=3, _) => return None,
            _ => skip_field(bytes, key, &mut at)?,
        }
    }
    Some(Item {
        uri,
        added_at,
        removed,
    })
}

/// Steps over a field this codec does not read, by the wire type that says how
/// wide it is.
fn skip_field(bytes: &[u8], key: u64, at: &mut usize) -> Option<()> {
    match key & 7 {
        0 => read_varint(bytes, at).map(|_| ()),
        1 => advance(bytes, at, 8),
        2 => read_bytes(bytes, at).map(|_| ()),
        5 => advance(bytes, at, 4),
        _ => None,
    }
}

fn advance(bytes: &[u8], at: &mut usize, width: usize) -> Option<()> {
    let end = at.checked_add(width)?;
    bytes.get(*at..end)?;
    *at = end;
    Some(())
}

/// The answer's bools, in the order the request asked for them.
///
/// The wire sends the run packed (`field 1`, wire type 2, one byte per answer)
/// but the schema also allows one varint per entry, so both are read. A field
/// this page does not know is skipped by its wire type, and a body that ends
/// mid-field hands back what was read rather than failing: a truncated answer is
/// still an answer for the entries it completed.
pub(crate) fn parse_contains(bytes: &[u8]) -> Vec<bool> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(key) = read_varint(bytes, &mut at) {
        let field = key >> 3;
        match (field, key & 7) {
            (1, 0) => match read_varint(bytes, &mut at) {
                Some(value) => out.push(value != 0),
                None => break,
            },
            (1, 2) => {
                let Some(body) = read_bytes(bytes, &mut at) else {
                    break;
                };
                for byte in body {
                    out.push(*byte != 0);
                }
            }
            (_, 0) => {
                if read_varint(bytes, &mut at).is_none() {
                    break;
                }
            }
            (_, 2) => {
                if read_bytes(bytes, &mut at).is_none() {
                    break;
                }
            }
            (_, 1) => at += 8,
            (_, 5) => at += 4,
            _ => break,
        }
        if at > bytes.len() {
            break;
        }
    }
    out
}

fn put_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn put_varint_field(tag: u32, value: u64, out: &mut Vec<u8>) {
    put_varint(u64::from(tag) << 3, out);
    put_varint(value, out);
}

fn put_str(tag: u32, value: &str, out: &mut Vec<u8>) {
    put_varint((u64::from(tag) << 3) | 2, out);
    put_varint(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

fn put_message(tag: u32, body: &[u8], out: &mut Vec<u8>) {
    put_varint((u64::from(tag) << 3) | 2, out);
    put_varint(body.len() as u64, out);
    out.extend_from_slice(body);
}

fn read_varint(buf: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *buf.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

fn read_bytes<'a>(buf: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let len = read_varint(buf, at)? as usize;
    let end = at.checked_add(len)?;
    let slice = buf.get(*at..end)?;
    *at = end;
    Some(slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "alice";
    const ONE: &str = "spotify:artist:x";
    const TWO: &str = "spotify:artist:y";
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
            contains_body(USER, &[ONE, TWO]),
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
            0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77,
            0x3a, 0x78, // item 1: "spotify:show:x"
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
            0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77,
            0x3a, 0x78, // item 1: "spotify:show:x"
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
            0x0a, 0x0e, 0x73, 0x70, 0x6f, 0x74, 0x69, 0x66, 0x79, 0x3a, 0x73, 0x68, 0x6f, 0x77,
            0x3a, 0x78, // item 1: "spotify:show:x"
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
}
