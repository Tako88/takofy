use librespot::protocol::playlist4_external::{ListChanges, SelectedListContent, op};
use protobuf::Message as _;

use super::{
    PlaylistError, access, add_body, changes_endpoint, create_body, created_uri, listing_access,
    move_body, owned, owner_of, removals_body, remove_body, rename_body, reply_revision,
    require_moves, require_name, require_playlist, require_removals, require_tracks,
    rootlist_changes_endpoint, varint,
};
use takofy_model::{Move, Owner, OwnerName, PLAYLIST_NAME_MAX, PlaylistAccess, Removal, Revision};

/// The revision the probe wrote, 24 bytes.
const PROBE_REV: [u8; 24] = [
    0x00, 0x00, 0x00, 0x06, 0xc1, 0x52, 0x28, 0xcf, 0xa5, 0x04, 0x98, 0xd6, 0x96, 0x0c, 0x4a, 0x3c,
    0x08, 0xe0, 0x88, 0xba, 0x39, 0xdf, 0xc8, 0x40,
];

/// The track the probe appended, 36 bytes of URI.
const PROBE_TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

/// Two more tracks, the same 36 bytes, so a block's arithmetic is only
/// about its length and its order.
const SECOND_TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQD";
const THIRD_TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQE";

/// A created playlist's URI, the 39 bytes the probe measured.
const NEW_PLAYLIST: &str = "spotify:playlist:0JQ5DAqbMKFEC4WFtoNRpw";

fn probe() -> Revision {
    Revision::from_wire(&PROBE_REV).expect("the probe's revision")
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn page(bytes: &[u8]) -> ListChanges {
    ListChanges::parse_from_bytes(bytes).expect("a change set")
}

fn removal(from: usize, length: usize) -> Removal {
    Removal { from, length }
}

fn one(from: usize, to: usize) -> [Move; 1] {
    [Move {
        from,
        length: 1,
        to,
    }]
}

#[test]
fn an_empty_move_list_is_no_write() {
    assert_eq!(require_moves(&[]), Err(PlaylistError::NoMoves));
    assert_eq!(require_moves(&one(0, 1)), Ok(()));
}

#[test]
fn a_move_body_is_the_probes_66_bytes() {
    let body = move_body(&probe(), &one(0, 1));
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
    let body = move_body(&revision, &one(0, 1));
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
    let read = page(&move_body(&probe(), &one(2, 4)));
    assert_eq!(read.base_revision(), &PROBE_REV[..]);
    assert_eq!(read.deltas.len(), 1);
    assert_eq!(read.deltas[0].base_version(), &PROBE_REV[..]);
    assert_eq!(read.deltas[0].ops.len(), 1);
    assert_eq!(read.deltas[0].ops[0].kind(), op::Kind::MOV);

    let chain = [
        Move {
            from: 0,
            length: 1,
            to: 4,
        },
        Move {
            from: 1,
            length: 1,
            to: 4,
        },
    ];
    let read = page(&move_body(&probe(), &chain));
    assert_eq!(read.base_revision(), &PROBE_REV[..]);
    assert_eq!(read.deltas.len(), 1);
    assert_eq!(read.deltas[0].base_version(), &PROBE_REV[..]);
    assert_eq!(read.deltas[0].ops.len(), 2);
}

#[test]
fn a_contiguous_block_is_one_mov_of_its_length() {
    let block = Move {
        from: 1,
        length: 3,
        to: 6,
    };
    let body = move_body(&probe(), &[block]);
    assert!(
        body.ends_with(&[0x08, 0x01, 0x10, 0x03, 0x18, 0x06]),
        "one MOV naming from 1, length 3, to 6"
    );
    let read = page(&body);
    assert_eq!(read.deltas[0].ops.len(), 1);
    let mov = read.deltas[0].ops[0].mov.as_ref().expect("a move");
    assert_eq!((mov.from_index(), mov.length(), mov.to_index()), (1, 3, 6));
}

#[test]
fn a_chain_is_two_ops_in_one_delta() {
    let chain = [
        Move {
            from: 0,
            length: 1,
            to: 4,
        },
        Move {
            from: 1,
            length: 1,
            to: 4,
        },
    ];
    let body = move_body(&probe(), &chain);
    let read = page(&body);
    assert_eq!(read.deltas.len(), 1, "one delta carries both ops");
    let ops = &read.deltas[0].ops;
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].kind(), op::Kind::MOV);
    assert_eq!(ops[1].kind(), op::Kind::MOV);
    let first = ops[0].mov.as_ref().expect("a move");
    assert_eq!(
        (first.from_index(), first.length(), first.to_index()),
        (0, 1, 4)
    );
    let second = ops[1].mov.as_ref().expect("a move");
    assert_eq!(
        (second.from_index(), second.length(), second.to_index()),
        (1, 1, 4)
    );

    // Mov = 2 + 2 + 2 = 6; Op = 2 + (2 + 6) = 10 each.
    // Delta = (2 + 24) + 2 * (2 + 10) = 50; ListChanges = (2 + 24) + (2 + 50) = 78.
    assert_eq!(body.len(), 78);
}

