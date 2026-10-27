use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device};

#[test]
fn new_playlist_follows_copy_link_and_precedes_the_band() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &playlist_listing(),
        Standing::Unknown,
    );
    let order = verbs(&items);
    let at = |verb: Verb| {
        order
            .iter()
            .position(|found| *found == verb)
            .unwrap_or_else(|| panic!("{verb:?} is offered"))
    };
    assert_eq!(at(Verb::AddToPlaylist), at(Verb::CopyLink) + 1);
    assert_eq!(at(Verb::NewPlaylist), at(Verb::AddToPlaylist) + 1);
    assert_eq!(at(Verb::Remove), at(Verb::NewPlaylist) + 1);
}

#[test]
fn new_playlist_is_offered_on_all_four_listings() {
    for listing in [
        Listing::Page,
        Listing::Queue,
        Listing::Current,
        playlist_listing(),
    ] {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &listing,
            Standing::Unknown,
        );
        assert!(
            verbs(&items).contains(&Verb::NewPlaylist),
            "{listing:?} has no new-playlist row"
        );
    }
}

#[test]
fn a_menu_with_no_other_item_gets_no_new_playlist_row() {
    let mut local = entry("spotify:local:A:B:C:1000");
    local.album_uri = None;
    local.artist_uris = Vec::new();
    let items = track_items(&[local], None, &Listing::Current, Standing::Unknown);
    assert!(
        items.is_empty(),
        "a row that acts on nothing got a menu of its own: {:?}",
        labels(&items)
    );
}

#[test]
fn new_playlist_is_plain_with_no_key_and_no_chevron() {
    let items = track_items(
        &tracks(&["spotify:track:a"]),
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    let row = items
        .iter()
        .find(|item| item.verb == Verb::NewPlaylist)
        .expect("a new-playlist row");
    assert_eq!(row.label, "New playlist…");
    assert_eq!(row.kind, Kind::Plain);
    assert_eq!(row.key, None);
    assert!(!row.submenu);
}

#[test]
fn outcome_new_playlist_carries_the_pick_and_survives_having_none() {
    assert_eq!(
        outcome(Verb::NewPlaylist, &[]),
        Some(Outcome::NewPlaylist { pick: Vec::new() })
    );
    assert_eq!(
        outcome(Verb::NewPlaylist, &tracks(&["spotify:track:a"])),
        Some(Outcome::NewPlaylist {
            pick: pick_of(&["a"]),
        })
    );
    assert_eq!(
        outcome(
            Verb::NewPlaylist,
            &tracks(&["spotify:track:a", "spotify:episode:e"])
        ),
        Some(Outcome::NewPlaylist { pick: Vec::new() }),
        "a mixed pick makes a list, but lands nothing in it"
    );
}

#[test]
fn request_allow_create_false_strips_only_that_row() {
    let targets = tracks(&["spotify:track:a"]);
    let items = track_items(&targets, None, &Listing::Page, Standing::Unknown);
    let offered = items.len();
    let mut request = Request {
        door: Door {
            anchor: Anchor::Pointer(egui::Pos2::ZERO),
            opener: egui::Id::unique("row-more"),
            device: Device::Keyboard,
        },
        playlists: Vec::new(),
        items,
        targets,
        rows: vec![0],
        listing: Listing::Page,
        page: None,
    };

    request.allow_create(false);

    assert_eq!(request.items.len(), offered - 1);
    assert!(
        request
            .items
            .iter()
            .all(|item| item.verb != Verb::NewPlaylist)
    );
    assert!(
        request.items.iter().any(|item| item.verb == Verb::CopyLink),
        "the strip took another row with it"
    );

    request.allow_create(true);
    assert_eq!(
        request.items.len(),
        offered - 1,
        "allowing a stripped row put it back"
    );
}
