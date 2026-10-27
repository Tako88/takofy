//! The playlist writes: their bodies, their replies, and where they go.

use librespot::core::SpotifyUri;
use librespot::protocol::playlist4_external::SelectedListContent;
use protobuf::Message as _;
use takofy_model::{Move, Owner, OwnerName, PLAYLIST_NAME_MAX, PlaylistAccess, Removal, Revision};

use crate::wire::{put_bytes, put_message, put_str, put_varint_field};

/// Why a playlist could not be changed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlaylistError {
    /// The URI does not name a playlist, so there is no route to write to.
    #[error("not a playlist URI")]
    NotAPlaylist,
    /// The URI does not name a track, so there is nothing a playlist can hold.
    #[error("not a track URI")]
    NotATrack,
    /// The user is blank, so the rootlist route has no name to write to.
    #[error("no user to write the rootlist for")]
    NoUser,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The write was accepted but the reply named nothing to hold next.
    #[error("the reply named nothing to hold next")]
    Reply,
    /// The write asked for no ops, so there is nothing to send.
    #[error("no moves to write")]
    NoMoves,
    /// The write asked for no removals, so there is nothing to send.
    #[error("no rows to remove")]
    NoRemovals,
    /// The write asked for no tracks, so there is nothing to send.
    #[error("no tracks to add")]
    NoTracks,
    /// The removals do not name a chain the wire can apply in sequence.
    #[error("the removals do not form a descending, non-overlapping chain")]
    BadRemovals,
    /// The name is blank, so there is nothing to call the playlist.
    #[error("the playlist has no name")]
    NoName,
    /// The name is longer than the server takes.
    #[error("the playlist name is too long")]
    NameTooLong,
}

/// Whether `moves` names anything to write.
pub fn require_moves(moves: &[Move]) -> Result<(), PlaylistError> {
    if moves.is_empty() {
        Err(PlaylistError::NoMoves)
    } else {
        Ok(())
    }
}

/// Whether `removals` names a chain the wire applies in order.
///
/// The chain is highest index first and never overlaps, so each removal names
/// rows that the removals before it left in place.
pub fn require_removals(removals: &[Removal]) -> Result<(), PlaylistError> {
    let Some((&first, rest)) = removals.split_first() else {
        return Err(PlaylistError::NoRemovals);
    };
    let mut below = usize::MAX;
    for removal in std::iter::once(&first).chain(rest) {
        let Some(end) = removal.from.checked_add(removal.length) else {
            return Err(PlaylistError::BadRemovals);
        };
        if removal.length == 0 || end > below {
            return Err(PlaylistError::BadRemovals);
        }
        below = removal.from;
    }
    Ok(())
}

/// `Op.kind` for an add in the playlist schema.
const ADD: u32 = 2;
/// `Op.kind` for a remove in the playlist schema.
const REM: u32 = 3;
/// `Op.kind` for a move in the playlist schema.
const MOV: u32 = 4;

/// One operation of `kind`, carrying `inner` in field `field`.
fn op(kind: u32, field: u32, inner: &[u8]) -> Vec<u8> {
    let mut op = Vec::new();
    put_varint_field(1, u64::from(kind), &mut op);
    put_message(field, inner, &mut op);
    op
}

/// A change set holding `ops`, based on `revision`.
///
/// The change set and its delta both name the revision the write was based on,
/// because a delta applies to one version. A delta's ops apply in sequence.
fn changes(revision: &Revision, ops: &[Vec<u8>]) -> Vec<u8> {
    let mut delta = Vec::new();
    put_bytes(1, revision.as_bytes(), &mut delta);
    for op in ops {
        put_message(2, op, &mut delta);
    }

    let mut body = Vec::new();
    put_bytes(1, revision.as_bytes(), &mut body);
    put_message(2, &delta, &mut body);
    body
}

/// The change set that applies `moves`, based on `revision`.
///
/// Each `MOV` carries `from`, `length` and `to`, where `to` is the gap in the
/// list **before** that move, so a move downward names the gap after its own run.
pub fn move_body(revision: &Revision, moves: &[Move]) -> Vec<u8> {
    let ops: Vec<Vec<u8>> = moves
        .iter()
        .map(|mv| {
            let mut mov = Vec::new();
            put_varint_field(1, mv.from as u64, &mut mov);
            put_varint_field(2, mv.length as u64, &mut mov);
            put_varint_field(3, mv.to as u64, &mut mov);
            op(MOV, 4, &mov)
        })
        .collect();
    changes(revision, &ops)
}

