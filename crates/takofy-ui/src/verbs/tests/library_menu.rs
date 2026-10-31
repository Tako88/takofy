use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device};
use crate::sidebar_kind::SidebarKind;

fn library_row(kind: SidebarKind, name: &str) -> LibraryItem {
    let uri = match kind {
        SidebarKind::Playlists => "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC",
        SidebarKind::Albums => "spotify:album:4uLU6hMCjMI75M1A2tKUQC",
        SidebarKind::Artists => "spotify:artist:4uLU6hMCjMI75M1A2tKUQC",
        SidebarKind::Podcasts => "spotify:show:4uLU6hMCjMI75M1A2tKUQC",
    };
    LibraryItem {
        kind,
        uri: uri.to_owned(),
        name: name.to_owned(),
    }
}

fn rights(renamable: bool, removable: bool) -> LibraryRights {
    LibraryRights {
        renamable,
        removable,
        saved: Some(true),
    }
}

#[test]
fn an_artist_band_offers_its_follow_its_radio_and_its_link() {
    let row = crate::menu::LibraryItem {
        kind: crate::sidebar_kind::SidebarKind::Artists,
        uri: "spotify:artist:a".to_owned(),
        name: "Vela Moreno".to_owned(),
    };
    let door = Door {
        anchor: Anchor::Pointer(egui::Pos2::ZERO),
        opener: egui::Id::unique("artist-more"),
        device: Device::Keyboard,
    };

    let followed = artist_request(row.clone(), door, false, None);
    let labels: Vec<&str> = followed
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["Unfollow", "Start radio", "Copy link"],
        "{labels:?}"
    );

    let unfollowed = artist_request(row, door, true, None);
    let labels: Vec<&str> = unfollowed
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(labels, ["Follow", "Start radio", "Copy link"], "{labels:?}");
}

#[test]
fn a_library_rows_radio_seed_follows_its_kind() {
    let cases = [
        (SidebarKind::Playlists, SeedKind::Playlist),
        (SidebarKind::Albums, SeedKind::Album),
        (SidebarKind::Artists, SeedKind::Artist),
    ];
    for (kind, seed_kind) in cases {
        let row = library_row(kind, "Deep Focus");
        assert_eq!(
            radio_seed(&row),
            Some(StationSeed {
                kind: seed_kind,
                uri: row.uri.clone(),
            }),
            "{kind:?}"
        );
    }
    assert_eq!(
        radio_seed(&library_row(SidebarKind::Podcasts, "Field Notes")),
        None,
        "a show has no station on the wire"
    );
}

#[test]
fn a_row_whose_uri_names_another_kind_offers_no_radio() {
    let cases = [
        LibraryItem {
            kind: SidebarKind::Albums,
            uri: "spotify:artist:abc".to_owned(),
            name: "A Record".to_owned(),
        },
        LibraryItem {
            kind: SidebarKind::Playlists,
            uri: "not a uri".to_owned(),
            name: "Deep Focus".to_owned(),
        },
        library_row(SidebarKind::Podcasts, "Field Notes"),
    ];
    for row in cases {
        let offered = verbs(&library_items(&row, rights(false, true)));
        assert!(!offered.contains(&Verb::StartRadio), "{row:?}");
    }
}

#[test]
fn radio_sits_right_after_the_library_row_and_before_the_removal() {
    let row = library_row(SidebarKind::Playlists, "Deep Focus");
    let items = library_items(
        &row,
        LibraryRights {
            renamable: true,
            removable: true,
            saved: Some(false),
        },
    );

    assert_eq!(
        verbs(&items),
        [
            Verb::Rename,
            Verb::CopyLink,
            Verb::SaveToLibrary,
            Verb::StartRadio,
            Verb::RemoveFromLibrary,
        ]
    );
    let radio = &items[3];
    assert_eq!(radio.kind, Kind::Plain, "a radio is a plain command");
    assert_eq!(radio.key, None, "a radio has no key hint");

    let saved = |saved, removable| LibraryRights {
        renamable: false,
        removable,
        saved,
    };
    assert_eq!(
        verbs(&library_items(&row, saved(Some(true), true))),
        [Verb::CopyLink, Verb::StartRadio, Verb::RemoveFromLibrary],
        "with the library's row already written the radio follows the link"
    );
    assert_eq!(
        verbs(&library_items(&row, saved(None, false))),
        [Verb::CopyLink, Verb::StartRadio],
        "a row with no removal keeps the radio last"
    );
}

