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
mod tests;
