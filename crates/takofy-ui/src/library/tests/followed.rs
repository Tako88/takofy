use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

fn related_artist(uri: &str, related: Vec<String>) -> ArtistDetail {
    ArtistDetail {
        related,
        ..artist_detail(uri)
    }
}

#[test]
fn followed_starts_idle() {
    assert_eq!(Library::default().followed, Load::Idle);
}

#[test]
fn followed_request_waits_for_ready_playlists() {
    let mut library = Library::default();
    assert_eq!(library.followed_request(), None, "nothing has loaded yet");

    library.apply(&Update::SignedIn);
    assert_eq!(library.followed_request(), None, "loading is not ready");

    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert_eq!(
        library.followed_request(),
        None,
        "a failed library never names a window"
    );
    assert_eq!(library.followed, Load::Idle);

    let mut ready = Library::default();
    ready.apply(&Update::Playlists(Vec::new()));
    assert_eq!(ready.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(ready.followed, Load::Loading);
}

#[test]
fn followed_request_fires_once() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(
        library.followed_request(),
        None,
        "an in-flight list is not asked for twice"
    );
}

#[test]
fn followed_request_is_none_while_loading_ready_or_failed() {
    for state in [
        Load::Loading,
        Load::Ready(vec![followed_show("spotify:show:a")]),
        Load::Failed(Failure::Followed),
    ] {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.followed = state.clone();

        assert_eq!(library.followed_request(), None, "{state:?} asked again");
        assert_eq!(library.followed, state);
    }
}

#[test]
fn a_followed_reply_is_recorded_whatever_the_page() {
    let shows = vec![followed_show("spotify:show:a")];
    let mut library = Library::default();
    library.apply(&Update::FollowedShows(shows.clone()));
    assert_eq!(library.followed, Load::Ready(shows.clone()));

    library.open(show("spotify:show:b"));
    library.apply(&Update::FollowedShows(shows.clone()));
    assert_eq!(
        library.followed,
        Load::Ready(shows),
        "a show page does not gate the list"
    );

    library.apply(&Update::FollowedShows(Vec::new()));
    assert_eq!(library.followed, Load::Ready(Vec::new()));
}

#[test]
fn a_followed_failure_is_recorded() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::FollowedShowsFailed(Failure::Followed));

    assert_eq!(library.followed, Load::Failed(Failure::Followed));
    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("a", "A")]),
        "the playlists beside it still draw"
    );
}

#[test]
fn reload_resets_followed_so_it_refetches() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:a",
    )]));

    assert_eq!(library.reload(), Command::ReloadLibrary);
    assert_eq!(library.followed, Load::Idle, "a playlist retry re-arms it");
    assert_eq!(
        library.followed_request(),
        None,
        "the playlists are loading again, so the list waits"
    );

    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
}

#[test]
fn reload_followed_marks_loading_and_returns_the_command() {
    let mut library = Library::default();
    library.apply(&Update::FollowedShowsFailed(Failure::Followed));

    assert_eq!(library.reload_followed(), Command::LoadFollowedShows);
    assert_eq!(library.followed, Load::Loading);

    let mut ready = Library::default();
    ready.apply(&Update::FollowedShows(Vec::new()));
    assert_eq!(ready.reload_followed(), Command::LoadFollowedShows);
    assert_eq!(ready.followed, Load::Loading);
}

#[test]
fn reload_while_followed_is_loading_accepts_either_reply() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(library.followed, Load::Loading);

    // A playlist retry while the first load is still in flight re-arms the
    // list, so a second load is issued and two replies are in the air.
    library.reload();
    assert_eq!(library.followed, Load::Idle);
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    assert_eq!(
        library.followed_request(),
        Some(Command::LoadFollowedShows),
        "the second load is issued while the first is still in flight"
    );

    let first = vec![followed_show("spotify:show:a")];
    let second = vec![followed_show("spotify:show:b")];
    library.apply(&Update::FollowedShows(first.clone()));
    assert_eq!(library.followed, Load::Ready(first));
    library.apply(&Update::FollowedShows(second.clone()));
    assert_eq!(
        library.followed,
        Load::Ready(second),
        "the later whole-set reply wins and neither corrupts the state"
    );
}

