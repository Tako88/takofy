//! The library root's filter row: which chips it shows, and what each shows.
//!
//! The row is the reference's five chips — All, Playlists, Artists, Albums and
//! Podcasts — in a fixed order, and each names a list the root can draw. A chip
//! whose list has no reader yet would be drawn but would not act; `acts` is that
//! question, and every chip answers true today.
//!
//! The choice belongs to the root, not to the frame: it is kept across a page,
//! so a Back to the root redraws the same chip.

use takofy_model::{FollowedArtist, FollowedShow, PlaylistEntry, SavedAlbum};

use crate::availability::{Availability, Control, availability};

/// A chip in the library root's filter row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RootKind {
    /// Every card, playlists first and then shows.
    #[default]
    All,
    /// Only playlists.
    Playlists,
    /// Only the artists the user follows.
    Artists,
    /// Only the albums the user saved.
    Albums,
    /// Only followed shows.
    Podcasts,
}

/// The chips in the reference's order.
///
/// A fixed order, because the root's contents are independent of the order the
/// playlists and shows happen to arrive in.
pub const ROW: [RootKind; 5] = [
    RootKind::All,
    RootKind::Playlists,
    RootKind::Artists,
    RootKind::Albums,
    RootKind::Podcasts,
];

impl RootKind {
    /// This chip's control, so the row gives each chip its own widget id.
    pub fn control(self) -> Control {
        match self {
            RootKind::All => Control::LibraryFilterAll,
            RootKind::Playlists => Control::LibraryFilterPlaylists,
            RootKind::Artists => Control::LibraryFilterArtists,
            RootKind::Albums => Control::LibraryFilterAlbums,
            RootKind::Podcasts => Control::LibraryFilterPodcasts,
        }
    }

    /// Whether the chip has a list behind it, so the row can draw it live or
    /// inert.
    pub fn acts(self) -> bool {
        matches!(availability(self.control()), Availability::Ready)
    }
}

/// The root's last-pressed chip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RootFilter {
    /// The chip last pressed.
    chosen: RootKind,
}

impl RootFilter {
    /// The chip last pressed.
    pub fn chosen(&self) -> RootKind {
        self.chosen
    }

    /// Presses a chip. There is no toggle: pressing the chosen chip again is
    /// harmless because the choice is not a toggle. A chip with no list behind
    /// it is a no-op that returns `false`, so the row can draw it pressed
    /// without acting.
    pub fn choose(&mut self, kind: RootKind) -> bool {
        if !kind.acts() {
            return false;
        }
        self.chosen = kind;
        true
    }
}

/// A card the root draws.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item<'a> {
    /// A playlist, drawn as its own card.
    Playlist(&'a PlaylistEntry),
    /// A followed show, drawn as its own card.
    Show(&'a FollowedShow),
    /// A followed artist, drawn as its own card.
    Artist(&'a FollowedArtist),
    /// A saved album, drawn as its own card.
    Album(&'a SavedAlbum),
}

/// The cards for the chosen chip.
///
/// All shows the playlists then the followed shows, which is what the library
/// has always drawn first; the two newer kinds narrow to their own lists rather
/// than joining it. An empty list is not an error, so a chip with nothing
/// behind it draws the empty copy.
pub fn items<'a>(
    chosen: RootKind,
    playlists: &'a [PlaylistEntry],
    followed: &'a [FollowedShow],
    artists: &'a [FollowedArtist],
    albums: &'a [SavedAlbum],
) -> Vec<Item<'a>> {
    match chosen {
        RootKind::All => playlists
            .iter()
            .map(Item::Playlist)
            .chain(followed.iter().map(Item::Show))
            .collect(),
        RootKind::Playlists => playlists.iter().map(Item::Playlist).collect(),
        RootKind::Artists => artists.iter().map(Item::Artist).collect(),
        RootKind::Albums => albums.iter().map(Item::Album).collect(),
        RootKind::Podcasts => followed.iter().map(Item::Show).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(uri: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
        }
    }

    fn show(uri: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            named: true,
            publisher: None,
            cover: None,
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
    fn all_shows_playlists_then_shows() {
        let playlists = vec![playlist("p")];
        let shows = vec![show("s")];
        let artists = vec![artist("a")];
        let albums = vec![album("b")];
        let items = items(RootKind::All, &playlists, &shows, &artists, &albums);
        assert_eq!(items[0], Item::Playlist(&playlist("p")));
        assert_eq!(items[1], Item::Show(&show("s")));
        assert_eq!(items.len(), 2, "the two newer kinds stay out of All");
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
}
