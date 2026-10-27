use takofy_model::{AlbumDetail, ArtistDetail, PlaylistEntry, Show, ShowDetail};

use super::*;
use crate::library::Load;
use crate::message::{Failure, Update};
use crate::route::Page;

fn show_detail(uri: &str, name: &str) -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: uri.to_owned(),
            name: name.to_owned(),
            publisher: "Lateral".to_owned(),
            description: String::new(),
            cover: None,
        },
        episodes: Vec::new(),
    }
}

fn album_detail(uri: &str, name: &str) -> AlbumDetail {
    AlbumDetail {
        uri: uri.to_owned(),
        name: name.to_owned(),
        artists: "Band".to_owned(),
        artist_uri: None,
        year: Some(1997),
        cover: None,
        tracks: Vec::new(),
    }
}

fn artist_detail(uri: &str, name: &str) -> ArtistDetail {
    ArtistDetail {
        uri: uri.to_owned(),
        name: name.to_owned(),
        portrait: None,
        top_tracks: Vec::new(),
        releases: Vec::new(),
        related: Vec::new(),
    }
}

/// The current crumb's label.
fn current_label(library: &Library) -> Option<&str> {
    trail(library)
        .into_iter()
        .find(|crumb| crumb.current)
        .map(|crumb| crumb.label)
}

fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

/// The middle crumb names the list the page lives in, and leads back to the
/// root showing that list's chip.
#[test]
fn a_page_names_the_list_it_lives_in() {
    let mut selected = Library::default();
    selected.apply(&Update::Playlists(vec![playlist("a", "Mix")]));
    selected.select("a");

    for (page, kind, label) in [
        (
            Page::Album("spotify:album:a".to_owned()),
            RootKind::Albums,
            "Albums",
        ),
        (
            Page::Artist("spotify:artist:a".to_owned()),
            RootKind::Artists,
            "Artists",
        ),
        (
            Page::Show("spotify:show:a".to_owned()),
            RootKind::Podcasts,
            "Podcasts",
        ),
    ] {
        let mut library = Library::default();
        library.open(page);
        let crumbs = trail(&library);
        assert_eq!(crumbs[1].label, label, "{crumbs:?}");
        assert_eq!(crumbs[1].target, Some(Target::Root(kind)), "{crumbs:?}");
    }

    let crumbs = trail(&selected);
    assert_eq!(crumbs[1].label, "Playlists");
    assert_eq!(crumbs[1].target, Some(Target::Root(RootKind::Playlists)));
}

/// Home goes back to the root as it was; the list crumb goes back to the
/// root showing that list.
#[test]
fn home_keeps_the_chip_and_the_list_crumb_sets_it() {
    let mut library = Library::default();
    library.root_filter.choose(RootKind::Artists);
    library.open(Page::Album("spotify:album:a".to_owned()));

    let crumbs = trail(&library);
    assert_eq!(crumbs[0].target, Some(Target::Root(RootKind::Artists)));
    assert_eq!(crumbs[1].target, Some(Target::Root(RootKind::Albums)));
}

/// A search is not a list the root holds, so it has no middle crumb.
#[test]
fn a_search_page_has_no_list_crumb() {
    let mut library = Library::default();
    library.open(Page::Search("never gonna".to_owned()));

    assert_eq!(
        trail(&library),
        vec![
            Crumb {
                label: "Home",
                current: false,
                target: Some(Target::Root(RootKind::All)),
            },
            Crumb {
                label: "Search",
                current: true,
                target: None,
            },
        ]
    );
}

/// At the root there is nowhere to go back to.
#[test]
fn the_root_trail_has_nowhere_to_go() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "Mix")]));
    assert!(trail(&library).iter().all(|crumb| crumb.target.is_none()));
}

#[test]
fn the_root_trail_is_home_then_the_library() {
    let library = Library::default();
    assert_eq!(
        trail(&library),
        vec![
            Crumb {
                label: "Home",
                current: false,
                target: None,
            },
            Crumb {
                label: "Your Library",
                current: true,
                target: None,
            },
        ]
    );
}

#[test]
fn a_selection_gives_home_a_list_link_and_the_name() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "Mix")]));
    library.select("a");

    assert_eq!(
        trail(&library),
        vec![
            Crumb {
                label: "Home",
                current: false,
                target: Some(Target::Root(RootKind::All)),
            },
            Crumb {
                label: "Playlists",
                current: false,
                target: Some(Target::Root(RootKind::Playlists)),
            },
            Crumb {
                label: "Mix",
                current: true,
                target: None,
            },
        ]
    );
}

#[test]
fn a_selection_with_no_name_available_gives_playlist_none() {
    let mut library = Library::default();
    library.select("a");

    let crumbs = trail(&library);
    assert_eq!(crumbs.len(), 2);
    assert_eq!(crumbs[1].label, "Playlists");
    assert_eq!(crumbs[1].target, Some(Target::Root(RootKind::Playlists)));
    assert!(!crumbs.iter().any(|crumb| crumb.current));
}

