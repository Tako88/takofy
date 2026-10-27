//! The collection-v2 request and answer codecs.
//!
//! Spotify's collection sets are written and paged with hand-encoded protobuf,
//! because librespot compiles no message for the collection endpoints. So each
//! body is decided here and pinned by a test: a mistake is a 4xx at best, and a
//! silently changed write at worst.
//!
//! Only what the app needs is encoded: one artist per follow write, the presence
//! probe the hero reads its label from, the paged read of the followed shows,
//! and the paged read of the collection set.

use crate::followed::FollowedError;
use crate::wire::{put_message, put_str, put_varint_field};
use librespot::core::SpotifyUri;

/// The media type both collection endpoints want on `Content-Type` and `Accept`.
pub(crate) const VENDOR: &str = "application/vnd.collection-v2.spotify.proto";

/// The set an artist is followed into.
pub(crate) const ARTIST_SET: &str = "artist";

/// The set that holds liked songs and saved albums together.
pub(crate) const COLLECTION_SET: &str = "collection";

/// The set the followed shows are collected into.
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
    put_str(2, ARTIST_SET, &mut body);
    put_message(3, &item, &mut body);
    body
}

/// One write request for a set: the username, the set, one item per URI, and
/// the label the endpoint de-duplicates by.
///
/// Every item carries the same date and removal flag, because one write is one
/// gesture: liking names the instant the batch was added, unliking sets
/// `is_removed` and carries no date.
pub(crate) fn items_body(
    username: &str,
    set: &str,
    uris: &[&str],
    added_at: Option<i64>,
    removed: bool,
    client_update_id: &str,
) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(1, username, &mut body);
    put_str(2, set, &mut body);
    for uri in uris {
        let mut item = Vec::new();
        put_str(1, uri, &mut item);
        if let Some(added_at) = added_at {
            put_varint_field(2, added_at as u64, &mut item);
        }
        if removed {
            put_varint_field(3, 1, &mut item);
        }
        put_message(3, &item, &mut body);
    }
    put_str(4, client_update_id, &mut body);
    body
}

/// The set a saved item of `uri` is written to, or a refusal.
///
/// Albums live in the collection set and shows in their own; a track is written
/// by the like path, and an artist is not in a set at all — removing a followed
/// artist is the unfollow. Refusing everything else keeps this writer out of
/// paths it has no set for.
pub(crate) fn saved_set(uri: &str) -> Result<&'static str, FollowedError> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Album { .. }) => Ok(COLLECTION_SET),
        Ok(SpotifyUri::Show { .. }) => Ok(SHOW_SET),
        _ => Err(FollowedError::Source(
            "only an album or a show has a saved set".to_owned(),
        )),
    }
}

/// Refuses a pick that names anything but a track, before a request is made.
///
/// The collection set holds episodes and albums too, and a like names only a
/// track: one wrong URI would make the whole write meaningless, so it is
/// refused whole.
pub(crate) fn require_tracks(uris: &[String]) -> Result<(), FollowedError> {
    if uris
        .iter()
        .all(|uri| matches!(SpotifyUri::from_uri(uri), Ok(SpotifyUri::Track { .. })))
    {
        return Ok(());
    }
    Err(FollowedError::Source(
        "a like writes tracks only".to_owned(),
    ))
}

/// The pick cut into writes of at most `per` URIs, in order.
///
/// A zero size writes nothing rather than panicking on an empty chunk.
pub(crate) fn like_batches<'a>(uris: &'a [&'a str], per: usize) -> Vec<&'a [&'a str]> {
    if per == 0 {
        return Vec::new();
    }
    uris.chunks(per).collect()
}

/// One contains request: the username, the set, and one item per URI.
///
/// The set is the caller's because the route answers for any of them: an artist
/// is asked about the followed set, an album about the collection.
pub(crate) fn contains_body(username: &str, set: &str, uris: &[&str]) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(1, username, &mut body);
    put_str(2, set, &mut body);
    for uri in uris {
        let mut item = Vec::new();
        put_str(1, uri, &mut item);
        put_message(3, &item, &mut body);
    }
    body
}

/// One paging request: the username, the set, the token to continue from, and
/// how many items to ask for at most.
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
#[derive(Debug)]
pub(crate) struct Item {
    pub(crate) uri: String,
    pub(crate) added_at: Option<i64>,
    pub(crate) removed: bool,
}

/// One page of a paging answer: its items, and the token that asks for the next.
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
mod tests;