/// The change set that appends `uris` at `from_index`, based on `revision`.
///
/// An append to a list the client holds in full names the list's length as its
/// index, which is where the first item goes; the items follow in the order
/// given, so one `Add` carries the whole block. Any other index inserts before
/// the item the client counted there, so the caller's index is the wire's.
pub fn add_body(revision: &Revision, from_index: usize, uris: &[String]) -> Vec<u8> {
    let mut add = Vec::new();
    put_varint_field(1, from_index as u64, &mut add);
    for uri in uris {
        let mut item = Vec::new();
        put_bytes(1, uri.as_bytes(), &mut item);
        put_message(2, &item, &mut add);
    }
    changes(revision, &[op(ADD, 2, &add)])
}

/// `Op.kind` for updating a list's attributes in the playlist schema.
const UPDATE_LIST_ATTRIBUTES: u32 = 6;

/// The field `Op` carries the attribute update in, which is the kind itself.
const UPDATE_FIELD: u32 = 6;

/// The change set that renames the list to `name`, based on `revision`.
///
/// The schema's update carries the new attributes through two wrappers, so the
/// name sits three messages deep. A create cannot name a list - the name travels
/// in the create body and the server ignores it - so this is what gives a made
/// playlist its title.
pub fn rename_body(revision: &Revision, name: &str) -> Vec<u8> {
    let mut attributes = Vec::new();
    put_str(1, name, &mut attributes);

    let mut values = Vec::new();
    put_message(1, &attributes, &mut values);

    let mut update = Vec::new();
    put_message(1, &values, &mut update);

    changes(
        revision,
        &[op(UPDATE_LIST_ATTRIBUTES, UPDATE_FIELD, &update)],
    )
}

/// The change set that applies `removals`, based on `revision`.
///
/// The ops apply in sequence, so a gapped pick is one change set of several
/// `REM`s, highest index first.
pub fn removals_body(revision: &Revision, removals: &[Removal]) -> Vec<u8> {
    let ops: Vec<Vec<u8>> = removals
        .iter()
        .map(|removal| {
            let mut rem = Vec::new();
            put_varint_field(1, removal.from as u64, &mut rem);
            put_varint_field(2, removal.length as u64, &mut rem);
            op(REM, 3, &rem)
        })
        .collect();
    changes(revision, &ops)
}

/// The change set that removes `length` items from `from_index`, based on
/// `revision`.
///
/// It undoes an add made at the same index with length one.
pub fn remove_body(revision: &Revision, from_index: usize, length: usize) -> Vec<u8> {
    removals_body(
        revision,
        &[Removal {
            from: from_index,
            length,
        }],
    )
}

/// The route that creates a playlist.
pub const CREATE_ENDPOINT: &str = "/playlist/v2/playlist";

/// `ModificationRequest.create_item_kind` for a playlist, which is what tells the
/// route to make a playlist and not a folder.
const CREATE_ITEM_KIND: u64 = 1;

/// The request that creates a playlist called `name`.
///
/// It names no base revision: a create starts a list rather than changing one,
/// so there is no version it could be based on. `playlist` is true and the item
/// kind is 1, which is what tells the route to make a playlist and not a folder.
pub fn create_body(name: &str) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(4, name, &mut body);
    put_varint_field(5, 1, &mut body);
    put_varint_field(12, CREATE_ITEM_KIND, &mut body);
    body
}

/// The new playlist's URI, from whichever field of the reply carries it.
///
/// The shipped message says field 2 and the probe measured field 1, and neither
/// is compiled, so the reply is scanned rather than parsed: the first
/// length-delimited top-level field that spells a playlist URI is the answer.
/// Anything that is not a well-formed field ends the scan.
pub fn created_uri(bytes: &[u8]) -> Option<String> {
    let mut rest = bytes;
    while let Some((key, tail)) = varint(rest) {
        let field = key >> 3;
        match key & 0x07 {
            0 => rest = varint(tail)?.1,
            1 => rest = tail.get(8..)?,
            2 => {
                let (length, body) = varint(tail)?;
                let length = usize::try_from(length).ok()?;
                let (value, after) = body.split_at_checked(length)?;
                if field != 0
                    && let Ok(uri) = std::str::from_utf8(value)
                    && require_playlist(uri).is_ok()
                {
                    return Some(uri.to_owned());
                }
                rest = after;
            }
            5 => rest = tail.get(4..)?,
            _ => return None,
        }
    }
    None
}

