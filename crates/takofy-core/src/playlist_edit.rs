//! The playlist writes: their bodies, their replies, and where they go.

use librespot::core::SpotifyUri;
use librespot::protocol::playlist4_external::SelectedListContent;
use protobuf::Message as _;
use takofy_model::{PlaylistAccess, Revision};

use crate::wire::{put_bytes, put_message, put_varint_field};

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
    /// The write was accepted but the reply named no revision to hold next.
    #[error("the reply carried no revision")]
    Reply,
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

/// A change set holding one operation, based on `revision`.
///
/// The change set and its single delta both name the revision the write was
/// based on, because a delta applies to one version.
fn changes(revision: &Revision, op: &[u8]) -> Vec<u8> {
    let mut delta = Vec::new();
    put_bytes(1, revision.as_bytes(), &mut delta);
    put_message(2, op, &mut delta);

    let mut body = Vec::new();
    put_bytes(1, revision.as_bytes(), &mut body);
    put_message(2, &delta, &mut body);
    body
}

/// The change set that moves one row, based on `revision`.
///
/// One `MOV` of length one: `to` is the gap in the list **before** the move, so
/// a move downward names the gap after its own row.
pub fn move_body(revision: &Revision, from: usize, to: usize) -> Vec<u8> {
    let mut mov = Vec::new();
    put_varint_field(1, from as u64, &mut mov);
    put_varint_field(2, 1, &mut mov);
    put_varint_field(3, to as u64, &mut mov);
    changes(revision, &op(MOV, 4, &mov))
}

/// The change set that appends one item, based on `revision`.
///
/// An append names the list's length as its index, which is where the item goes
/// in a list it already holds in full.
pub fn add_body(revision: &Revision, from_index: usize, uri: &str) -> Vec<u8> {
    let mut item = Vec::new();
    put_bytes(1, uri.as_bytes(), &mut item);

    let mut add = Vec::new();
    put_varint_field(1, from_index as u64, &mut add);
    put_message(2, &item, &mut add);
    changes(revision, &op(ADD, 2, &add))
}

/// The change set that removes `length` items from `from_index`, based on
/// `revision`.
///
/// It undoes an add made at the same index with length one.
pub fn remove_body(revision: &Revision, from_index: usize, length: usize) -> Vec<u8> {
    let mut rem = Vec::new();
    put_varint_field(1, from_index as u64, &mut rem);
    put_varint_field(2, length as u64, &mut rem);
    changes(revision, &op(REM, 3, &rem))
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

/// Whether `uri` names a track a playlist can hold.
pub fn require_track(uri: &str) -> Result<(), PlaylistError> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Track { .. }) => Ok(()),
        _ => Err(PlaylistError::NotATrack),
    }
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
mod tests {
    use librespot::protocol::playlist4_external::{ListChanges, SelectedListContent, op};
    use protobuf::Message as _;

    use super::{
        PlaylistError, access, add_body, changes_endpoint, listing_access, move_body, owned,
        remove_body, reply_revision, require_track, rootlist_changes_endpoint,
    };
    use takofy_model::{PlaylistAccess, Revision};

    /// The revision the probe wrote, 24 bytes.
    const PROBE_REV: [u8; 24] = [
        0x00, 0x00, 0x00, 0x06, 0xc1, 0x52, 0x28, 0xcf, 0xa5, 0x04, 0x98, 0xd6, 0x96, 0x0c, 0x4a,
        0x3c, 0x08, 0xe0, 0x88, 0xba, 0x39, 0xdf, 0xc8, 0x40,
    ];

    /// The track the probe appended, 36 bytes of URI.
    const PROBE_TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

