//! The current location as a stack of crumbs.
//!
//! The view draws the crumbs; this module only decides which ones exist, which
//! one is current, and which one is a link back. It never measures text.

use crate::library::Library;

/// One entry in the location trail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Crumb<'a> {
    pub label: &'a str,
    /// The deepest crumb, drawn as the current location.
    pub current: bool,
    /// Whether the crumb can be clicked to go back.
    pub link: bool,
}

/// The trail for the current selection.
///
/// With nothing selected, Playlists is where you are. With a selection, the
/// Playlists crumb becomes a link and the playlist's name becomes current; if
/// the list that names it is still loading, there is no name crumb yet.
pub fn trail(library: &Library) -> Vec<Crumb<'_>> {
    let mut crumbs = vec![Crumb {
        label: "Home",
        current: false,
        link: false,
    }];

    if library.selected().is_some() {
        let name = library.selected_name();
        crumbs.push(Crumb {
            label: "Playlists",
            current: false,
            link: true,
        });
        if let Some(name) = name {
            crumbs.push(Crumb {
                label: name,
                current: true,
                link: false,
            });
        }
    } else {
        crumbs.push(Crumb {
            label: "Playlists",
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

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
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

        for library in [unselected, selected] {
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
}