/// One varint and what follows it, or `None` when the bytes end mid-value.
fn varint(mut bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (byte, rest) = bytes.split_first()?;
        value |= u64::from(byte & 0x7f) << shift;
        bytes = rest;
        if byte & 0x80 == 0 {
            return Some((value, rest));
        }
    }
    None
}

/// The revision a write's answer holds next, when it names one.
pub fn reply_revision(bytes: &[u8]) -> Option<Revision> {
    let reply = SelectedListContent::parse_from_bytes(bytes).ok()?;
    Revision::from_wire(reply.revision.as_deref()?)
}

/// Whether the list's owner is the session user, spelled the same way.
///
/// An owner the wire did not name, or named blank, is not the user: there is no
/// name there to compare against the session's. The owner arrives as
/// `playlist_owner` spells it, whitespace collapsed, and is compared byte-exact.
pub fn owned(owner: Option<&str>, user: &str) -> bool {
    matches!(owner, Some(owner) if !owner.trim().is_empty() && owner == user)
}

/// The owner a playlist row draws, when the wire named one this client can show.
///
/// The session's own name is [`Owner::You`] even when it is a number: it is the
/// account signed in, not a name to filter. Another account's numeric id names
/// nobody, so it is dropped rather than drawn.
pub fn owner_of(wire: Option<&str>, user: &str) -> Option<Owner> {
    if owned(wire, user) {
        return Some(Owner::You);
    }
    wire.and_then(OwnerName::from_wire).map(Owner::Named)
}

/// What the page may change, when the read carries a revision.
///
/// Editable needs the list's permission, the user owning it, and the shown URIs
/// being the front of the list's own. A position the page dropped before its
/// last row shifts every index after it, so the page is read-only; one dropped
/// past the last row leaves every shown index equal to its wire index, so the
/// rows stay editable.
pub fn access(
    revision: Option<Revision>,
    can_edit: bool,
    owned: bool,
    items: &[String],
    shown: &[String],
) -> Option<PlaylistAccess> {
    let revision = revision?;
    Some(PlaylistAccess {
        revision,
        editable: can_edit && owned && items.starts_with(shown),
    })
}

/// What a listing read tells the sidebar about writing to a playlist.
///
/// An empty revision is no revision: a write based on it names no version of
/// the list.
pub fn listing_access(revision: &[u8], can_edit: bool) -> Option<PlaylistAccess> {
    Some(PlaylistAccess {
        revision: Revision::from_wire(revision)?,
        editable: can_edit,
    })
}

/// Whether every URI names a track a playlist can hold.
///
/// One refusal refuses the whole write: a block that would drop its episode
/// halfway is not a block the user asked for.
pub fn require_tracks(uris: &[String]) -> Result<(), PlaylistError> {
    if uris.is_empty() {
        return Err(PlaylistError::NoTracks);
    }
    for uri in uris {
        match SpotifyUri::from_uri(uri) {
            Ok(SpotifyUri::Track { .. }) => {}
            _ => return Err(PlaylistError::NotATrack),
        }
    }
    Ok(())
}

/// Whether `uri` names a playlist.
pub fn require_playlist(uri: &str) -> Result<(), PlaylistError> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Playlist { .. }) => Ok(()),
        _ => Err(PlaylistError::NotAPlaylist),
    }
}

/// Whether `name` is one the server takes.
///
/// The length is counted in characters: a name is what the user typed, and the
/// cap is on the name rather than on its encoding.
pub fn require_name(name: &str) -> Result<(), PlaylistError> {
    if name.trim().is_empty() {
        return Err(PlaylistError::NoName);
    }
    if name.chars().count() > PLAYLIST_NAME_MAX {
        return Err(PlaylistError::NameTooLong);
    }
    Ok(())
}

/// The changes route for a playlist, or why the URI names none.
pub fn changes_endpoint(uri: &str) -> Result<String, PlaylistError> {
    let Ok(SpotifyUri::Playlist { id, .. }) = SpotifyUri::from_uri(uri) else {
        return Err(PlaylistError::NotAPlaylist);
    };
    let base = id.to_base62().map_err(|_| PlaylistError::NotAPlaylist)?;
    Ok(format!("/playlist/v2/playlist/{base}/changes"))
}

/// The changes route for the user's rootlist, or why there is none.
///
/// A user that is blank once trimmed has no name to put in the route, and the
/// spelling the route keeps is the one it was given.
pub fn rootlist_changes_endpoint(user: &str) -> Result<String, PlaylistError> {
    if user.trim().is_empty() {
        return Err(PlaylistError::NoUser);
    }
    Ok(format!("/playlist/v2/user/{user}/rootlist/changes"))
}

#[cfg(test)]
mod tests;
