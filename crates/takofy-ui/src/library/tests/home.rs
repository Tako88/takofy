use super::super::*;
use super::test_support::*;
use crate::home::{Budget, Slot};
use std::time::Instant;
use takofy_model::home::{HomeAnswer, HomeAsk, Station, StationSeed};
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

#[test]
fn retrying_one_shelf_keeps_a_healthy_likes_read() {
    let mut library = settled(Vec::new(), Vec::new(), Vec::new());
    assert_eq!(library.likes.phase(), crate::home::Phase::Arrived);
    assert_eq!(
        library.likes_request(),
        None,
        "arrange: the liked read has settled"
    );

    assert_eq!(library.retry_home(Slot::Counted), None);

    assert_eq!(
        library.likes_request(),
        None,
        "the retry re-read every like from a healthy read"
    );
    assert_eq!(library.likes.phase(), crate::home::Phase::Arrived);
}

#[test]
fn retrying_one_shelf_keeps_another_ready_and_its_budget() {
    let mut library = settled(
        vec![dated_playlist("spotify:playlist:a", 5_000)],
        Vec::new(),
        vec![liked("spotify:track:t", 4_000)],
    );
    library.apply(&Update::SavedLabelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: vec![labelled("spotify:track:t")],
    });
    let before = drawn_uris(&library.home_shelf(4));
    assert_eq!(before.len(), 2, "arrange: the dated shelf draws");
    for _ in 0..crate::home::Budget::per_open(Slot::Counted) {
        assert!(library.home.budget.take(Slot::Counted));
    }

    assert_eq!(library.retry_home(Slot::Episodes), None);

    assert_eq!(
        drawn_uris(&library.home_shelf(4)),
        before,
        "the retried shelf dropped another shelf's cards"
    );
    assert!(
        library.home.answered("spotify:track:t"),
        "the retried shelf dropped another shelf's label"
    );
    assert_eq!(
        library.home.budget.left(Slot::Counted),
        0,
        "the retried shelf returned another shelf's read"
    );
    assert_eq!(
        library.home.budget.left(Slot::Episodes),
        Budget::per_open(Slot::Episodes),
        "the retried shelf got its allowance back"
    );
}

#[test]
fn a_counted_label_answer_leaves_the_dated_shelf_alone() {
    let mut library = home_with_a_liked_song();
    library.apply(&Update::SavedLabelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: vec![labelled("spotify:track:t")],
    });
    let before = drawn_uris(&library.home_shelf(4));
    assert_eq!(before, vec!["spotify:track:t".to_owned()]);

    let mut unusable = track("spotify:track:t");
    unusable.named = false;
    library.apply(&Update::Home(HomeAnswer::Labelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: vec![unusable],
    }));

    assert_eq!(
        drawn_uris(&library.home_shelf(4)),
        before,
        "the counted answer replaced the dated shelf's label"
    );
    assert_eq!(
        library
            .home
            .label("spotify:track:t")
            .map(|track| track.named),
        Some(true),
        "the dated shelf kept the label its own read landed"
    );
}

#[test]
fn a_failed_counted_label_read_leaves_the_dated_shelf_alone() {
    let mut library = home_with_a_liked_song();
    library.apply(&Update::SavedLabelled {
        asked: vec!["spotify:track:t".to_owned()],
        named: vec![labelled("spotify:track:t")],
    });
    let before = drawn_uris(&library.home_shelf(4));
    assert_eq!(before, vec!["spotify:track:t".to_owned()]);

    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Labels {
            tracks: vec!["spotify:track:t".to_owned()],
        },
        failure: Failure::Load,
    });

    assert_eq!(
        drawn_uris(&library.home_shelf(4)),
        before,
        "the counted failure changed what the dated shelf draws"
    );
    assert_eq!(
        library.home.failed(),
        None,
        "the dated shelf has no failure of its own"
    );
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

/// The label a liked song's own read lands.
fn labelled(uri: &str) -> TrackEntry {
    TrackEntry {
        album_uri: Some("spotify:album:r".to_owned()),
        ..track(uri)
    }
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

/// A Home whose rootlist and followed lists have answered, with both remaning
/// dated feeds asked and left in flight.
fn feeds_out() -> Library {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums),
        "arrange: the albums are out"
    );
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the likes are out"
    );
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
    )]));
    library.apply(&Update::FollowedArtists(Vec::new()));
    library
}

/// The number of `LoadHome` asks in a frame's asks.
fn built_asks(asks: &[Command]) -> usize {
    asks.iter()
        .filter(|ask| matches!(ask, Command::LoadHome(_)))
        .count()
}

#[test]
fn the_dated_label_leads() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        vec![liked("spotify:track:t", 5_000)],
    );
    library.go_home(1_700_000_000_000);
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
    )]));

    let asks = library.home_asks(Instant::now());

    assert!(
        matches!(asks.first(), Some(Command::LabelSaved { tracks }) if tracks == &vec!["spotify:track:t".to_owned()]),
        "the dated shelf's own read goes out first: {asks:?}"
    );
    assert!(
        built_asks(&asks) > 0,
        "the built shelves are asked once their feeds are ready: {asks:?}"
    );
    assert!(!library.home_held());
}