/// The likeness cards are the artist page's second read: nothing is asked
/// until the artist's own detail names somebody, and then it is asked once.
#[test]
fn the_related_request_waits_for_the_artist_and_fires_once() {
    let mut library = Library::default();
    assert_eq!(library.related_request(), None, "no artist page is open");

    // Ready with nobody to draw: no request, and no section.
    library.artist = Load::Ready(related_artist("spotify:artist:1", Vec::new()));
    assert_eq!(library.related_request(), None, "nobody was named");

    library.artist = Load::Ready(related_artist(
        "spotify:artist:1",
        vec!["spotify:artist:2".to_owned(), "spotify:artist:3".to_owned()],
    ));
    assert_eq!(
        library.related_request(),
        Some(Command::LoadRelated {
            artist: "spotify:artist:1".to_owned(),
            uris: vec!["spotify:artist:2".to_owned(), "spotify:artist:3".to_owned()],
        })
    );
    assert_eq!(library.related, Load::Loading);
    assert_eq!(library.related_request(), None, "the load is in flight");
}

#[test]
fn a_related_reply_is_taken_only_for_the_artist_that_asked() {
    let mut library = Library {
        artist: Load::Ready(related_artist(
            "spotify:artist:1",
            vec!["spotify:artist:2".to_owned()],
        )),
        ..Library::default()
    };
    let _ = library.related_request();

    library.apply(&Update::Related {
        artist: "spotify:artist:9".to_owned(),
        cards: vec![followed_artist("spotify:artist:9")],
    });
    assert_eq!(library.related, Load::Loading, "another artist's cards");

    library.apply(&Update::Related {
        artist: "spotify:artist:1".to_owned(),
        cards: vec![followed_artist("spotify:artist:2")],
    });
    assert!(matches!(library.related, Load::Ready(_)));
}

#[test]
fn another_artist_page_starts_a_fresh_likeness_list() {
    let mut library = Library {
        artist: Load::Ready(related_artist(
            "spotify:artist:1",
            vec!["spotify:artist:2".to_owned()],
        )),
        ..Library::default()
    };
    let _ = library.related_request();
    library.apply(&Update::Related {
        artist: "spotify:artist:1".to_owned(),
        cards: vec![followed_artist("spotify:artist:2")],
    });

    // Opening another artist drops the first one's row.
    let _ = library.open(Page::Artist("spotify:artist:5".to_owned()));
    assert_eq!(library.related, Load::Idle);
    library.artist = Load::Ready(related_artist(
        "spotify:artist:5",
        vec!["spotify:artist:6".to_owned()],
    ));
    assert!(matches!(
        library.related_request(),
        Some(Command::LoadRelated { .. })
    ));
}

#[test]
fn the_library_draws_tiles_until_rows_are_chosen() {
    let mut library = Library::default();
    assert_eq!(library.layout, MediaLayout::Grid);

    library.layout = MediaLayout::List;
    // The choice survives a page, because the library outlives it.
    library.apply(&Update::SignedIn);
    let _ = library.select("spotify:playlist:a");
    assert_eq!(library.layout, MediaLayout::List);
    library.back();
    assert_eq!(library.layout, MediaLayout::List, "back keeps the density");
}

#[test]
fn followed_artists_and_saved_albums_start_idle() {
    let library = Library::default();
    assert_eq!(library.followed_artists, Load::Idle);
    assert_eq!(library.saved_albums, Load::Idle);
}

