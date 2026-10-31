use super::super::*;
use super::test_support::*;

fn library(items: &[Item]) -> Option<&Item> {
    items
        .iter()
        .find(|item| matches!(item.verb, Verb::Save | Verb::Unsave))
}

#[test]
fn heart_label_names_the_action() {
    assert_eq!(heart_label(false), "Save to your library");
    assert_eq!(heart_label(true), "Remove from your library");
}

#[test]
fn an_unsaved_track_gets_save_after_add_to_queue() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &Listing::Page,
        Standing::Unsaved,
    );
    assert_eq!(
        labels(&items),
        [
            "Add to queue",
            "Save to your library",
            "Start radio",
            "Go to album",
            "Go to artist",
            "Copy link",
            "Add to playlist",
            "New playlist…"
        ]
    );
    assert_eq!(
        verbs(&items),
        [
            Verb::AddToQueue,
            Verb::Save,
            Verb::StartRadio,
            Verb::GoToAlbum,
            Verb::GoToArtist,
            Verb::CopyLink,
            Verb::AddToPlaylist,
            Verb::NewPlaylist
        ]
    );
}

#[test]
fn a_saved_track_gets_remove_from_your_library() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &Listing::Page,
        Standing::Saved,
    );
    let row = library(&items).expect("a library row");
    assert_eq!(row.label, "Remove from your library");
    assert_eq!(row.verb, Verb::Unsave);
}

#[test]
fn a_pick_of_unsaved_says_save_n_tracks() {
    let items = track_items(
        &tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]),
        None,
        &Listing::Page,
        Standing::Unsaved,
    );
    assert_eq!(
        library(&items).expect("a library row").label,
        "Save 3 tracks to your library"
    );
}

#[test]
fn a_pick_of_saved_says_remove_n_tracks_from_your_library() {
    let items = track_items(
        &tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]),
        None,
        &Listing::Page,
        Standing::Saved,
    );
    assert_eq!(
        library(&items).expect("a library row").label,
        "Remove 3 tracks from your library"
    );
}

#[test]
fn a_mixed_pick_offers_save_n_tracks_and_never_remove() {
    let targets = tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
    let items = track_items(&targets, None, &Listing::Page, Standing::Mixed);
    assert_eq!(
        library(&items).expect("a library row").label,
        "Save 3 tracks to your library",
        "the count is the whole pick, because that is what the sheet draws"
    );
    assert!(
        items.iter().all(|item| item.verb != Verb::Unsave),
        "a mixed pick has no single remove"
    );
}

#[test]
fn an_unknown_standing_has_no_library_row() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    assert!(library(&items).is_none());
}

#[test]
fn a_pick_with_an_episode_or_a_local_file_has_no_library_row() {
    for uri in ["spotify:episode:e", "spotify:local:A:B:C:1000"] {
        let items = track_items(
            &tracks(&["spotify:track:a", uri]),
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(library(&items).is_none(), "{uri}");
    }
}

#[test]
fn all_four_listings_carry_the_library_row() {
    let targets = tracks(&["spotify:track:a"]);
    for listing in [
        Listing::Page,
        Listing::Queue,
        Listing::Current,
        playlist_listing(),
    ] {
        let items = track_items(&targets, None, &listing, Standing::Unsaved);
        assert!(library(&items).is_some(), "{listing:?} has no library row");
    }
}

#[test]
fn the_queue_listing_has_save_but_no_add_to_queue() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &Listing::Queue,
        Standing::Unsaved,
    );
    assert!(library(&items).is_some());
    assert!(items.iter().all(|item| item.verb != Verb::AddToQueue));
}

#[test]
fn the_current_listing_with_a_saveable_track_has_a_non_empty_menu() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &Listing::Current,
        Standing::Unsaved,
    );
    assert!(!items.is_empty());
    assert!(library(&items).is_some());
}

#[test]
fn the_library_row_is_plain_and_carries_no_key_hint() {
    for standing in [Standing::Unsaved, Standing::Saved, Standing::Mixed] {
        let items = track_items(
            &tracks(&["spotify:track:a", "spotify:track:b"]),
            None,
            &Listing::Page,
            standing,
        );
        let row = library(&items).expect("a library row");
        assert_eq!(row.kind, Kind::Plain, "{standing:?}");
        assert_eq!(row.key, None, "{standing:?}");
        assert!(!row.submenu, "{standing:?}");
    }
}

#[test]
fn outcome_save_lists_every_track_uri_once_in_order() {
    let targets = tracks(&[
        "spotify:track:a",
        "spotify:track:b",
        "spotify:track:a",
        "spotify:track:c",
    ]);
    assert_eq!(
        outcome(Verb::Save, &targets),
        Some(Outcome::Like {
            tracks: vec![
                "spotify:track:a".to_owned(),
                "spotify:track:b".to_owned(),
                "spotify:track:c".to_owned(),
            ],
            liked: true,
        })
    );
}

#[test]
fn outcome_unsave_is_the_same_with_liked_false() {
    let targets = tracks(&["spotify:track:a", "spotify:track:b"]);
    assert_eq!(
        outcome(Verb::Unsave, &targets),
        Some(Outcome::Like {
            tracks: vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()],
            liked: false,
        })
    );
}

#[test]
fn outcome_save_is_none_for_a_pick_with_a_non_track() {
    assert_eq!(
        outcome(
            Verb::Save,
            &tracks(&["spotify:track:a", "spotify:episode:e"])
        ),
        None
    );
    assert_eq!(
        outcome(Verb::Save, &tracks(&["spotify:local:A:B:C:1000"])),
        None
    );
    assert_eq!(outcome(Verb::Save, &[]), None);
}