/// A Home whose every shelf's feeds have answered with one candidate each, and
/// whose one liked song nobody has labelled yet.
fn every_shelf_ready() -> Library {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::Playlists(vec![dated_playlist(
        "spotify:playlist:a",
        5_000,
    )]));
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums),
        "arrange: the albums are out"
    );
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the likes are out"
    );
    library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:r")]));
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:t", 4_000)]));
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
    )]));
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
    )]));
    library
}

/// A station answer for `seed`, with rows the band can draw.
fn station_for(seed: &StationSeed) -> Station {
    Station {
        seed: seed.clone(),
        uri: format!("spotify:station:{}", seed.uri),
        title: seed.uri.clone(),
        subtitle: None,
        art: None,
        rows: vec![
            track(&format!("{}:one", seed.uri)),
            track(&format!("{}:two", seed.uri)),
        ],
    }
}

/// The `LoadHome` asks one frame made, in the order the frame made them.
fn load_home_asks(asks: &[Command]) -> Vec<HomeAsk> {
    asks.iter()
        .filter_map(|ask| match ask {
            Command::LoadHome(ask) => Some(ask.clone()),
            _ => None,
        })
        .collect()
}

/// Asks every frame until the library owes nothing more, appending the
/// `LoadHome` asks it made.
fn drain_asks(library: &mut Library, into: &mut Vec<HomeAsk>) {
    for _ in 0..=Budget::allowance() {
        let frame = load_home_asks(&library.home_asks(Instant::now()));
        if frame.is_empty() {
            return;
        }
        into.extend(frame);
    }
}

/// The shelf a built ask names, panicking on one that only decorates.
fn built_slot(ask: &HomeAsk, liked: &str) -> Slot {
    match ask {
        HomeAsk::Station { .. } => Slot::Stations,
        HomeAsk::Related { .. } => Slot::Because,
        HomeAsk::Episodes { .. } => Slot::Episodes,
        HomeAsk::Labels { tracks } if tracks.len() == 1 && tracks[0] == liked => Slot::Counted,
        HomeAsk::Labels { tracks } => panic!("a labels ask outside the shelves: {tracks:?}"),
        HomeAsk::Portraits { artists } => panic!("a decoration asked first: {artists:?}"),
    }
}

#[test]
fn one_frame_carries_every_built_ask_it_owes() {
    let mut library = every_shelf_ready();
    let before = library.home_asks(Instant::now());

    let after = library.home_asks(Instant::now());

    assert!(
        built_asks(&before) >= 2,
        "the frame that owes several asks sends them together, or the worker's places idle: {before:?}"
    );
    assert!(
        built_asks(&after) < built_asks(&before),
        "what a frame sent is not owed again by the next one: {after:?}"
    );
}

#[test]
fn the_built_asks_follow_the_page_order() {
    let mut library = every_shelf_ready();
    let liked = "spotify:track:t";
    let mut asks: Vec<HomeAsk> = Vec::new();

    drain_asks(&mut library, &mut asks);

    let mut order: Vec<Slot> = Vec::new();
    for ask in &asks {
        let slot = built_slot(ask, liked);
        if order.last() != Some(&slot) {
            order.push(slot);
        }
    }
    assert_eq!(
        order,
        Slot::ORDER[1..],
        "the shelves are asked in the page's order: {asks:?}"
    );

    for ask in &asks {
        if let HomeAsk::Station { seed } = ask {
            library.apply(&Update::Home(HomeAnswer::Station(station_for(seed))));
        }
    }
    library.apply(&Update::Home(HomeAnswer::Labelled {
        asked: vec![liked.to_owned()],
        named: vec![TrackEntry {
            artist_uris: vec!["spotify:artist:x".to_owned()],
            artist_names: vec!["X".to_owned()],
            ..track(liked)
        }],
    }));

    let shelves = asks.len();
    drain_asks(&mut library, &mut asks);

    assert!(
        matches!(
            &asks[shelves..],
            [HomeAsk::Labels { .. }, HomeAsk::Portraits { .. }]
        ),
        "the decorations are asked after every shelf: {:?}",
        &asks[shelves..]
    );
}

#[test]
fn built_asks_wait_for_the_dated_feeds() {
    let mut library = feeds_out();
    let t0 = Instant::now();

    assert!(library.home_asks(t0).is_empty());
    assert!(library.home_held());
}

