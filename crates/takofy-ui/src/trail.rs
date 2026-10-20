//! The current location as a stack of crumbs.
//!
//! The view draws the crumbs; this module only decides which ones exist, which
//! one is current, and which one is a link back. It never measures text.

use crate::library::Library;
use crate::route::Page;

/// One entry in the location trail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Crumb<'a> {
    pub label: &'a str,
    /// The deepest crumb, drawn as the current location.
    pub current: bool,
    /// Whether the crumb can be clicked to go back.
    pub link: bool,
}

/// The trail for the current page.
///
/// With the root on screen, Playlists is where you are. On any page the
/// Playlists crumb becomes a link and the last crumb is current: the playlist's
/// name once the list that names it is ready, the album's or artist's own name
/// once its detail names this page, or the page's kind when there is no name to
/// show. A
/// playlist whose entry is not in a ready list is the one page that draws no
/// current crumb at all.
pub fn trail(library: &Library) -> Vec<Crumb<'_>> {
    let mut crumbs = vec![Crumb {
        label: "Home",
        current: false,
        link: false,
    }];

    let Some(page) = library.page() else {
        crumbs.push(Crumb {
            label: "Playlists",
            current: true,
            link: false,
        });
        return crumbs;
    };

    crumbs.push(Crumb {
        label: "Playlists",
        current: false,
        link: true,
    });
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
    };
    if let Some(label) = label {
        crumbs.push(Crumb {
            label,
            current: true,
            link: false,
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

    /// The current crumb's label: the last crumb that is not a link.
    fn current_label(library: &Library) -> Option<&str> {
        trail(library)
            .into_iter()
            .find(|crumb| crumb.current)
            .map(|crumb| crumb.label)
    }

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
        }
    }

    /// Only one crumb may be a link, because the view names a link crumb with a
    /// single `Control`, and two widgets cannot share a widget id. This is the
    /// bug that drew egui's "second use of widget ID" warning over the trail the
    /// moment a playlist was opened.
    #[test]
    fn at_most_one_crumb_is_a_link() {
        let mut unselected = Library::default();
        unselected.apply(&Update::Playlists(vec![playlist("a", "Mix")]));

        let mut selected = Library::default();
        selected.apply(&Update::Playlists(vec![playlist("a", "Mix")]));
        selected.select("a");

        let mut album = Library::default();
        album.open(Page::Album("spotify:album:a".to_owned()));

        let mut artist = Library::default();
        artist.open(Page::Artist("spotify:artist:a".to_owned()));

        let mut show = Library::default();
        show.open(Page::Show("spotify:show:a".to_owned()));

        for library in [unselected, selected, album, artist, show] {
            let crumbs = trail(&library);
            let links = crumbs.iter().filter(|crumb| crumb.link).count();
            assert!(links <= 1, "{links} link crumbs: {crumbs:?}");
        }
    }

    #[test]
    fn no_selection_gives_home_then_playlists_as_current() {
        let library = Library::default();
        assert_eq!(
            trail(&library),
            vec![
                Crumb {
                    label: "Home",
                    current: false,
                    link: false,
                },
                Crumb {
                    label: "Playlists",
                    current: true,
                    link: false,
                },
            ]
        );
    }

    #[test]
    fn a_selection_gives_home_a_playlists_link_and_the_name() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "Mix")]));
        library.select("a");

        assert_eq!(
            trail(&library),
            vec![
                Crumb {
                    label: "Home",
                    current: false,
                    link: false,
                },
                Crumb {
                    label: "Playlists",
                    current: false,
                    link: true,
                },
                Crumb {
                    label: "Mix",
                    current: true,
                    link: false,
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
        assert!(crumbs[1].link);
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
        assert!(!crumbs[2].link);
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
            let mut library = Library::default();
            library.open(page);

            assert_eq!(
                trail(&library),
                vec![
                    Crumb {
                        label: "Home",
                        current: false,
                        link: false,
                    },
                    Crumb {
                        label: "Playlists",
                        current: false,
                        link: true,
                    },
                    Crumb {
                        label: kind,
                        current: true,
                        link: false,
                    },
                ]
            );
        }
    }
}
