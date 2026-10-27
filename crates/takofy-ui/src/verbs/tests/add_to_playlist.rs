use super::super::*;
use super::test_support::*;

#[test]
fn a_pick_of_tracks_keeps_the_rows_order_and_duplicates() {
    assert_eq!(pick(&tracks(&["spotify:track:a"])), Some(pick_of(&["a"])));
    assert_eq!(
        pick(&tracks(&[
            "spotify:track:a",
            "spotify:track:b",
            "spotify:track:c"
        ])),
        Some(pick_of(&["a", "b", "c"]))
    );
    assert_eq!(
        pick(&tracks(&["spotify:track:a", "spotify:track:a"])),
        Some(pick_of(&["a", "a"])),
        "two rows holding one track both travel"
    );
}

#[test]
fn a_pick_with_anything_but_tracks_is_not_a_pick() {
    assert_eq!(pick(&[]), None, "an empty pick");
    assert!(!pickable(&[]));
    for uri in ["spotify:episode:e", "spotify:local:A:B:C:1000"] {
        assert_eq!(pick(&tracks(&[uri])), None, "{uri}");
        assert_eq!(
            pick(&tracks(&["spotify:track:a", uri])),
            None,
            "a mixed pick is refused whole: {uri}"
        );
        assert_eq!(
            pick(&tracks(&[uri, "spotify:track:a"])),
            None,
            "{uri} in front"
        );
    }
}

#[test]
fn a_pick_is_only_tracks_and_nothing_else() {
    assert!(pickable(&tracks(&["spotify:track:a"])));
    assert!(pickable(&tracks(&["spotify:track:a", "spotify:track:b"])));
    assert!(!pickable(&tracks(&["spotify:episode:e"])));
}

#[test]
fn an_add_row_names_the_pick_and_opens_a_list() {
    for (uris, label) in [
        (&["spotify:track:a"][..], "Add to playlist"),
        (
            &["spotify:track:a", "spotify:track:b", "spotify:track:c"][..],
            "Add 3 tracks to playlist",
        ),
    ] {
        let items = track_items(&tracks(uris), None, &Listing::Page, Standing::Unknown);
        let row = items
            .iter()
            .find(|item| item.verb == Verb::AddToPlaylist)
            .expect("an add row");
        assert_eq!(row.label, label);
        assert_eq!(row.kind, Kind::Plain);
        assert_eq!(row.key, None);
        assert!(row.submenu, "the add row is not a command");
    }
}

#[test]
fn an_add_row_is_absent_for_a_pick_with_no_tracks() {
    for listing in [
        Listing::Page,
        Listing::Queue,
        Listing::Current,
        playlist_listing(),
    ] {
        for uris in [
            &["spotify:episode:e"][..],
            &["spotify:local:A:B:C:1000"][..],
            &["spotify:track:a", "spotify:episode:e"][..],
        ] {
            let items = track_items(&tracks(uris), None, &listing, Standing::Unknown);
            assert!(
                items.iter().all(|item| item.verb != Verb::AddToPlaylist),
                "{listing:?} offered an add for {uris:?}"
            );
        }
    }
}

#[test]
fn outcome_add_to_playlist_carries_the_pick_in_order() {
    let targets = tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
    assert_eq!(
        outcome(Verb::AddToPlaylist, &targets),
        Some(Outcome::AddToPlaylist {
            tracks: pick_of(&["a", "b", "c"]),
        })
    );
    assert_eq!(outcome(Verb::AddToPlaylist, &[]), None);
    assert_eq!(
        outcome(
            Verb::AddToPlaylist,
            &tracks(&["spotify:track:a", "spotify:episode:e"])
        ),
        None
    );
}