    fn probe() -> Revision {
        Revision::from_wire(&PROBE_REV).expect("the probe's revision")
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn page(bytes: &[u8]) -> ListChanges {
        ListChanges::parse_from_bytes(bytes).expect("a change set")
    }

    #[test]
    fn a_move_body_is_the_probes_66_bytes() {
        let body = move_body(&probe(), 0, 1);
        let mut expected = vec![
            0x0a, 0x18, // ListChanges.base_revision: 24 bytes
        ];
        expected.extend_from_slice(&PROBE_REV);
        expected.extend_from_slice(&[
            0x12, 0x26, // ListChanges.deltas[0]: 38 bytes
            0x0a, 0x18, // Delta.base_version: 24 bytes
        ]);
        expected.extend_from_slice(&PROBE_REV);
        expected.extend_from_slice(&[
            0x12, 0x0a, // Delta.ops[0]: 10 bytes
            0x08, 0x04, // Op.kind = MOV
            0x22, 0x06, // Op.mov: 6 bytes
            0x08, 0x00, // Mov.from_index = 0
            0x10, 0x01, // Mov.length = 1
            0x18, 0x01, // Mov.to_index = 1
        ]);
        assert_eq!(body, expected);
        assert_eq!(body.len(), 66);
    }

    #[test]
    fn a_20_byte_revision_writes_58_bytes() {
        let revision = Revision::from_wire(&[0xa5; 20]).expect("a revision");
        let body = move_body(&revision, 0, 1);
        // Mov: three varint fields, two bytes each = 6.
        // Op: kind (2) + mov tag and length (2) + Mov (6) = 10.
        // Delta: base tag, length and 20 bytes (22) + op tag and length (2) + Op (10) = 34.
        // ListChanges: base tag, length and 20 bytes (22) + delta tag and length (2)
        // + Delta (34) = 58.
        assert_eq!(body[1], 0x14, "20 bytes");
        assert_eq!(body[2 + 20 + 1], 0x22, "the delta is 34 bytes");
        assert_eq!(body.len(), 58);
    }

    #[test]
    fn the_delta_and_the_change_set_carry_the_same_base() {
        let read = page(&move_body(&probe(), 2, 4));
        assert_eq!(read.base_revision(), &PROBE_REV[..]);
        assert_eq!(read.deltas.len(), 1);
        assert_eq!(read.deltas[0].base_version(), &PROBE_REV[..]);
        assert_eq!(read.deltas[0].ops.len(), 1);
        assert_eq!(read.deltas[0].ops[0].kind(), op::Kind::MOV);
    }

    #[test]
    fn a_far_move_writes_multi_byte_indices() {
        let body = move_body(&probe(), 200, 3);
        assert!(
            body.ends_with(&[0x08, 0xc8, 0x01, 0x10, 0x01, 0x18, 0x03]),
            "200 is two bytes"
        );
        assert_eq!(body.len(), 67);
        let read = page(&body);
        let mov = read.deltas[0].ops[0].mov.as_ref().expect("a move");
        assert_eq!(
            (mov.from_index(), mov.length(), mov.to_index()),
            (200, 1, 3)
        );

        let deep = move_body(&probe(), 3, 300);
        assert!(deep.ends_with(&[0x08, 0x03, 0x10, 0x01, 0x18, 0xac, 0x02]));
        let read = page(&deep);
        let mov = read.deltas[0].ops[0].mov.as_ref().expect("a move");
        assert_eq!(
            (mov.from_index(), mov.length(), mov.to_index()),
            (3, 1, 300)
        );
    }

    #[test]
    fn an_add_body_is_102_bytes_for_a_24_byte_revision() {
        let body = add_body(&probe(), 23, PROBE_TRACK);
        let mut expected = vec![
            0x0a, 0x18, // ListChanges.base_revision: 24 bytes
        ];
        expected.extend_from_slice(&PROBE_REV);
        expected.extend_from_slice(&[
            0x12, 0x4a, // ListChanges.deltas[0]: 74 bytes
            0x0a, 0x18, // Delta.base_version: 24 bytes
        ]);
        expected.extend_from_slice(&PROBE_REV);
        expected.extend_from_slice(&[
            0x12, 0x2e, // Delta.ops[0]: 46 bytes
            0x08, 0x02, // Op.kind = ADD
            0x12, 0x2a, // Op.add: 42 bytes
            0x08, 0x17, // Add.from_index = 23
            0x12, 0x26, // Add.items[0]: 38 bytes
            0x0a, 0x24, // Item.uri: 36 bytes
        ]);
        expected.extend_from_slice(PROBE_TRACK.as_bytes());

        // Item = 2 + 36 = 38
        // Add = 2 + (2 + 38) = 42
        // Op = 2 + (2 + 42) = 46
        // Delta = (2 + 24) + (2 + 46) = 74
        // ListChanges = (2 + 24) + (2 + 74) = 102
        assert_eq!(body, expected);
        assert_eq!(body.len(), 102);
    }

    #[test]
    fn an_add_at_a_far_index_widens_every_length() {
        let body = add_body(&probe(), 200, PROBE_TRACK);
        // 200 is two varint bytes, so the add is one longer at every level.
        assert!(
            body.windows(6)
                .any(|run| run == [0x12, 0x2b, 0x08, 0xc8, 0x01, 0x12]),
            "the add and its item keep their own tags"
        );
        assert_eq!(body.len(), 103);
        let read = page(&body);
        let add = read.deltas[0].ops[0].add.as_ref().expect("an add");
        assert_eq!(add.from_index(), 200);
    }

    #[test]
    fn an_add_on_a_20_byte_revision_writes_94_bytes() {
        let revision = Revision::from_wire(&[0xa5; 20]).expect("a revision");
        let body = add_body(&revision, 23, PROBE_TRACK);
        // ListChanges base: 2 + 20 = 22. Delta: 22 + (2 + 46) = 70, which is 0x46.
        assert_eq!(body[1], 0x14, "20 bytes");
        assert_eq!(body[22], 0x12, "the deltas field starts here");
        assert_eq!(body[23], 0x46, "the delta is 70 bytes");
        assert_eq!(body.len(), 94);
    }

    #[test]
    fn an_add_parses_back_as_one_add_of_one_item() {
        let read = page(&add_body(&probe(), 23, PROBE_TRACK));
        assert_eq!(read.base_revision(), &PROBE_REV[..]);
        assert_eq!(read.deltas.len(), 1);
        assert_eq!(read.deltas[0].base_version(), &PROBE_REV[..]);
        assert_eq!(read.deltas[0].ops.len(), 1);

        let found = &read.deltas[0].ops[0];
        assert_eq!(found.kind(), op::Kind::ADD);
        let add = found.add.as_ref().expect("an add");
        assert_eq!(add.from_index(), 23);
        assert_eq!(add.items.len(), 1);
        assert_eq!(add.items[0].uri(), PROBE_TRACK);
        assert!(found.mov.is_none(), "an add is not a move");
        assert!(found.rem.is_none(), "an add is not a remove");
    }

    #[test]
    fn a_remove_body_is_64_bytes() {
        let body = remove_body(&probe(), 23, 1);
        let mut expected = vec![
            0x0a, 0x18, // ListChanges.base_revision: 24 bytes
        ];
        expected.extend_from_slice(&PROBE_REV);
        expected.extend_from_slice(&[
            0x12, 0x24, // ListChanges.deltas[0]: 36 bytes
            0x0a, 0x18, // Delta.base_version: 24 bytes
        ]);
        expected.extend_from_slice(&PROBE_REV);
        expected.extend_from_slice(&[
            0x12, 0x08, // Delta.ops[0]: 8 bytes
            0x08, 0x03, // Op.kind = REM
            0x1a, 0x04, // Op.rem: 4 bytes
            0x08, 0x17, // Rem.from_index = 23
            0x10, 0x01, // Rem.length = 1
        ]);

        // Rem = 2 + 2 = 4; Op = 2 + 2 + 4 = 8; Delta = 26 + 2 + 8 = 36;
        // ListChanges = 26 + 2 + 36 = 64.
        assert_eq!(body, expected);
        assert_eq!(body.len(), 64);
    }

    #[test]
    fn a_remove_parses_back_as_one_rem() {
        let read = page(&remove_body(&probe(), 23, 1));
        let found = &read.deltas[0].ops[0];
        assert_eq!(found.kind(), op::Kind::REM);
        let rem = found.rem.as_ref().expect("a remove");
        assert_eq!((rem.from_index(), rem.length()), (23, 1));
    }

    #[test]
    fn an_add_and_its_remove_name_the_same_index() {
        let added = page(&add_body(&probe(), 9, PROBE_TRACK));
        let removed = page(&remove_body(&probe(), 9, 1));
        let add = added.deltas[0].ops[0].add.as_ref().expect("an add");
        let rem = removed.deltas[0].ops[0].rem.as_ref().expect("a remove");
        assert_eq!(add.from_index(), rem.from_index());
        assert_eq!(rem.length(), 1);
    }

    #[test]
    fn a_reply_names_its_new_revision() {
        let mut reply = SelectedListContent::new();
        reply.revision = Some(vec![0x0a, 0x0b]);
        let bytes = reply.write_to_bytes().expect("a reply");
        assert_eq!(reply_revision(&bytes), Revision::from_wire(&[0x0a, 0x0b]));
    }

    #[test]
    fn a_reply_without_a_revision_is_none() {
        let empty = SelectedListContent::new()
            .write_to_bytes()
            .expect("an empty reply");
        assert_eq!(reply_revision(&empty), None);

        let mut blank = SelectedListContent::new();
        blank.revision = Some(Vec::new());
        let bytes = blank.write_to_bytes().expect("a reply");
        assert_eq!(reply_revision(&bytes), None);
    }

    #[test]
    fn garbage_is_no_reply() {
        assert_eq!(reply_revision(&[0xff]), None);
        assert_eq!(reply_revision(&[0x0a, 0x05, 0x01]), None);
        assert_eq!(reply_revision(&[]), None);
    }

    #[test]
    fn a_prefix_of_the_items_is_editable() {
        let items = strings(&["a", "b", "c"]);
        let first = access(Some(probe()), true, true, &items, &strings(&["a"]));
        assert_eq!(
            first,
            Some(PlaylistAccess {
                revision: probe(),
                editable: true,
            })
        );
        let whole = access(Some(probe()), true, true, &items, &items);
        assert_eq!(
            whole,
            Some(PlaylistAccess {
                revision: probe(),
                editable: true,
            })
        );
    }

    #[test]
    fn an_unowned_or_uneditable_list_is_not() {
        let items = strings(&["a", "b"]);
        for (can_edit, is_owned) in [(false, true), (true, false), (false, false)] {
            let read = access(Some(probe()), can_edit, is_owned, &items, &items).expect("a page");
            assert!(!read.editable, "can_edit={can_edit} owned={is_owned}");
        }
    }

    #[test]
    fn a_position_dropped_before_the_page_ends_makes_the_rows_read_only() {
        let items = strings(&["a", "skipped", "b"]);
        let shown = strings(&["a", "b"]);
        let read = access(Some(probe()), true, true, &items, &shown).expect("a page");
        assert!(
            !read.editable,
            "the page's second row is wire index two, not one"
        );
    }

    #[test]
    fn a_position_dropped_past_the_page_leaves_the_rows_editable() {
        let items = strings(&["a", "b", "skipped"]);
        let shown = strings(&["a", "b"]);
        let read = access(Some(probe()), true, true, &items, &shown).expect("a page");
        assert!(read.editable, "every shown index is still its wire index");
    }

    #[test]
    fn more_rows_than_items_is_read_only() {
        let items = strings(&["a"]);
        let shown = strings(&["a", "b"]);
        let read = access(Some(probe()), true, true, &items, &shown).expect("a page");
        assert!(!read.editable);
    }

    #[test]
    fn an_empty_revision_has_no_access() {
        let items = strings(&["a"]);
        assert_eq!(access(None, true, true, &items, &items), None);
    }

    #[test]
    fn an_empty_page_is_editable() {
        let none: &[String] = &[];
        let read = access(Some(probe()), true, true, &strings(&["a"]), none).expect("a page");
        assert!(read.editable);
    }

    #[test]
    fn the_owner_must_be_the_session_user_exactly() {
        assert!(owned(Some("alice"), "alice"));
        assert!(!owned(Some("Alice"), "alice"));
        assert!(!owned(Some("alice"), "Alice"));
        assert!(!owned(Some("alice "), "alice"));
        assert!(!owned(Some("alice"), "alice "));
        assert!(!owned(Some("alicee"), "alice"));
        assert!(!owned(None, "alice"));
        assert!(!owned(Some(""), ""));
        assert!(!owned(Some("   "), "   "));
    }

    #[test]
    fn a_playlist_uri_names_its_changes_route() {
        assert_eq!(
            changes_endpoint("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            Ok("/playlist/v2/playlist/37i9dQZF1DXcBWIGoYBM5M/changes".to_owned())
        );
    }

    #[test]
    fn an_album_uri_is_not_a_playlist() {
        for uri in [
            "spotify:album:4uLU6hMCjMI75M1A2tKUQC",
            "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            "spotify:show:6rqhFgbbKwnb9MLmUQDhG6",
            "not a uri",
            "",
            "spotify:playlist:not-base62!",
        ] {
            assert_eq!(
                changes_endpoint(uri),
                Err(PlaylistError::NotAPlaylist),
                "must refuse {uri:?}"
            );
        }
    }

    #[test]
    fn a_rootlist_names_its_changes_route() {
        assert_eq!(
            rootlist_changes_endpoint("alice"),
            Ok("/playlist/v2/user/alice/rootlist/changes".to_owned())
        );
        for blank in ["", "  ", "\t"] {
            assert_eq!(
                rootlist_changes_endpoint(blank),
                Err(PlaylistError::NoUser),
                "must refuse {blank:?}"
            );
        }
    }

    #[test]
    fn only_a_track_uri_may_be_added() {
        assert_eq!(require_track(PROBE_TRACK), Ok(()));
        for uri in [
            "spotify:episode:1",
            "spotify:album:4uLU6hMCjMI75M1A2tKUQC",
            "spotify:local:a:b:c:1",
            "not a uri",
            "",
            "spotify:track:not-base62!",
        ] {
            assert_eq!(
                require_track(uri),
                Err(PlaylistError::NotATrack),
                "must refuse {uri:?}"
            );
        }
    }

    #[test]
    fn a_listing_names_its_revision_and_permission() {
        assert_eq!(
            listing_access(&PROBE_REV, true),
            Some(PlaylistAccess {
                revision: probe(),
                editable: true,
            })
        );
        assert_eq!(
            listing_access(&PROBE_REV, false),
            Some(PlaylistAccess {
                revision: probe(),
                editable: false,
            })
        );
        assert_eq!(listing_access(&[], true), None, "no revision, no access");
    }
}
