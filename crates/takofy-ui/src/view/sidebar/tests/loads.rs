use super::super::*;
use super::test_support::*;

#[test]
fn a_failed_playlist_load_wins_over_a_ready_followed_list() {
    let mut library = Library::default();
    library.playlists = Load::Failed(Failure::Load);
    library.followed = Load::Ready(vec![followed_show("spotify:show:a", "A")]);

    assert_eq!(
        shows_view(&library),
        ListView::Failed(Failure::Load, Retry::Playlists)
    );
}

#[test]
fn an_unstarted_or_inflight_playlist_load_is_loading() {
    for playlists in [Load::Idle, Load::Loading] {
        let load = format!("{playlists:?}");
        let mut library = Library::default();
        library.playlists = playlists;
        assert_eq!(shows_view(&library), ListView::Loading, "{load}");
    }
}

#[test]
fn a_reload_does_not_flash_the_empty_state() {
    // The retry reset the followed list to Idle while the playlists load.
    let mut library = Library::default();
    library.playlists = Load::Loading;
    library.followed = Load::Idle;
    assert_eq!(shows_view(&library), ListView::Loading);

    // One beat later the playlists have landed, but the list has not been
    // asked for yet.
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    library.followed = Load::Idle;
    assert_eq!(shows_view(&library), ListView::Loading);
}

#[test]
fn an_inflight_followed_list_is_loading() {
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    library.followed = Load::Loading;
    assert_eq!(shows_view(&library), ListView::Loading);
}

#[test]
fn a_failed_followed_list_carries_its_own_retry() {
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    library.followed = Load::Failed(Failure::Followed);

    assert_eq!(
        shows_view(&library),
        ListView::Failed(Failure::Followed, Retry::Followed)
    );
}

#[test]
fn a_ready_followed_list_is_empty_when_it_follows_nothing() {
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    library.followed = Load::Ready(Vec::new());
    assert_eq!(shows_view(&library), ListView::Empty);
}

#[test]
fn a_ready_followed_list_is_the_shows_to_draw() {
    let shows = vec![
        followed_show("spotify:show:a", "A"),
        followed_show("spotify:show:b", "B"),
    ];
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    library.followed = Load::Ready(shows.clone());

    assert_eq!(shows_view(&library), ListView::List(shows.as_slice()));
}

#[test]
fn a_failed_artists_or_albums_load_carries_its_own_retry() {
    let mut artists = Library::default();
    artists.playlists = Load::Ready(Vec::new());
    artists.followed_artists = Load::Failed(Failure::FollowedArtists);
    assert_eq!(
        artists_view(&artists),
        ListView::Failed(Failure::FollowedArtists, Retry::FollowedArtists)
    );

    let mut albums = Library::default();
    albums.playlists = Load::Ready(Vec::new());
    albums.saved_albums = Load::Failed(Failure::SavedAlbums);
    assert_eq!(
        albums_view(&albums),
        ListView::Failed(Failure::SavedAlbums, Retry::SavedAlbums)
    );
}

#[test]
fn an_unasked_artists_or_albums_list_is_loading_not_empty() {
    // Idle is "nobody has asked yet", which reads as loading: the empty
    // copy would claim the library holds nothing.
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    assert_eq!(artists_view(&library), ListView::Loading);
    assert_eq!(albums_view(&library), ListView::Loading);

    library.followed_artists = Load::Ready(Vec::new());
    library.saved_albums = Load::Ready(Vec::new());
    assert_eq!(artists_view(&library), ListView::Empty);
    assert_eq!(albums_view(&library), ListView::Empty);
}

#[test]
fn the_size_counts_the_two_new_lists() {
    let mut library = Library::default();
    library.playlists = Load::Ready(vec![PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: "spotify:playlist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]);
    library.followed = Load::Ready(vec![followed_show("spotify:show:a", "A")]);
    library.followed_artists = Load::Ready(vec![
        followed_artist("spotify:artist:a", "A"),
        followed_artist("spotify:artist:b", "B"),
    ]);
    library.saved_albums = Load::Ready(vec![saved_album("spotify:album:a", "A", "X")]);

    assert_eq!(counts(&library), (1, 5), "one shown of five saved");

    library.sidebar_kind.choose(SidebarKind::Artists);
    assert_eq!(counts(&library), (2, 5));
    library.sidebar_kind.choose(SidebarKind::Albums);
    assert_eq!(counts(&library), (1, 5));
}

#[test]
fn an_artist_row_says_its_kind_and_an_album_row_names_its_artist() {
    assert_eq!(followed_artist("spotify:artist:a", "A").kind(), "Artist");
    assert_eq!(
        saved_album("spotify:album:a", "A", "Vela Moreno").kind(),
        "Album · Vela Moreno"
    );
    assert_eq!(
        saved_album("spotify:album:a", "A", "").kind(),
        "Album",
        "an album the wire named no artist for says only its kind"
    );
}