#[test]
fn a_hung_feed_does_not_stall_the_built_shelves() {
    let mut library = feeds_out();
    library.apply(&Update::SavedAlbums(Vec::new()));
    let t0 = Instant::now();

    assert!(
        library.home_asks(t0).is_empty(),
        "arrange: the hold is armed"
    );

    let asks = library.home_asks(t0 + HOME_HOLD);

    assert!(
        matches!(
            asks.as_slice(),
            [Command::LoadHome(HomeAsk::Episodes { .. })]
        ),
        "the built ask goes out once the bound passes: {asks:?}"
    );
    assert!(
        !asks
            .iter()
            .any(|ask| matches!(ask, Command::LabelSaved { .. })),
        "a hung like has no label to send"
    );
    assert!(!library.home_held());
}

#[test]
fn the_hold_is_armed_once_and_not_by_later_frames() {
    let mut library = feeds_out();
    let t0 = Instant::now();

    assert!(
        library.home_asks(t0).is_empty(),
        "arrange: the hold is armed"
    );
    assert!(
        library.home_asks(t0 + HOME_HOLD / 2).is_empty(),
        "a later frame must not push the deadline out"
    );
    assert!(!library.home_asks(t0 + HOME_HOLD).is_empty());
}

#[test]
fn a_failed_feed_does_not_hold() {
    let mut library = feeds_out();
    library.apply(&Update::SavedAlbumsFailed(Failure::Load));
    library.apply(&Update::LikedTracks(Vec::new()));

    let asks = library.home_asks(Instant::now());

    assert_eq!(built_asks(&asks), 1, "{asks:?}");
    assert!(!library.home_held(), "a read that failed is not waited on");
}

#[test]
fn a_library_with_nothing_liked_does_not_hold() {
    let mut library = feeds_out();
    library.apply(&Update::LikedTracks(Vec::new()));

    let asks = library.home_asks(Instant::now());

    assert_eq!(
        built_asks(&asks),
        1,
        "no label read is coming, so nothing is worth waiting for: {asks:?}"
    );
    assert!(!library.home_held());
}

#[test]
fn a_rootlist_that_fails_after_the_hold_arms_holds_nothing() {
    let mut library = feeds_out();
    let t0 = Instant::now();
    assert!(
        library.home_asks(t0).is_empty(),
        "arrange: the hold is armed"
    );

    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert!(
        !library.home_held(),
        "a failed rootlist has no feed left to wait for"
    );
}

#[test]
fn likes_that_answer_empty_after_the_hold_arms_hold_nothing() {
    let mut library = feeds_out();
    let t0 = Instant::now();
    assert!(
        library.home_asks(t0).is_empty(),
        "arrange: the hold is armed"
    );

    library.apply(&Update::LikedTracks(Vec::new()));

    assert!(
        !library.home_held(),
        "no label read is coming, so nothing is worth waiting for"
    );
}

#[test]
fn a_failed_rootlist_asks_for_nothing() {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert!(library.home_asks(Instant::now()).is_empty());
    assert!(!library.home_held());
}

#[test]
fn away_from_home_nothing_is_asked() {
    let mut library = feeds_out();
    library.to_playlists();

    assert!(library.home_asks(Instant::now()).is_empty());
    assert!(!library.home_held());
}

#[test]
fn reopening_home_arms_the_hold_again() {
    let mut library = feeds_out();
    let t0 = Instant::now();
    assert!(
        library.home_asks(t0).is_empty(),
        "arrange: the hold is armed"
    );

    library.to_playlists();
    library.go_home(1_700_000_000_000);

    assert!(
        library.home_asks(t0 + HOME_HOLD).is_empty(),
        "a fresh visit waits its own bound"
    );
    assert!(library.home_held());
}

#[test]
fn retrying_the_dated_shelf_leads_with_its_label_again() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        vec![liked("spotify:track:t", 5_000)],
    );
    library.go_home(1_700_000_000_000);
    let t0 = Instant::now();
    assert!(
        library
            .home_asks(t0)
            .first()
            .is_some_and(|ask| matches!(ask, Command::LabelSaved { .. })),
        "arrange: the label leads"
    );

    library.retry_home(Slot::Dated);
    let asks = library.home_asks(t0);

    assert!(
        matches!(asks.first(), Some(Command::LabelSaved { .. })),
        "{asks:?}"
    );
}

#[test]
fn retrying_the_dated_shelf_waits_for_the_feed_it_re_reads() {
    let mut library = Library::starting_home(1_700_000_000_000);
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums),
        "arrange: the albums are out"
    );
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the likes are out"
    );
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:t", 5_000)]));
    library.apply(&Update::SavedAlbumsFailed(Failure::Load));

    library.retry_home(Slot::Dated);

    assert_eq!(
        library.saved_albums,
        Load::Idle,
        "the failed feed is re-read"
    );
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums)
    );
    assert!(
        library
            .home_asks(Instant::now())
            .iter()
            .all(|ask| !matches!(ask, Command::LabelSaved { .. })),
        "a label sent before the re-read lands would name a pooled set"
    );

    library.apply(&Update::SavedAlbums(Vec::new()));
    let asks = library.home_asks(Instant::now());

    assert!(
        matches!(asks.first(), Some(Command::LabelSaved { .. })),
        "the label leads once the feed lands: {asks:?}"
    );
}