#[test]
fn a_far_move_writes_multi_byte_indices() {
    let body = move_body(&probe(), &one(200, 3));
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

    let deep = move_body(&probe(), &one(3, 300));
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
    let body = add_body(&probe(), 23, &strings(&[PROBE_TRACK]));
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
    let body = add_body(&probe(), 200, &strings(&[PROBE_TRACK]));
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
    let body = add_body(&revision, 23, &strings(&[PROBE_TRACK]));
    // ListChanges base: 2 + 20 = 22. Delta: 22 + (2 + 46) = 70, which is 0x46.
    assert_eq!(body[1], 0x14, "20 bytes");
    assert_eq!(body[22], 0x12, "the deltas field starts here");
    assert_eq!(body[23], 0x46, "the delta is 70 bytes");
    assert_eq!(body.len(), 94);
}

#[test]
fn an_add_parses_back_as_one_add_of_one_item() {
    let read = page(&add_body(&probe(), 23, &strings(&[PROBE_TRACK])));
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
    let added = page(&add_body(&probe(), 9, &strings(&[PROBE_TRACK])));
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
fn the_session_user_is_you_even_when_the_name_is_a_number() {
    assert_eq!(
        owner_of(Some("31415926535"), "31415926535"),
        Some(Owner::You)
    );
}

#[test]
fn another_users_number_is_no_owner() {
    assert_eq!(owner_of(Some("31415926535"), "alice"), None);
}

#[test]
fn another_users_name_is_named() {
    assert_eq!(
        owner_of(Some("Vela Moreno"), "alice"),
        Some(Owner::Named(
            OwnerName::from_wire("Vela Moreno").expect("a name")
        ))
    );
}

#[test]
fn a_blank_or_missing_owner_is_none() {
    assert_eq!(owner_of(Some(""), "alice"), None);
    assert_eq!(owner_of(Some("   "), "alice"), None);
    assert_eq!(owner_of(None, "alice"), None);
}

#[test]
fn a_collapsed_owner_is_not_you() {
    let owner = owner_of(Some("alice  smith"), "alice smith").expect("an owner");
    assert_ne!(owner, Owner::You);
    assert_eq!(owner.name(), Some("alice  smith"));
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
    assert_eq!(require_tracks(&strings(&[PROBE_TRACK])), Ok(()));
    for uri in [
        "spotify:episode:1",
        "spotify:album:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:local:a:b:c:1",
        "not a uri",
        "",
        "spotify:track:not-base62!",
    ] {
        assert_eq!(
            require_tracks(&[uri.to_owned()]),
            Err(PlaylistError::NotATrack),
            "must refuse {uri:?}"
        );
    }
}

#[test]
fn an_empty_track_slice_is_no_write() {
    assert_eq!(require_tracks(&[]), Err(PlaylistError::NoTracks));
}

#[test]
fn one_episode_refuses_the_whole_slice() {
    assert_eq!(
        require_tracks(&strings(&[PROBE_TRACK, "spotify:episode:1"])),
        Err(PlaylistError::NotATrack)
    );
    assert_eq!(
        require_tracks(&strings(&["spotify:episode:1", PROBE_TRACK])),
        Err(PlaylistError::NotATrack)
    );
    assert_eq!(
        require_tracks(&strings(&[PROBE_TRACK, SECOND_TRACK, THIRD_TRACK])),
        Ok(())
    );
}

#[test]
fn require_removals_rejects_empty_zero_length_ascending_and_overlapping_lists() {
    assert_eq!(require_removals(&[]), Err(PlaylistError::NoRemovals));
    assert_eq!(
        require_removals(&[removal(4, 0)]),
        Err(PlaylistError::BadRemovals)
    );
    assert_eq!(
        require_removals(&[removal(1, 1), removal(4, 1)]),
        Err(PlaylistError::BadRemovals),
        "an ascending chain names rows the last removal moved"
    );
    assert_eq!(
        require_removals(&[removal(4, 3), removal(3, 2)]),
        Err(PlaylistError::BadRemovals),
        "the runs overlap"
    );
    assert_eq!(
        require_removals(&[removal(4, 3), removal(4, 1)]),
        Err(PlaylistError::BadRemovals),
        "two runs start at the same row"
    );
}

#[test]
fn require_removals_accepts_a_descending_chain() {
    assert_eq!(
        require_removals(&[removal(5, 1), removal(3, 1), removal(1, 1)]),
        Ok(())
    );
    assert_eq!(
        require_removals(&[removal(6, 3), removal(2, 2), removal(0, 1)]),
        Ok(()),
        "runs may touch as long as they do not overlap"
    );
}

#[test]
fn a_removal_that_overflows_is_rejected() {
    assert_eq!(
        require_removals(&[removal(usize::MAX, 2)]),
        Err(PlaylistError::BadRemovals)
    );
    assert_eq!(
        require_removals(&[removal(usize::MAX, usize::MAX)]),
        Err(PlaylistError::BadRemovals)
    );
}

#[test]
fn removals_body_is_one_delta_with_one_op_per_removal_in_order() {
    let chain = [removal(5, 1), removal(3, 1), removal(1, 1)];
    let body = removals_body(&probe(), &chain);
    let read = page(&body);
    assert_eq!(read.base_revision(), &PROBE_REV[..]);
    assert_eq!(read.deltas.len(), 1, "one delta carries the chain");
    assert_eq!(read.deltas[0].base_version(), &PROBE_REV[..]);
    assert_eq!(read.deltas[0].ops.len(), 3);
    for (found, removal) in read.deltas[0].ops.iter().zip(&chain) {
        assert_eq!(found.kind(), op::Kind::REM);
        let rem = found.rem.as_ref().expect("a remove");
        assert_eq!(
            (rem.from_index() as usize, rem.length() as usize),
            (removal.from, removal.length),
            "the ops keep the descending order they were given"
        );
        assert!(found.mov.is_none(), "a removal is not a move");
    }

    assert_eq!(
        removals_body(&probe(), &[removal(23, 1)]),
        remove_body(&probe(), 23, 1),
        "one run is the pinned single-removal body"
    );
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

/// Every top-level field of a hand-encoded body: its number and its payload,
/// with a varint payload as its little-endian bytes.
fn fields(bytes: &[u8]) -> Vec<(u64, Vec<u8>)> {
    let mut found = Vec::new();
    let mut rest = bytes;
    while let Some((key, tail)) = varint(rest) {
        let (payload, after) = match key & 0x07 {
            0 => {
                let (value, after) = varint(tail).expect("a varint value");
                (value.to_le_bytes().to_vec(), after)
            }
            2 => {
                let (length, body) = varint(tail).expect("a length");
                let (value, after) = body
                    .split_at_checked(length as usize)
                    .expect("a whole field");
                (value.to_vec(), after)
            }
            other => panic!("wire type {other} is not in this body"),
        };
        found.push((key >> 3, payload));
        rest = after;
    }
    assert!(rest.is_empty(), "the body ended mid-field");
    found
}

#[test]
fn a_create_body_for_mix_is_names_playlist_and_kind() {
    assert_eq!(
        create_body("Mix"),
        vec![
            0x22, 0x03, 0x4d, 0x69, 0x78, // ModificationRequest.name = "Mix"
            0x28, 0x01, // ModificationRequest.playlist = true
            0x60, 0x01, // ModificationRequest.create_item_kind = 1
        ]
    );
}

#[test]
fn a_create_body_for_a_multibyte_name_writes_utf8_length() {
    // "Måne" is four characters and five bytes, so the field counts bytes.
    assert_eq!(
        create_body("Måne"),
        vec![
            0x22, 0x05, 0x4d, 0xc3, 0xa5, 0x6e, 0x65, 0x28, 0x01, 0x60, 0x01
        ]
    );
}

#[test]
fn a_create_body_for_a_100_char_name_keeps_a_one_byte_length() {
    let name = "a".repeat(PLAYLIST_NAME_MAX);
    let body = create_body(&name);
    assert_eq!(&body[..2], &[0x22, 0x64], "100 bytes, still one varint");
    assert_eq!(&body[2..102], name.as_bytes());
    assert_eq!(&body[102..], &[0x28, 0x01, 0x60, 0x01]);
}

#[test]
fn create_body_parses_back_as_name_playlist_true_and_kind_one() {
    let found = fields(&create_body("Mix"));
    assert_eq!(found.len(), 3, "a name, the playlist flag and the kind");
    assert_eq!(found[0], (4, b"Mix".to_vec()));
    assert_eq!(found[1], (5, 1u64.to_le_bytes().to_vec()));
    assert_eq!(found[2], (12, 1u64.to_le_bytes().to_vec()));
}

#[test]
fn a_rename_body_is_69_bytes_for_a_24_byte_revision() {
    let body = rename_body(&probe(), "Mix");
    let mut expected = vec![
        0x0a, 0x18, // ListChanges.base_revision: 24 bytes
    ];
    expected.extend_from_slice(&PROBE_REV);
    expected.extend_from_slice(&[
        0x12, 0x29, // ListChanges.deltas[0]: 41 bytes
        0x0a, 0x18, // Delta.base_version: 24 bytes
    ]);
    expected.extend_from_slice(&PROBE_REV);
    expected.extend_from_slice(&[
        0x12, 0x0d, // Delta.ops[0]: 13 bytes
        0x08, 0x06, // Op.kind = UPDATE_LIST_ATTRIBUTES
        0x32, 0x09, // Op.update_list_attributes: 9 bytes
        0x0a, 0x07, // UpdateListAttributes.new_attributes: 7 bytes
        0x0a, 0x05, // ListPartial.values: 5 bytes
        0x0a, 0x03, // ListAttributes.name: 3 bytes
    ]);
    expected.extend_from_slice(b"Mix");
    assert_eq!(body, expected);
    assert_eq!(body.len(), 69);
}

#[test]
fn a_rename_parses_back_as_one_update_of_the_name() {
    let changes = page(&rename_body(&probe(), "Night Drive"));
    assert_eq!(changes.deltas.len(), 1);
    let delta = &changes.deltas[0];
    assert_eq!(delta.base_version, Some(PROBE_REV.to_vec()));
    assert_eq!(delta.ops.len(), 1, "a rename is one op");
    let op = &delta.ops[0];
    assert_eq!(op.kind(), op::Kind::UPDATE_LIST_ATTRIBUTES);
    let update = op.update_list_attributes.as_ref().expect("an update");
    let name = update
        .new_attributes
        .as_ref()
        .and_then(|partial| partial.values.as_ref())
        .and_then(|attributes| attributes.name.as_ref());
    assert_eq!(name.map(String::as_str), Some("Night Drive"));
}

#[test]
fn a_rename_and_the_add_that_follows_name_the_same_revision() {
    // What the create chain does: the rename answers the revision the add
    // is based on, so both changes name the version they follow.
    let next = Revision::from_wire(&PROBE_REV).expect("a revision");
    let rename = page(&rename_body(&probe(), "Mix"));
    let add = page(&add_body(&next, 0, &[PROBE_TRACK.to_owned()]));
    assert_eq!(rename.base_revision, Some(PROBE_REV.to_vec()));
    assert_eq!(add.base_revision, Some(PROBE_REV.to_vec()));
}

#[test]
fn require_name_rejects_blank_and_over_100_chars() {
    for blank in ["", " ", "   ", "\t\n"] {
        assert_eq!(require_name(blank), Err(PlaylistError::NoName), "{blank:?}");
    }
    assert_eq!(require_name("Mix"), Ok(()));
    assert_eq!(require_name(&"a".repeat(PLAYLIST_NAME_MAX)), Ok(()));
    assert_eq!(
        require_name(&"a".repeat(PLAYLIST_NAME_MAX + 1)),
        Err(PlaylistError::NameTooLong)
    );
    // The cap counts characters: 100 two-byte characters is 200 bytes and
    // still a name the server takes.
    assert_eq!(require_name(&"å".repeat(PLAYLIST_NAME_MAX)), Ok(()));
    assert_eq!(
        require_name(&"å".repeat(PLAYLIST_NAME_MAX + 1)),
        Err(PlaylistError::NameTooLong)
    );
}

/// One length-delimited field holding `uri`.
fn uri_field(field: u8, uri: &str) -> Vec<u8> {
    let mut body = vec![(field << 3) | 2, uri.len() as u8];
    body.extend_from_slice(uri.as_bytes());
    body
}

#[test]
fn created_uri_reads_field_one_or_field_two() {
    for field in [1, 2] {
        assert_eq!(
            created_uri(&uri_field(field, NEW_PLAYLIST)),
            Some(NEW_PLAYLIST.to_owned()),
            "field {field}"
        );
    }
}

#[test]
fn created_uri_reads_a_39_byte_uri() {
    assert_eq!(NEW_PLAYLIST.len(), 39, "the probe's length");
    let body = uri_field(1, NEW_PLAYLIST);
    assert_eq!(body[1], 39);
    assert_eq!(created_uri(&body), Some(NEW_PLAYLIST.to_owned()));
}

#[test]
fn created_uri_skips_unknown_fields_before_and_after() {
    let mut body = vec![
        0x08, 0x01, // field 1, a varint
        0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // field 3, 64-bit
        0x25, 0x00, 0x00, 0x00, 0x00, // field 4, 32-bit
        0x32, 0x02, 0x68, 0x69, // field 6, the string "hi"
    ];
    body.extend_from_slice(&uri_field(2, NEW_PLAYLIST));
    body.extend_from_slice(&[0x38, 0x01]); // field 7, a varint after it
    assert_eq!(created_uri(&body), Some(NEW_PLAYLIST.to_owned()));
}

#[test]
fn created_uri_rejects_a_non_playlist_uri_a_truncated_length_and_garbage() {
    assert_eq!(created_uri(&uri_field(1, PROBE_TRACK)), None);
    assert_eq!(
        created_uri(&uri_field(1, "spotify:album:4uLU6hMCjMI75M1A2tKUQC")),
        None
    );
    assert_eq!(
        created_uri(&[0x0a, 0x27, 0x73, 0x70]),
        None,
        "a short field"
    );
    assert_eq!(created_uri(&[0x0a, 0x05, 0x68]), None);
    assert_eq!(created_uri(&[0xff]), None);
    assert_eq!(created_uri(&[0x1f]), None, "a group has no place here");
    assert_eq!(created_uri(&[]), None);
}

#[test]
fn require_playlist_accepts_a_playlist_and_rejects_track_artist_garbage() {
    assert_eq!(require_playlist(NEW_PLAYLIST), Ok(()));
    for uri in [
        PROBE_TRACK,
        "spotify:artist:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:album:4uLU6hMCjMI75M1A2tKUQC",
        "not a uri",
        "",
        "spotify:playlist:not-base62!",
    ] {
        assert_eq!(
            require_playlist(uri),
            Err(PlaylistError::NotAPlaylist),
            "must refuse {uri:?}"
        );
    }
}

#[test]
fn three_items_are_one_add_in_the_order_sent() {
    let tracks = strings(&[PROBE_TRACK, SECOND_TRACK, THIRD_TRACK]);
    let body = add_body(&probe(), 23, &tracks);
    let mut expected = vec![
        0x0a, 0x18, // ListChanges.base_revision: 24 bytes
    ];
    expected.extend_from_slice(&PROBE_REV);
    expected.extend_from_slice(&[
        0x12, 0x9a, 0x01, // ListChanges.deltas[0]: 154 bytes, two varint bytes
        0x0a, 0x18, // Delta.base_version: 24 bytes
    ]);
    expected.extend_from_slice(&PROBE_REV);
    expected.extend_from_slice(&[
        0x12, 0x7e, // Delta.ops[0]: 126 bytes
        0x08, 0x02, // Op.kind = ADD
        0x12, 0x7a, // Op.add: 122 bytes
        0x08, 0x17, // Add.from_index = 23
    ]);
    for track in &tracks {
        expected.extend_from_slice(&[
            0x12, 0x26, // Add.items[i]: 38 bytes
            0x0a, 0x24, // Item.uri: 36 bytes
        ]);
        expected.extend_from_slice(track.as_bytes());
    }

    // Item = 2 + 36 = 38; three of them under one Add is 3 * 40 = 120.
    // Add = 2 + 120 = 122; Op = 2 + (2 + 122) = 126.
    // Delta = (2 + 24) + (2 + 126) = 154, so its own length is two bytes.
    // ListChanges = 26 + (1 + 2 + 154) = 183.
    assert_eq!(body, expected);
    assert_eq!(body.len(), 183);
}

#[test]
fn a_three_item_add_at_the_varint_boundary_is_one_byte_longer() {
    let tracks = strings(&[PROBE_TRACK, SECOND_TRACK, THIRD_TRACK]);
    // `from_index` 128 costs one more varint byte, and that byte crosses the
    // 128-byte delta boundary, so both the delta and the change set grow.
    assert_eq!(add_body(&probe(), 128, &tracks).len(), 184);
    assert_eq!(add_body(&probe(), 127, &tracks).len(), 183);
}

#[test]
fn a_three_item_add_parses_back_as_one_add_in_order() {
    let tracks = strings(&[PROBE_TRACK, SECOND_TRACK, THIRD_TRACK]);
    let read = page(&add_body(&probe(), 23, &tracks));
    assert_eq!(read.deltas.len(), 1);
    assert_eq!(read.deltas[0].ops.len(), 1, "one op carries the block");
    let found = &read.deltas[0].ops[0];
    assert_eq!(found.kind(), op::Kind::ADD);
    let add = found.add.as_ref().expect("an add");
    assert_eq!(add.from_index(), 23);
    let uris: Vec<&str> = add.items.iter().map(|item| item.uri()).collect();
    assert_eq!(uris, [PROBE_TRACK, SECOND_TRACK, THIRD_TRACK]);
    assert!(found.rem.is_none(), "an add is not a remove");
    assert!(found.mov.is_none(), "an add is not a move");
}

#[test]
fn a_rootlist_add_at_zero_pins_its_bytes() {
    let body = add_body(&probe(), 0, &strings(&[NEW_PLAYLIST]));
    let mut expected = vec![
        0x0a, 0x18, // ListChanges.base_revision: 24 bytes
    ];
    expected.extend_from_slice(&PROBE_REV);
    expected.extend_from_slice(&[
        0x12, 0x4d, // ListChanges.deltas[0]: 77 bytes
        0x0a, 0x18, // Delta.base_version: 24 bytes
    ]);
    expected.extend_from_slice(&PROBE_REV);
    expected.extend_from_slice(&[
        0x12, 0x31, // Delta.ops[0]: 49 bytes
        0x08, 0x02, // Op.kind = ADD
        0x12, 0x2d, // Op.add: 45 bytes
        0x08, 0x00, // Add.from_index = 0
        0x12, 0x29, // Add.items[0]: 41 bytes
        0x0a, 0x27, // Item.uri: 39 bytes
    ]);
    expected.extend_from_slice(NEW_PLAYLIST.as_bytes());

    // Item = 2 + 39 = 41; Add = 2 + (2 + 41) = 45; Op = 2 + (2 + 45) = 49;
    // Delta = 26 + (2 + 49) = 77; ListChanges = 26 + (2 + 77) = 105.
    assert_eq!(body, expected);
    assert_eq!(body.len(), 105);
}

#[test]
fn a_rootlist_add_at_zero_parses_back_as_one_add_at_index_zero() {
    let read = page(&add_body(&probe(), 0, &strings(&[NEW_PLAYLIST])));
    assert_eq!(read.deltas.len(), 1);
    assert_eq!(read.deltas[0].ops.len(), 1);
    let found = &read.deltas[0].ops[0];
    assert_eq!(found.kind(), op::Kind::ADD);
    let add = found.add.as_ref().expect("an add");
    assert_eq!(add.from_index(), 0);
    assert_eq!(add.items.len(), 1);
    assert_eq!(add.items[0].uri(), NEW_PLAYLIST);
}
