use super::super::*;
use super::test_support::*;

#[test]
fn one_track_gets_the_sheets_six_verbs_in_order() {
    let items = track_items(
        &[entry("spotify:track:a")],
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    assert_eq!(
        labels(&items),
        [
            "Add to queue",
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
            Verb::GoToAlbum,
            Verb::GoToArtist,
            Verb::CopyLink,
            Verb::AddToPlaylist,
            Verb::NewPlaylist,
        ]
    );
    assert!(items.iter().all(|item| item.kind == Kind::Plain));
}

#[test]
fn a_pick_puts_its_count_in_the_labels() {
    let targets = [
        entry("spotify:track:a"),
        entry("spotify:track:b"),
        entry("spotify:track:c"),
    ];
    let items = track_items(&targets, None, &Listing::Page, Standing::Unknown);
    let labels = labels(&items);
    assert_eq!(labels[0], "Add 3 tracks to queue");
    assert_eq!(labels[2], "Go to artist", "a go-to never carries a count");
    assert_eq!(labels[3], "Copy 3 links");
}

#[test]
fn a_pick_across_albums_omits_go_to_album_and_across_artists_go_to_artist() {
    let mut other_album = entry("spotify:track:b");
    other_album.album_uri = Some("spotify:album:z".to_owned());
    let items = track_items(
        &[entry("spotify:track:a"), other_album],
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    assert!(!labels(&items).contains(&"Go to album"));

    let mut other_artist = entry("spotify:track:b");
    other_artist.artist_uris = vec!["spotify:artist:z".to_owned()];
    let items = track_items(
        &[entry("spotify:track:a"), other_artist],
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    assert!(!labels(&items).contains(&"Go to artist"));
}

#[test]
fn a_missing_album_or_artist_uri_omits_the_verb() {
    let mut bare = entry("spotify:track:a");
    bare.album_uri = None;
    bare.artist_uris = Vec::new();
    let items = track_items(&[bare], None, &Listing::Page, Standing::Unknown);
    assert_eq!(
        labels(&items),
        [
            "Add to queue",
            "Copy link",
            "Add to playlist",
            "New playlist…"
        ]
    );
}

#[test]
fn a_verb_that_leads_here_is_omitted() {
    let targets = [entry("spotify:track:a")];
    let album = track_items(
        &targets,
        Some(&Page::Album("spotify:album:a".to_owned())),
        &Listing::Page,
        Standing::Unknown,
    );
    assert!(!labels(&album).contains(&"Go to album"));
    assert!(labels(&album).contains(&"Go to artist"));

    let artist = track_items(
        &targets,
        Some(&Page::credit("spotify:artist:b")),
        &Listing::Page,
        Standing::Unknown,
    );
    assert!(!labels(&artist).contains(&"Go to artist"));
    assert!(labels(&artist).contains(&"Go to album"));
}

#[test]
fn a_pick_with_one_linkable_track_says_copy_link() {
    let mut local = entry("spotify:local:A:B:C:1000");
    local.album_uri = None;
    local.artist_uris = Vec::new();
    let targets = [entry("spotify:track:a"), local];
    let items = track_items(&targets, None, &Listing::Page, Standing::Unknown);
    let copy = items
        .iter()
        .find(|item| item.verb == Verb::CopyLink)
        .expect("one target is linkable");
    assert_eq!(copy.label, "Copy link");
    let Some(Outcome::Copy(text)) = outcome(Verb::CopyLink, &targets) else {
        panic!("copy link copies");
    };
    assert_eq!(text, "https://open.spotify.com/track/a");
    assert_eq!(text.lines().count(), 1);
}

#[test]
fn a_local_only_pick_has_no_copy_link() {
    let mut local = entry("spotify:local:A:B:C:1000");
    local.album_uri = None;
    local.artist_uris = Vec::new();
    let items = track_items(&[local], None, &Listing::Page, Standing::Unknown);
    assert_eq!(labels(&items), ["Add to queue", "New playlist…"]);
}

#[test]
fn an_episode_pick_says_items() {
    let episode = entry("spotify:episode:e");
    let items = track_items(
        &[episode.clone(), episode],
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    assert_eq!(labels(&items)[0], "Add 2 items to queue");
}

#[test]
fn outcomes_carry_what_the_menu_snapshot_held() {
    let targets = [
        entry("spotify:track:a"),
        entry("spotify:track:b"),
        entry("spotify:track:c"),
    ];
    assert_eq!(
        outcome(Verb::AddToQueue, &targets),
        Some(Outcome::Queue(targets.to_vec()))
    );

    let Some(Outcome::Copy(text)) = outcome(Verb::CopyLink, &targets) else {
        panic!("copy link copies");
    };
    assert_eq!(
        text,
        "https://open.spotify.com/track/a\nhttps://open.spotify.com/track/b\n\
         https://open.spotify.com/track/c"
    );
    assert!(!text.ends_with('\n'), "one URL per line, no trailing break");

    assert_eq!(
        outcome(Verb::GoToAlbum, &targets),
        Some(Outcome::Open(Page::Album("spotify:album:a".to_owned())))
    );
    assert_eq!(
        outcome(Verb::GoToArtist, &targets),
        Some(Outcome::Open(Page::Artist("spotify:artist:b".to_owned())))
    );
}

#[test]
fn an_outcome_that_cannot_apply_is_none() {
    let mut bare = entry("spotify:track:a");
    bare.album_uri = None;
    bare.artist_uris = Vec::new();
    assert_eq!(outcome(Verb::GoToAlbum, &[bare.clone()]), None);
    assert_eq!(outcome(Verb::GoToArtist, &[bare]), None);
    assert_eq!(outcome(Verb::AddToQueue, &[]), None);
}

#[test]
fn a_playlist_pick_labels_the_count() {
    let one = track_items(
        &[entry("spotify:track:a")],
        None,
        &playlist_listing(),
        Standing::Unknown,
    );
    assert_eq!(
        labels(&one),
        [
            "Add to queue",
            "Go to album",
            "Go to artist",
            "Copy link",
            "Add to playlist",
            "New playlist…",
            "Remove from this playlist",
        ]
    );

    let three = [
        entry("spotify:track:a"),
        entry("spotify:track:b"),
        entry("spotify:track:c"),
    ];
    let items = track_items(&three, None, &playlist_listing(), Standing::Unknown);
    assert!(
        labels(&items).contains(&"Remove 3 tracks from this playlist"),
        "{:?}",
        labels(&items)
    );
}

#[test]
fn a_queue_pick_labels_the_count_without_a_noun() {
    let one = track_items(
        &[entry("spotify:track:a")],
        None,
        &Listing::Queue,
        Standing::Unknown,
    );
    assert_eq!(
        labels(&one),
        [
            "Go to album",
            "Go to artist",
            "Copy link",
            "Add to playlist",
            "New playlist…",
            "Remove from the queue"
        ]
    );

    let three = [
        entry("spotify:track:a"),
        entry("spotify:track:b"),
        entry("spotify:track:c"),
    ];
    let items = track_items(&three, None, &Listing::Queue, Standing::Unknown);
    assert_eq!(
        labels(&items).last(),
        Some(&"Remove 3 from the queue"),
        "the destructive row stays last"
    );
}

#[test]
fn the_remove_row_is_last_destructive_and_carries_del() {
    let items = track_items(
        &[entry("spotify:track:a")],
        None,
        &playlist_listing(),
        Standing::Unknown,
    );
    let last = items.last().expect("a command");
    assert_eq!(last.verb, Verb::Remove);
    assert_eq!(last.kind, Kind::Destructive);
    assert_eq!(last.key, Some("Del"));
    assert!(!last.submenu);
    assert!(
        items[..items.len() - 1]
            .iter()
            .all(|item| item.kind == Kind::Plain),
        "a second command claimed to be destructive"
    );
}

#[test]
fn a_page_listing_has_no_remove_row() {
    let items = track_items(
        &[entry("spotify:track:a")],
        None,
        &Listing::Page,
        Standing::Unknown,
    );
    assert!(!verbs(&items).contains(&Verb::Remove));
    assert!(verbs(&items).contains(&Verb::AddToQueue));
}

#[test]
fn the_current_listing_has_neither_add_nor_remove() {
    let items = track_items(
        &[entry("spotify:track:a")],
        None,
        &Listing::Current,
        Standing::Unknown,
    );
    assert_eq!(
        verbs(&items),
        [
            Verb::GoToAlbum,
            Verb::GoToArtist,
            Verb::CopyLink,
            Verb::AddToPlaylist,
            Verb::NewPlaylist
        ]
    );
}

#[test]
fn the_queue_listing_has_no_add_to_queue() {
    let items = track_items(
        &[entry("spotify:track:a")],
        None,
        &Listing::Queue,
        Standing::Unknown,
    );
    assert!(!verbs(&items).contains(&Verb::AddToQueue));
    assert_eq!(verbs(&items).last(), Some(&Verb::Remove));
}

#[test]
fn no_other_item_carries_a_key_hint() {
    let targets = [entry("spotify:track:a")];
    for listing in [
        Listing::Page,
        Listing::Queue,
        Listing::Current,
        playlist_listing(),
    ] {
        for item in track_items(&targets, None, &listing, Standing::Unknown) {
            if item.verb == Verb::Remove {
                assert_eq!(item.key, Some("Del"), "{}", item.label);
            } else {
                assert_eq!(item.key, None, "{}", item.label);
            }
            assert_eq!(
                item.submenu,
                item.verb == Verb::AddToPlaylist,
                "{}",
                item.label
            );
        }
    }
}

#[test]
fn a_local_current_entry_has_no_items() {
    let mut local = entry("spotify:local:A:B:C:1000");
    local.album_uri = None;
    local.artist_uris = Vec::new();
    assert!(track_items(&[local], None, &Listing::Current, Standing::Unknown).is_empty());
}

#[test]
fn outcome_remove_is_a_unit_and_none_for_no_targets() {
    let targets = [entry("spotify:track:a")];
    assert_eq!(outcome(Verb::Remove, &targets), Some(Outcome::Remove));
    assert_eq!(outcome(Verb::Remove, &[]), None);
}

#[test]
fn a_mixed_pick_says_items_and_counts_only_the_linkable_rows() {
    let mut local = entry("spotify:local:A:B:C:1000");
    local.album_uri = None;
    local.artist_uris = Vec::new();
    let targets = [entry("spotify:track:a"), entry("spotify:episode:e"), local];

    let items = track_items(&targets, None, &playlist_listing(), Standing::Unknown);
    assert!(
        labels(&items).contains(&"Remove 3 items from this playlist"),
        "{:?}",
        labels(&items)
    );
    let copy = items
        .iter()
        .find(|item| item.verb == Verb::CopyLink)
        .expect("two rows are linkable");
    assert_eq!(copy.label, "Copy 2 links");
}
