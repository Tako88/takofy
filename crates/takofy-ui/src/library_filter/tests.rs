use super::*;

fn playlist(uri: &str) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn show(uri: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

#[test]
fn the_row_is_the_five_reference_chips_in_order() {
    assert_eq!(
        ROW,
        [
            RootKind::All,
            RootKind::Playlists,
            RootKind::Artists,
            RootKind::Albums,
            RootKind::Podcasts,
        ]
    );
}

#[test]
fn every_chip_names_a_list_the_root_can_draw() {
    for kind in ROW {
        assert_eq!(
            kind.acts(),
            availability(kind.control()) == Availability::Ready,
            "{kind:?}"
        );
        assert!(kind.acts(), "{kind:?} has a list behind it");
    }
}

#[test]
fn any_chip_takes_the_choice() {
    for kind in ROW {
        let mut filter = RootFilter::default();
        assert!(filter.choose(kind), "{kind:?}");
        assert_eq!(filter.chosen(), kind, "{kind:?}");
    }
}

fn artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        year: None,
        cover: None,
        tracks: None,
    }
}

#[test]
fn all_shows_every_kind_in_the_chips_order() {
    let playlists = vec![playlist("p")];
    let shows = vec![show("s")];
    let artists = vec![artist("a")];
    let albums = vec![album("b")];
    let items = items(RootKind::All, &playlists, &shows, &artists, &albums);
    assert_eq!(items[0], Item::Playlist(&playlist("p")));
    assert_eq!(items[1], Item::Show(&show("s")));
    assert_eq!(items[2], Item::Artist(&artist("a")));
    assert_eq!(items[3], Item::Album(&album("b")));
    assert_eq!(items.len(), 4, "a kind was left out of All");
}

#[test]
fn playlists_narrows_to_playlists() {
    let playlists = vec![playlist("p")];
    let shows = vec![show("s")];
    let artists = vec![artist("a")];
    let albums = vec![album("b")];
    let items = items(RootKind::Playlists, &playlists, &shows, &artists, &albums);
    assert_eq!(items, vec![Item::Playlist(&playlist("p"))]);
}

#[test]
fn podcasts_narrows_to_shows() {
    let playlists = vec![playlist("p")];
    let shows = vec![show("s")];
    let artists = vec![artist("a")];
    let albums = vec![album("b")];
    let items = items(RootKind::Podcasts, &playlists, &shows, &artists, &albums);
    assert_eq!(items, vec![Item::Show(&show("s"))]);
}

#[test]
fn artists_and_albums_narrow_to_their_own_cards() {
    let playlists = vec![playlist("p")];
    let shows = vec![show("s")];
    let artists = vec![artist("a"), artist("a2")];
    let albums = vec![album("b")];

    let picked = items(RootKind::Artists, &playlists, &shows, &artists, &albums);
    assert_eq!(
        picked,
        vec![Item::Artist(&artist("a")), Item::Artist(&artist("a2"))]
    );

    let picked = items(RootKind::Albums, &playlists, &shows, &artists, &albums);
    assert_eq!(picked, vec![Item::Album(&album("b"))]);
}