#[test]
fn an_artists_verbs_name_the_follow_and_the_link() {
    let row = crate::menu::LibraryItem {
        kind: crate::sidebar_kind::SidebarKind::Artists,
        uri: "spotify:artist:a".to_owned(),
        name: "Vela Moreno".to_owned(),
    };

    assert_eq!(
        library_outcome(Verb::Follow, &row),
        Some(Outcome::Follow {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        })
    );
    assert_eq!(
        library_outcome(Verb::Unfollow, &row),
        Some(Outcome::Follow {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        })
    );
    assert_eq!(
        library_outcome(Verb::SaveToLibrary, &row),
        Some(Outcome::Save(row))
    );
}

/// A band offers the save only when the library says it is not there, and the
/// removal only when it is.
#[test]
fn a_band_offers_one_direction_of_the_library_write() {
    let row = crate::menu::LibraryItem {
        kind: crate::sidebar_kind::SidebarKind::Albums,
        uri: "spotify:album:r".to_owned(),
        name: "Signal Decay".to_owned(),
    };
    let rights = |saved| LibraryRights {
        renamable: false,
        removable: saved == Some(true),
        saved,
    };

    let labels = |saved| -> Vec<String> {
        library_items(&row, rights(saved))
            .into_iter()
            .map(|item| item.label)
            .collect()
    };
    assert_eq!(
        labels(Some(false)),
        ["Copy link", "Save to your library", "Start radio"],
        "the identity verb comes before the library's"
    );
    assert_eq!(
        labels(Some(true)),
        ["Copy link", "Start radio", "Remove from library"],
        "the destructive row stays last"
    );
    assert_eq!(
        labels(None),
        ["Copy link", "Start radio"],
        "an unknown state offers neither write"
    );
}

#[test]
fn a_library_row_offers_its_link_and_the_removal_last() {
    for kind in crate::sidebar_kind::ROW {
        let row = library_row(kind, "Deep Focus");
        let items = library_items(&row, rights(false, true));

        let expected_labels: Vec<&str> = if kind == SidebarKind::Podcasts {
            vec!["Copy link", "Remove from library"]
        } else {
            vec!["Copy link", "Start radio", "Remove from library"]
        };
        assert_eq!(labels(&items), expected_labels, "{kind:?}");
        let expected: Vec<Verb> = if kind == SidebarKind::Podcasts {
            vec![Verb::CopyLink, Verb::RemoveFromLibrary]
        } else {
            vec![Verb::CopyLink, Verb::StartRadio, Verb::RemoveFromLibrary]
        };
        assert_eq!(verbs(&items), expected, "{kind:?}");
        let removal = items.last().expect("a removal row");
        assert_eq!(removal.kind, Kind::Destructive, "{kind:?}");
        assert_eq!(removal.key, Some("Del"), "{kind:?}");
        assert!(!removal.submenu, "{kind:?}");
        assert_eq!(items[0].kind, Kind::Plain, "{kind:?}");
        assert_eq!(items[0].key, None, "{kind:?}");
    }
}

#[test]
fn a_renamable_library_row_offers_rename_first_and_plain() {
    let row = library_row(SidebarKind::Playlists, "Deep Focus");
    let items = library_items(&row, rights(true, true));

    assert_eq!(
        labels(&items),
        ["Rename", "Copy link", "Start radio", "Remove from library"]
    );
    assert_eq!(verbs(&items)[0], Verb::Rename);
    assert_eq!(items[0].kind, Kind::Plain);
    assert_eq!(items[0].key, None, "a rename has no key hint");
}

#[test]
fn a_row_that_is_not_renamable_has_no_rename_row() {
    let row = library_row(SidebarKind::Playlists, "Deep Focus");

    assert!(
        library_items(&row, rights(false, true))
            .iter()
            .all(|item| item.verb != Verb::Rename),
        "a row the user cannot rename offered one"
    );
}

