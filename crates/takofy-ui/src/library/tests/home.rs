use super::super::*;
use super::test_support::*;
use takofy_model::{LikedTrack, SavedAt, TrackEntry};

/// A saved date `ms` after the epoch.
fn at(ms: u64) -> Option<SavedAt> {
    SavedAt::from_millis(i64::try_from(ms).ok()?)
}

fn dated_playlist(uri: &str, ms: u64) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: at(ms),
        ..playlist(uri, uri)
    }
}

fn dated_album(uri: &str, ms: u64) -> SavedAlbum {
    SavedAlbum {
        saved_at: at(ms),
        ..saved_album(uri)
    }
}

fn liked(uri: &str, ms: u64) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: at(ms),
    }
}

/// A library whose three Home sources have all answered with these lists.
fn settled(
    playlists: Vec<PlaylistEntry>,
    albums: Vec<SavedAlbum>,
    tracks: Vec<LikedTrack>,
) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(playlists));
    library.apply(&Update::SavedAlbums(albums));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(tracks));
    library
}

/// The URI a card draws, whichever kind it is.
fn card_uri(card: &crate::home::Card<'_>) -> String {
    match card.saved {
        crate::home::Saved::Playlist(entry) => entry.uri.clone(),
        crate::home::Saved::Album(album) => album.uri.clone(),
        crate::home::Saved::Track(track) => track.uri.clone(),
    }
}

/// The URIs a shelf draws, in card order; empty for a shelf with no cards.
fn drawn_uris(shelf: &crate::home::Shelf<'_>) -> Vec<String> {
    match shelf {
        crate::home::Shelf::Cards(cards) => cards.iter().map(card_uri).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn a_held_seed_draws_the_same_cards() {
    let library = settled(
        vec![
            dated_playlist("spotify:playlist:a", 5_000),
            dated_playlist("spotify:playlist:b", 4_000),
            dated_playlist("spotify:playlist:c", 3_000),
        ],
        Vec::new(),
        Vec::new(),
    );

    let first = library.home_shelf(2);
    assert_eq!(
        first,
        library.home_shelf(2),
        "a repaint draws the same shelf"
    );
    match &first {
        crate::home::Shelf::Cards(cards) => assert_eq!(cards.len(), 2),
        other => panic!("expected cards, got {other:?}"),
    }
}

#[test]
fn the_pool_holds_no_undated_item() {
    let library = settled(
        vec![
            dated_playlist("spotify:playlist:a", 5_000),
            playlist("spotify:playlist:b", "B"),
        ],
        vec![saved_album("spotify:album:r")],
        Vec::new(),
    );

    match library.home_shelf(8) {
        crate::home::Shelf::Cards(cards) => {
            let uris: Vec<String> = cards.iter().map(card_uri).collect();
            assert_eq!(uris, vec!["spotify:playlist:a".to_owned()]);
            assert_eq!(cards[0].at, at(5_000).expect("a date"));
        }
        other => panic!("expected cards, got {other:?}"),
    }
}

#[test]
fn a_dated_album_draws_a_card() {
    let library = settled(
        Vec::new(),
        vec![dated_album("spotify:album:r", 5_000)],
        Vec::new(),
    );

    match library.home_shelf(4) {
        crate::home::Shelf::Cards(cards) => {
            assert_eq!(cards.len(), 1);
            assert_eq!(card_uri(&cards[0]), "spotify:album:r");
        }
        other => panic!("expected cards, got {other:?}"),
    }
}

#[test]
fn a_liked_card_waits_for_its_label_and_then_draws() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        vec![liked("spotify:track:t", 5_000)],
    );
    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Waiting,
        "an unlabelled liked song holds the shelf"
    );

    let named = TrackEntry {
        album_uri: Some("spotify:album:r".to_owned()),
        ..track("spotify:track:t")
    };
    library.apply(&Update::SavedLabelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: vec![named.clone()],
    });

    match library.home_shelf(4) {
        crate::home::Shelf::Cards(cards) => {
            assert_eq!(cards.len(), 1);
            assert_eq!(
                cards[0].saved,
                crate::home::Saved::Track(&named),
                "the card draws the named track"
            );
        }
        other => panic!("expected cards, got {other:?}"),
    }
}

#[test]
fn an_answered_unusable_track_leaves_the_pool() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        vec![liked("spotify:track:t", 5_000)],
    );
    library.apply(&Update::SavedLabelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: Vec::new(),
    });

    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Quiet,
        "the answer is in, so the shelf is not waiting; nothing is left to draw"
    );
}

