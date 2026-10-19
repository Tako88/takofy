//! The collection-v2 request and answer codecs.
//!
//! Spotify's followed-artist set is written with a hand-encoded protobuf
//! request, because librespot compiles no message for the collection endpoints.
//! So each byte is decided here and pinned by a test: a mistake is a 4xx at
//! best, and a silently changed write at worst.
//!
//! Only what this page needs is encoded: one artist per write, and the presence
//! probe the hero reads its label from.

/// The media type both collection endpoints want on `Content-Type` and `Accept`.
pub(crate) const VENDOR: &str = "application/vnd.collection-v2.spotify.proto";

/// The set an artist is followed into.
const SET: &str = "artist";

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
}
