//! The playlist reorder write: its body, its reply, and what a page may change.

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
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The write was accepted but the reply named no revision to hold next.
    #[error("the reply carried no revision")]
    Reply,
}

/// `Op.kind` for a move in the playlist schema.
const MOV: u32 = 4;

/// The change set that moves one row, based on `revision`.
///
/// One `MOV` of length one: the change set and its single delta both name the
/// revision the move was based on, because a delta applies to one version.
pub fn move_body(revision: &Revision, from: usize, to: usize) -> Vec<u8> {
    let mut mov = Vec::new();
    put_varint_field(1, from as u64, &mut mov);
    put_varint_field(2, 1, &mut mov);
    put_varint_field(3, to as u64, &mut mov);

    let mut op = Vec::new();
    put_varint_field(1, u64::from(MOV), &mut op);
    put_message(4, &mov, &mut op);

    let mut delta = Vec::new();
    put_bytes(1, revision.as_bytes(), &mut delta);
    put_message(2, &op, &mut delta);

    let mut body = Vec::new();
    put_bytes(1, revision.as_bytes(), &mut body);
    put_message(2, &delta, &mut body);
    body
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

/// The changes route for a playlist, or why the URI names none.
pub fn move_endpoint(uri: &str) -> Result<String, PlaylistError> {
    let Ok(SpotifyUri::Playlist { id, .. }) = SpotifyUri::from_uri(uri) else {
        return Err(PlaylistError::NotAPlaylist);
    };
    let base = id.to_base62().map_err(|_| PlaylistError::NotAPlaylist)?;
    Ok(format!("/playlist/v2/playlist/{base}/changes"))
}

#[cfg(test)]
mod tests {
    use librespot::protocol::playlist4_external::{ListChanges, SelectedListContent, op};
    use protobuf::Message as _;

    use super::{PlaylistError, access, move_body, move_endpoint, owned, reply_revision};
    use takofy_model::{PlaylistAccess, Revision};

    /// The revision the probe wrote, 24 bytes.
    const PROBE_REV: [u8; 24] = [
        0x00, 0x00, 0x00, 0x06, 0xc1, 0x52, 0x28, 0xcf, 0xa5, 0x04, 0x98, 0xd6, 0x96, 0x0c, 0x4a,
        0x3c, 0x08, 0xe0, 0x88, 0xba, 0x39, 0xdf, 0xc8, 0x40,
    ];

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
            move_endpoint("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
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
                move_endpoint(uri),
                Err(PlaylistError::NotAPlaylist),
                "must refuse {uri:?}"
            );
        }
    }
}