#[test]
fn a_wider_shelf_keeps_the_cards_it_already_drew() {
    let library = settled(
        vec![
            dated_playlist("spotify:playlist:a", 5_000),
            dated_playlist("spotify:playlist:b", 4_000),
            dated_playlist("spotify:playlist:c", 3_000),
            dated_playlist("spotify:playlist:d", 2_000),
        ],
        Vec::new(),
        Vec::new(),
    );

    for cards in 0..4 {
        let narrower = drawn_uris(&library.home_shelf(cards));
        let wider = drawn_uris(&library.home_shelf(cards + 1));
        assert!(
            narrower.iter().all(|uri| wider.contains(uri)),
            "k = {cards}"
        );
        assert_eq!(wider.len(), cards + 1, "k = {cards}");
    }
}

#[test]
fn a_song_liked_this_session_is_not_the_day_one_page() {
    let mut library = settled(Vec::new(), Vec::new(), Vec::new());
    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::DayOne,
        "arrange: nothing is saved"
    );

    assert!(
        library
            .set_liked(vec!["spotify:track:t".to_owned()], true)
            .is_some(),
        "arrange: the set takes the like"
    );
    assert_eq!(
        library.likes.recent().expect("a read landed").count(),
        0,
        "arrange: the set has the song, the last read does not"
    );

    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Quiet,
        "the set is not empty, so the user is not told nothing is saved"
    );
}

#[test]
fn a_failed_likes_read_with_a_held_set_still_draws_cards() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        vec![liked("spotify:track:t", 5_000)],
    );
    let named = TrackEntry {
        album_uri: Some("spotify:album:r".to_owned()),
        ..track("spotify:track:t")
    };
    library.apply(&Update::SavedLabelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: vec![named],
    });

    library.likes.reload();
    assert!(
        library.likes_request().is_some(),
        "arrange: a re-read is in flight"
    );
    library.apply(&Update::LikedTracksFailed(Failure::Load));
    assert_eq!(
        library.likes.phase(),
        crate::home::Phase::Failed(Failure::Load),
        "arrange: the read failed"
    );

    match library.home_shelf(4) {
        crate::home::Shelf::Cards(cards) => assert_eq!(cards.len(), 1),
        other => panic!("expected cards, got {other:?}"),
    }
}

#[test]
fn an_idle_library_waits() {
    assert_eq!(
        Library::default().home_shelf(4),
        crate::home::Shelf::Waiting
    );
}

#[test]
fn a_signed_in_library_waits_for_the_rootlist() {
    let mut library = Library::default();
    library.apply(&Update::SignedIn);
    assert_eq!(library.playlists, Load::Loading);
    assert_eq!(library.home_shelf(4), crate::home::Shelf::Waiting);
}

#[test]
fn a_failed_rootlist_is_unreadable_with_the_other_sources_still_idle() {
    let mut library = Library::default();
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.saved_albums, Load::Idle, "albums were never asked");
    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Unreadable(Failure::Load),
        "an unasked source must not keep the page waiting"
    );
}

#[test]
fn empty_lists_are_the_day_one_page() {
    assert_eq!(
        settled(Vec::new(), Vec::new(), Vec::new()).home_shelf(4),
        crate::home::Shelf::DayOne
    );
}

#[test]
fn an_empty_library_with_a_failed_albums_read_is_incomplete() {
    let mut library = settled(Vec::new(), Vec::new(), Vec::new());
    library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));

    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Incomplete(Failure::SavedAlbums)
    );
}

#[test]
fn undated_saves_are_quiet() {
    assert_eq!(
        settled(
            vec![playlist("spotify:playlist:a", "A")],
            Vec::new(),
            Vec::new()
        )
        .home_shelf(4),
        crate::home::Shelf::Quiet
    );
}

#[test]
fn a_dated_playlist_draws_even_with_a_failed_albums_read() {
    let mut library = settled(
        vec![dated_playlist("spotify:playlist:a", 5_000)],
        Vec::new(),
        Vec::new(),
    );
    library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));

    match library.home_shelf(1) {
        crate::home::Shelf::Cards(cards) => assert_eq!(cards.len(), 1),
        other => panic!("expected cards, got {other:?}"),
    }
}

/// A Home library whose three sources have all answered, holding one dated
/// liked song whose label nobody has asked for yet.
fn home_with_a_liked_song() -> Library {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:t", 5_000)]));
    library.apply(&Update::SavedAlbums(Vec::new()));
    library
}

#[test]
fn starting_home_opens_on_home_with_nowhere_behind_it() {
    let library = Library::starting_home(1_700_000_000_000);

    assert_eq!(library.page(), Some(&Page::Home));
    assert!(!library.can_back());
    assert_eq!(
        Library::default().page(),
        None,
        "a default library still opens on the root"
    );
}

#[test]
fn home_on_home_keeps_the_draw_and_a_new_page_reseeds() {
    let mut library = Library::default();
    library.go_home(1_700_000_000_000);
    let seed = library.home.seed();

    library.go_home(1_700_000_001_000);
    assert_eq!(library.home.seed(), seed, "Home on Home changes nothing");

    library.open(Page::Album("spotify:album:a".to_owned()));
    library.go_home(1_700_000_002_000);
    assert_eq!(library.page(), Some(&Page::Home));
    assert_ne!(library.home.seed(), seed, "a new open is a new draw");
}

