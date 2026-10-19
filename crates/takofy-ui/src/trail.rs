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
/// name once the list that names it is ready, or the page's kind when there is
/// no name to show. A playlist whose entry is not in a ready list is the one
/// page that draws no current crumb at all.
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
        Page::Album(_) | Page::Artist(_) | Page::Show(_) => Some(page.kind()),
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
    use takofy_model::PlaylistEntry;

    use super::*;
    use crate::message::Update;
    use crate::route::Page;

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
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