#[test]
fn the_artist_and_album_requests_stay_silent_until_the_playlists_are_ready() {
    let mut library = Library::default();
    assert_eq!(
        library.followed_artists_request(),
        None,
        "nothing has loaded yet"
    );
    assert_eq!(
        library.saved_albums_request(),
        None,
        "nothing has loaded yet"
    );

    library.apply(&Update::SignedIn);
    assert_eq!(
        library.followed_artists_request(),
        None,
        "loading is not ready"
    );
    assert_eq!(library.saved_albums_request(), None, "loading is not ready");

    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert_eq!(
        library.followed_artists_request(),
        None,
        "a failed library never names a window"
    );
    assert_eq!(
        library.saved_albums_request(),
        None,
        "a failed library never names a window"
    );

    // All draws every kind, so a ready library under it asks for both.
    let mut ready = Library::default();
    ready.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        ready.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert_eq!(ready.saved_albums_request(), Some(Command::LoadSavedAlbums));

    // A chip that draws only playlists names neither list, so both stay
    // idle until one is chosen.
    let mut narrowed = Library::default();
    narrowed.apply(&Update::Playlists(Vec::new()));
    narrowed.root_filter.choose(RootKind::Playlists);
    assert_eq!(narrowed.followed_artists_request(), None);
    assert_eq!(narrowed.saved_albums_request(), None);
    assert_eq!(narrowed.followed_artists, Load::Idle);
    assert_eq!(narrowed.saved_albums, Load::Idle);
}

/// The chip half of the two requests, tested where it can be reached: no
/// chip naming artists or albums can be pressed until the controls act, so
/// the condition is a function of the two chosen kinds.
#[test]
fn a_chip_naming_the_list_or_all_asks_for_it() {
    for root in crate::library_filter::ROW {
        for sidebar in crate::sidebar_kind::ROW {
            let all = root == RootKind::All;
            assert_eq!(
                artists_chosen(root, sidebar),
                all || root == RootKind::Artists || sidebar == SidebarKind::Artists,
                "artists at {root:?} / {sidebar:?}"
            );
            assert_eq!(
                albums_chosen(root, sidebar),
                all || root == RootKind::Albums || sidebar == SidebarKind::Albums,
                "albums at {root:?} / {sidebar:?}"
            );
        }
    }

    // Either surface names the list, so a root Artists chip beside a
    // sidebar Albums chip still asks for both lists.
    assert!(artists_chosen(RootKind::Artists, SidebarKind::Artists));
    assert!(artists_chosen(RootKind::Artists, SidebarKind::Albums));
    assert!(!artists_chosen(RootKind::Albums, SidebarKind::Albums));
    assert!(!albums_chosen(RootKind::Podcasts, SidebarKind::Playlists));
}

/// The state half of the two requests. The chip half cannot be exercised
/// here, so this pins only that a list already loading, ready or failed is
/// never asked for again.
#[test]
fn a_list_that_already_has_an_answer_is_never_asked_again() {
    let artist_states: [Load<Vec<FollowedArtist>>; 3] = [
        Load::Loading,
        Load::Ready(vec![followed_artist("spotify:artist:a")]),
        Load::Failed(Failure::FollowedArtists),
    ];
    for state in artist_states {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.followed_artists = state.clone();

        assert_eq!(
            library.followed_artists_request(),
            None,
            "{state:?} asked again"
        );
        assert_eq!(library.followed_artists, state);
    }

    let album_states: [Load<Vec<SavedAlbum>>; 3] = [
        Load::Loading,
        Load::Ready(vec![saved_album("spotify:album:a")]),
        Load::Failed(Failure::SavedAlbums),
    ];
    for state in album_states {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.saved_albums = state.clone();

        assert_eq!(
            library.saved_albums_request(),
            None,
            "{state:?} asked again"
        );
        assert_eq!(library.saved_albums, state);
    }
}