#[test]
fn a_library_row_the_library_cannot_write_offers_only_its_link_and_radio() {
    let row = library_row(SidebarKind::Playlists, "Deep Focus");

    assert_eq!(
        labels(&library_items(&row, rights(false, false))),
        ["Copy link", "Start radio"]
    );
    assert!(
        library_items(&row, rights(false, false))
            .iter()
            .all(|item| item.verb != Verb::RemoveFromLibrary)
    );
}

#[test]
fn library_outcome_copies_the_link_renames_and_unlinks_the_item() {
    let row = library_row(SidebarKind::Albums, "A Record");

    assert_eq!(
        library_outcome(Verb::CopyLink, &row),
        Some(Outcome::Copy(
            "https://open.spotify.com/album/4uLU6hMCjMI75M1A2tKUQC".to_owned()
        ))
    );
    assert_eq!(
        library_outcome(Verb::RemoveFromLibrary, &row),
        Some(Outcome::Unlink(row.clone()))
    );
    assert_eq!(
        library_outcome(Verb::Rename, &row),
        Some(Outcome::Rename(row.clone()))
    );
    for verb in [
        Verb::AddToQueue,
        Verb::Save,
        Verb::Unsave,
        Verb::GoToAlbum,
        Verb::GoToArtist,
        Verb::AddToPlaylist,
        Verb::NewPlaylist,
        Verb::Remove,
    ] {
        assert_eq!(library_outcome(verb, &row), None, "{verb:?}");
    }
}

#[test]
fn library_outcome_starts_the_radio_the_kind_seeds() {
    let cases = [
        (SidebarKind::Playlists, SeedKind::Playlist),
        (SidebarKind::Albums, SeedKind::Album),
        (SidebarKind::Artists, SeedKind::Artist),
    ];
    for (kind, seed_kind) in cases {
        let row = library_row(kind, "Deep Focus");
        assert_eq!(
            library_outcome(Verb::StartRadio, &row),
            Some(Outcome::StartRadio {
                seed: StationSeed {
                    kind: seed_kind,
                    uri: row.uri.clone(),
                },
            }),
            "{kind:?}"
        );
    }
    assert_eq!(
        library_outcome(
            Verb::StartRadio,
            &library_row(SidebarKind::Podcasts, "Field Notes")
        ),
        None,
        "a show seeds no station"
    );
    let other = LibraryItem {
        kind: SidebarKind::Albums,
        uri: "spotify:artist:abc".to_owned(),
        name: "A Record".to_owned(),
    };
    assert_eq!(library_outcome(Verb::StartRadio, &other), None);
}

#[test]
fn a_track_pick_has_no_remove_from_library_outcome() {
    assert_eq!(
        outcome(Verb::RemoveFromLibrary, &tracks(&["spotify:track:a"])),
        None,
        "a track pick is not a library row"
    );
}

#[test]
fn a_library_rows_menu_is_named_after_the_row() {
    let row = library_row(SidebarKind::Playlists, "Deep Focus");

    assert_eq!(
        menu_label(&[], &Listing::Library(row.clone())),
        "Deep Focus",
        "a menu states its subject, not its commands"
    );
    assert_eq!(
        menu_label(&[], &Listing::Band(row)),
        "Deep Focus",
        "a band menu names its playlist too"
    );
    assert_eq!(menu_label(&[], &Listing::Page), "Commands");
}
#[test]
fn library_request_picks_the_listing_from_the_surface() {
    let row = library_row(SidebarKind::Playlists, "Deep Focus");
    let door = Door {
        anchor: Anchor::Button(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(28.0, 28.0),
        )),
        opener: egui::Id::unique("row"),
        device: Device::Keyboard,
    };
    let request = library_request(
        row.clone(),
        door,
        rights(true, true),
        Some(Page::Playlist("spotify:playlist:a".to_owned())),
        crate::naming::Surface::Band,
    );

    assert_eq!(request.listing, Listing::Band(row.clone()));
    assert_eq!(
        request
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["Rename", "Copy link", "Start radio", "Remove from library"]
    );
    for surface in [crate::naming::Surface::Row, crate::naming::Surface::Band] {
        let request = library_request(row.clone(), door, rights(false, false), None, surface);
        assert_eq!(
            matches!(request.listing, Listing::Library(_)),
            surface == crate::naming::Surface::Row
        );
    }
}