#[test]
fn an_album_page_names_the_loaded_album() {
    let mut library = Library::default();
    library.open(Page::Album("spotify:album:a".to_owned()));
    library.apply(&Update::Album(album_detail(
        "spotify:album:a",
        "Signal Decay",
    )));

    assert_eq!(current_label(&library), Some("Signal Decay"));
}

#[test]
fn an_album_page_names_its_kind_while_loading() {
    let mut library = Library::default();
    library.open(Page::Album("spotify:album:a".to_owned()));

    assert_eq!(current_label(&library), Some("Album"));
}

#[test]
fn an_album_page_names_its_kind_after_a_failure() {
    let mut library = Library::default();
    library.open(Page::Album("spotify:album:a".to_owned()));
    library.apply(&Update::AlbumFailed {
        album: "spotify:album:a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(current_label(&library), Some("Album"));
}

#[test]
fn an_album_page_names_its_kind_for_another_uri() {
    let mut library = Library::default();
    library.open(Page::Album("spotify:album:b".to_owned()));
    library.album = Load::Ready(album_detail("spotify:album:a", "Signal Decay"));

    assert_eq!(
        current_label(&library),
        Some("Album"),
        "A's name must not name B's crumb"
    );
}

#[test]
fn a_loaded_album_crumb_is_still_the_only_current_one() {
    let mut library = Library::default();
    library.open(Page::Album("spotify:album:a".to_owned()));
    library.apply(&Update::Album(album_detail(
        "spotify:album:a",
        "Signal Decay",
    )));

    let crumbs = trail(&library);
    assert_eq!(crumbs.len(), 3);
    assert_eq!(crumbs[2].label, "Signal Decay");
    assert!(crumbs[2].current);
    assert_eq!(crumbs[2].target, None);
    assert_eq!(
        crumbs.iter().filter(|crumb| crumb.current).count(),
        1,
        "one current crumb, whatever it is named"
    );
}

#[test]
fn an_artist_page_names_the_loaded_artist() {
    let mut library = Library::default();
    library.open(Page::Artist("spotify:artist:a".to_owned()));
    library.apply(&Update::Artist(artist_detail(
        "spotify:artist:a",
        "Vela Moreno",
    )));

    assert_eq!(current_label(&library), Some("Vela Moreno"));
}

#[test]
fn an_artist_page_names_its_kind_while_loading() {
    let mut library = Library::default();
    library.open(Page::Artist("spotify:artist:a".to_owned()));

    assert_eq!(current_label(&library), Some("Artist"));
}

#[test]
fn an_artist_page_names_its_kind_after_a_failure() {
    let mut library = Library::default();
    library.open(Page::Artist("spotify:artist:a".to_owned()));
    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(current_label(&library), Some("Artist"));
}

#[test]
fn an_artist_page_names_its_kind_for_another_uri() {
    let mut library = Library::default();
    library.open(Page::Artist("spotify:artist:b".to_owned()));
    library.artist = Load::Ready(artist_detail("spotify:artist:a", "Vela Moreno"));

    assert_eq!(
        current_label(&library),
        Some("Artist"),
        "A's name must not name B's crumb"
    );
}

#[test]
fn a_loaded_show_names_the_crumb() {
    let mut library = Library::default();
    library.open(Page::Show("spotify:show:a".to_owned()));
    library.apply(&Update::Show(show_detail(
        "spotify:show:a",
        "Lateral with Tom Scott",
    )));

    assert_eq!(
        current_label(&library),
        Some("Lateral with Tom Scott"),
        "a loaded show names its crumb"
    );
}

#[test]
fn another_shows_detail_does_not_name_the_crumb() {
    let mut library = Library::default();
    library.open(Page::Show("spotify:show:b".to_owned()));
    library.show = Load::Ready(show_detail("spotify:show:a", "Lateral"));

    assert_eq!(
        current_label(&library),
        Some("Show"),
        "A's detail must not name B's crumb"
    );
}

#[test]
fn an_album_artist_or_show_page_gives_its_kind_as_current() {
    for page in [
        Page::Album("spotify:album:a".to_owned()),
        Page::Artist("spotify:artist:a".to_owned()),
        Page::Show("spotify:show:a".to_owned()),
    ] {
        let kind = page.kind();
        let list = owning_list(&page).expect("every drawn page lives in a list");
        let mut library = Library::default();
        library.open(page);

        assert_eq!(
            trail(&library),
            vec![
                Crumb {
                    label: "Home",
                    current: false,
                    target: Some(Target::Root(RootKind::All)),
                },
                Crumb {
                    label: root_filter_label(list),
                    current: false,
                    target: Some(Target::Root(list)),
                },
                Crumb {
                    label: kind,
                    current: true,
                    target: None,
                },
            ]
        );
    }
}