#[test]
fn an_artist_list_reply_is_recorded_whatever_the_page() {
    let artists = vec![followed_artist("spotify:artist:a")];
    let mut library = Library::default();
    library.apply(&Update::FollowedArtists(artists.clone()));
    assert_eq!(library.followed_artists, Load::Ready(artists.clone()));

    library.open(show("spotify:show:b"));
    library.apply(&Update::FollowedArtists(artists.clone()));
    assert_eq!(
        library.followed_artists,
        Load::Ready(artists),
        "a show page does not gate the list"
    );

    library.apply(&Update::FollowedArtists(Vec::new()));
    assert_eq!(library.followed_artists, Load::Ready(Vec::new()));
}

#[test]
fn a_saved_album_reply_is_recorded_whatever_the_page() {
    let albums = vec![saved_album("spotify:album:a")];
    let mut library = Library::default();
    library.apply(&Update::SavedAlbums(albums.clone()));
    assert_eq!(library.saved_albums, Load::Ready(albums.clone()));

    library.open(artist("spotify:artist:b"));
    library.apply(&Update::SavedAlbums(albums.clone()));
    assert_eq!(
        library.saved_albums,
        Load::Ready(albums),
        "an artist page does not gate the list"
    );

    library.apply(&Update::SavedAlbums(Vec::new()));
    assert_eq!(library.saved_albums, Load::Ready(Vec::new()));
}

#[test]
fn an_artist_list_failure_is_recorded() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));

    assert_eq!(
        library.followed_artists,
        Load::Failed(Failure::FollowedArtists)
    );
    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("a", "A")]),
        "the playlists beside it still draw"
    );
}

#[test]
fn a_saved_album_failure_is_recorded() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));

    assert_eq!(library.saved_albums, Load::Failed(Failure::SavedAlbums));
    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("a", "A")]),
        "the playlists beside it still draw"
    );
}

#[test]
fn reload_rearms_the_artist_and_album_lists() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
    )]));
    library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:a")]));

    assert_eq!(library.reload(), Command::ReloadLibrary);
    assert_eq!(
        library.followed_artists,
        Load::Idle,
        "a playlist retry re-arms it"
    );
    assert_eq!(
        library.saved_albums,
        Load::Idle,
        "a playlist retry re-arms it"
    );
}

#[test]
fn reload_followed_artists_clears_the_stale_state_and_returns_the_command() {
    let mut failed = Library::default();
    failed.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
    assert_eq!(
        failed.reload_followed_artists(),
        Command::LoadFollowedArtists
    );
    assert_eq!(failed.followed_artists, Load::Loading);

    let mut loading = Library {
        followed_artists: Load::Loading,
        ..Library::default()
    };
    assert_eq!(
        loading.reload_followed_artists(),
        Command::LoadFollowedArtists
    );
    assert_eq!(
        loading.followed_artists,
        Load::Loading,
        "a stale load is dropped and asked again"
    );

    let mut ready = Library::default();
    ready.apply(&Update::FollowedArtists(Vec::new()));
    assert_eq!(
        ready.reload_followed_artists(),
        Command::LoadFollowedArtists
    );
    assert_eq!(ready.followed_artists, Load::Loading);
}

#[test]
fn reload_saved_albums_clears_the_stale_state_and_returns_the_command() {
    let mut failed = Library::default();
    failed.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
    assert_eq!(failed.reload_saved_albums(), Command::LoadSavedAlbums);
    assert_eq!(failed.saved_albums, Load::Loading);

    let mut loading = Library {
        saved_albums: Load::Loading,
        ..Library::default()
    };
    assert_eq!(loading.reload_saved_albums(), Command::LoadSavedAlbums);
    assert_eq!(
        loading.saved_albums,
        Load::Loading,
        "a stale load is dropped and asked again"
    );

    let mut ready = Library::default();
    ready.apply(&Update::SavedAlbums(Vec::new()));
    assert_eq!(ready.reload_saved_albums(), Command::LoadSavedAlbums);
    assert_eq!(ready.saved_albums, Load::Loading);
}
