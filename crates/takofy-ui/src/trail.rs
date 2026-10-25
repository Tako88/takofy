//! The current location as a stack of crumbs.
//!
//! The view draws the crumbs; this module only decides which ones exist, which
//! one is current, and where a click on the others leads. It never measures text.

use crate::format::root_filter_label;
use crate::library::Library;
use crate::library_filter::RootKind;
use crate::route::Page;

/// One entry in the location trail.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Crumb<'a> {
    pub label: &'a str,
    /// The deepest crumb, drawn as the current location.
    pub current: bool,
    /// Where a click goes. `None` draws the crumb as text.
    pub target: Option<Target>,
}

/// Where a crumb leads.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    /// The library root, showing one chip.
    Root(RootKind),
    /// A page.
    Page(Page),
}

/// The root list a page's kind belongs to.
///
/// A search is not a list the root holds, so it has no middle crumb: the query
/// is its own answer.
fn owning_list(page: &Page) -> Option<RootKind> {
    match page {
        Page::Playlist(_) => Some(RootKind::Playlists),
        Page::Album(_) => Some(RootKind::Albums),
        Page::Artist(_) => Some(RootKind::Artists),
        Page::Show(_) => Some(RootKind::Podcasts),
        Page::Search(_) => None,
    }
}

/// The trail for the current page.
///
/// With the root on screen, Your Library is where you are. On any page Home
/// leads back to the root as it was, the list the page belongs to leads back to
/// the root showing that list's chip, and the last crumb is current: the
/// playlist's name once the list that names it is ready, the album's or
/// artist's own name once its detail names this page, or the page's kind when
/// there is no name to show. A playlist whose entry is not in a ready list is
/// the one page that draws no current crumb at all.
pub fn trail(library: &Library) -> Vec<Crumb<'_>> {
    let mut crumbs = vec![Crumb {
        label: "Home",
        current: false,
        target: library
            .page()
            .map(|_| Target::Root(library.root_filter.chosen())),
    }];

    let Some(page) = library.page() else {
        crumbs.push(Crumb {
            label: "Your Library",
            current: true,
            target: None,
        });
        return crumbs;
    };

    if let Some(kind) = owning_list(page) {
        crumbs.push(Crumb {
            label: root_filter_label(kind),
            current: false,
            target: Some(Target::Root(kind)),
        });
    }
    let label = match page {
        Page::Playlist(_) => library.selected_name(),
        // The album's name only once it is loaded, and only for this page: a
        // ready detail left over from the last album must not name this one.
        Page::Album(_) => Some(
            library
                .current_album()
                .map_or(page.kind(), |album| album.name.as_str()),
        ),
        // The artist's name only once it is loaded, and only for this page.
        Page::Artist(_) => Some(
            library
                .current_artist()
                .map_or(page.kind(), |artist| artist.name.as_str()),
        ),
        // The show's name only once it is loaded, and only for this page: a
        // ready detail left over from the last show must not name this one.
        Page::Show(_) => Some(
            library
                .current_show()
                .map_or(page.kind(), |detail| detail.show.name.as_str()),
        ),
        // A query has no name of its own to resolve, so the crumb is the kind.
        Page::Search(_) => Some(page.kind()),
    };
    if let Some(label) = label {
        crumbs.push(Crumb {
            label,
            current: true,
            target: None,
        });
    }

    crumbs
}

#[cfg(test)]
mod tests {
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
}
