use super::super::*;
use super::test_support::*;

#[test]
fn back_to_a_ready_playlist_returns_no_command() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    library.open(album("album"));

    assert_eq!(library.back(), None);
    assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")]))
    );
}

#[test]
fn back_to_a_failed_playlist_loads_it_again() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });
    library.open(album("album"));

    assert_eq!(
        library.back(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert_eq!(library.tracks, Load::Loading);
    // The reload must not push the page it lands on.
    assert!(library.back().is_none(), "the root is the only entry left");
    assert!(!library.can_back());
}

#[test]
fn back_to_an_idle_playlist_loads_it() {
    let mut library = Library::default();
    library.select("a");
    library.open(album("album"));
    library.tracks = Load::Idle;

    assert_eq!(
        library.back(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn back_to_a_loading_playlist_returns_no_command() {
    let mut library = Library::default();
    library.select("a");
    library.open(album("album"));

    assert_eq!(library.back(), None);
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn back_from_the_root_loads_the_playlist() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    library.to_playlists();

    assert_eq!(
        library.back(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn a_reply_for_the_wanted_playlist_is_applied_on_an_album_page() {
    let mut library = Library::default();
    library.select("a");
    library.open(album("album"));

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(library.page(), Some(&album("album")));
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")]))
    );
}

#[test]
fn can_back_and_can_forward_track_the_moves() {
    let mut library = Library::default();
    assert!(!library.can_back());
    assert!(!library.can_forward());

    library.select("a");
    assert!(library.can_back());
    assert!(!library.can_forward());

    assert_eq!(
        library.open(album("spotify:album:rec")),
        Some(Command::LoadAlbum {
            album: "spotify:album:rec".to_owned()
        })
    );
    assert_eq!(
        library.back(),
        None,
        "back onto the playlist already wanted needs no load"
    );
    assert!(library.can_forward());

    assert_eq!(
        library.forward(),
        None,
        "forward onto the album already wanted needs no load"
    );
    assert!(!library.can_forward());
    assert_eq!(library.page(), Some(&album("spotify:album:rec")));
}

#[test]
fn a_prune_keeps_an_album_page_and_the_album_and_artist_entries() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    library.open(album("album"));
    library.open(artist("artist"));
    library.back();

    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(library.page(), Some(&album("album")));
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")]))
    );
    assert_eq!(library.back(), None);
    assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
    assert_eq!(library.forward(), None);
    assert_eq!(library.page(), Some(&album("album")));
    assert_eq!(library.forward(), None);
    assert_eq!(library.page(), Some(&artist("artist")));
}

#[test]
fn a_prune_of_the_current_playlist_returns_to_the_root_without_duplicates() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert_eq!(library.page(), None);
    assert_eq!(library.selected(), None);
    assert_eq!(library.tracks, Load::Idle);
    assert_eq!(library.wanted, None);
    assert!(
        !library.can_back(),
        "the root must not be duplicated behind the root"
    );
}

#[test]
fn back_with_nowhere_to_go_does_not_reload() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.back(), None, "the root needs no command");
    assert_eq!(library.page(), None);
    assert_eq!(
        library.back(),
        None,
        "no page left behind the root, and no reload"
    );
    assert!(!library.can_back());
}

#[test]
fn forward_with_nowhere_to_go_does_not_reload_a_failed_playlist() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert!(!library.can_forward());
    assert_eq!(
        library.forward(),
        None,
        "a no-op Forward must not retry the page it is on"
    );
    assert_eq!(library.tracks, Load::Failed(Failure::Load));
    assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
}

#[test]
fn a_prune_with_no_wanted_load_clears_ready_tracks() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    library.to_playlists();
    assert_eq!(library.wanted, None);
    assert!(matches!(library.tracks, Load::Ready(_)));

    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert_eq!(
        library.tracks,
        Load::Idle,
        "the deleted playlist's ready tracks must be freed"
    );
}

#[test]
fn to_playlists_while_loading_leaves_no_spinner_at_the_root() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    assert_eq!(library.tracks, Load::Loading);

    library.to_playlists();

    assert_eq!(
        library.tracks,
        Load::Idle,
        "the root must not sit on a spinner nothing can settle"
    );
    assert_eq!(library.wanted, None);
}

#[test]
fn to_playlists_drops_a_failed_load() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    library.to_playlists();

    assert_eq!(library.tracks, Load::Idle);
    assert_eq!(library.wanted, None);
}