#[test]
fn back_and_forward_keep_the_seed() {
    let mut library = Library::default();
    library.go_home(1_700_000_000_000);
    let seed = library.home.seed();

    library.open(Page::Album("spotify:album:a".to_owned()));
    library.back();
    assert_eq!(library.page(), Some(&Page::Home));
    assert_eq!(library.home.seed(), seed, "Back onto Home keeps the draw");

    library.forward();
    library.back();
    assert_eq!(library.home.seed(), seed, "a later Back keeps it too");
}

#[test]
fn going_home_drops_a_failed_page_load() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    library.go_home(1_700_000_000_000);

    assert_eq!(library.tracks, Load::Idle);
    assert_eq!(library.wanted, None);
}

/// Every pending source on Home is one Home guarantees a read for, whatever
/// chip the root was left on: a narrowed chip must not leave the shelf waiting
/// for an ask nobody makes.
#[test]
fn home_asks_for_the_albums_and_the_likes_under_every_chip() {
    for kind in crate::library_filter::ROW {
        let mut library = Library::starting_home(1_700_000_000_000);
        library.root_filter.choose(kind);
        library.apply(&Update::Playlists(Vec::new()));

        assert_eq!(
            library.saved_albums_request(),
            Some(Command::LoadSavedAlbums),
            "{kind:?}"
        );
        assert_eq!(
            library.likes_request(),
            Some(Command::LoadLikedTracks),
            "{kind:?}"
        );
    }
}

#[test]
fn a_failed_rootlist_asks_for_nothing_and_says_it_is_unreadable() {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.saved_albums_request(), None);
    assert_eq!(library.likes_request(), None);
    assert_eq!(library.home_labels_request(), None);
    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Unreadable(Failure::Load)
    );
}

#[test]
fn retrying_a_failed_rootlist_waits_again() {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Unreadable(Failure::Load)
    );

    library.reload();

    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Waiting,
        "the retry is a read in flight, not another failure"
    );
}

#[test]
fn home_asks_for_a_label_once_per_uri() {
    let mut library = home_with_a_liked_song();

    assert_eq!(
        library.home_labels_request(),
        Some(Command::LabelSaved {
            tracks: vec!["spotify:track:t".to_owned()],
        })
    );
    assert_eq!(
        library.home_labels_request(),
        None,
        "a URI is asked for once"
    );
    assert!(library.home.asked("spotify:track:t"));
}

#[test]
fn home_asks_for_no_label_until_every_source_has_settled() {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:t", 5_000)]));

    assert!(
        matches!(library.home_intake().albums, crate::home::Phase::Pending),
        "arrange: the albums are still unasked"
    );
    assert_eq!(
        library.home_labels_request(),
        None,
        "an unsettled pool could reshuffle under the labels"
    );

    library.apply(&Update::SavedAlbums(Vec::new()));

    assert_eq!(
        library.home_labels_request(),
        Some(Command::LabelSaved {
            tracks: vec!["spotify:track:t".to_owned()],
        })
    );
}

#[test]
fn home_asks_for_no_label_away_from_home() {
    let mut library = home_with_a_liked_song();
    library.to_playlists();

    assert_eq!(library.home_labels_request(), None);
}

#[test]
fn a_failed_label_read_is_incomplete_and_never_quiet() {
    let mut library = home_with_a_liked_song();
    assert!(
        library.home_labels_request().is_some(),
        "arrange: the label is owed"
    );

    library.apply(&Update::SavedLabelFailed {
        asked: vec!["spotify:track:t".to_owned()],
        failure: Failure::Load,
    });

    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Incomplete(Failure::Load),
        "a failed label read is not a quiet library"
    );
    assert_eq!(
        library.home_labels_request(),
        None,
        "the failed URI is not asked for again on its own"
    );
}

#[test]
fn a_retry_after_a_failed_label_read_asks_for_the_labels_again() {
    let mut library = home_with_a_liked_song();
    assert!(
        library.home_labels_request().is_some(),
        "arrange: the label is owed"
    );
    library.apply(&Update::SavedLabelFailed {
        asked: vec!["spotify:track:t".to_owned()],
        failure: Failure::Load,
    });
    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Incomplete(Failure::Load),
        "arrange: the shelf names the failure"
    );

    library.reload();

    assert_eq!(
        library.home_shelf(4),
        crate::home::Shelf::Waiting,
        "the retry is a read in flight, not the old failure"
    );
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::SavedAlbums(Vec::new()));
    assert_eq!(
        library.home_labels_request(),
        Some(Command::LabelSaved {
            tracks: vec!["spotify:track:t".to_owned()],
        }),
        "the retry asks for the label again"
    );
}
