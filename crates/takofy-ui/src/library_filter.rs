//! The library root's filter row: which chips it shows, and what each shows.
//!
//! The row is the reference's five chips — All, Playlists, Artists, Albums and
//! Podcasts — in a fixed order. All, Playlists and Podcasts have data and act;
//! Artists and Albums are the reference's chips with no data yet, so they are
//! drawn but inert.
//!
//! The choice belongs to the root, not to the frame: it is kept across a page,
//! so a Back to the root redraws the same chip.

use takofy_model::{FollowedShow, PlaylistEntry};

use crate::availability::{Availability, Control, availability};

/// A chip in the library root's filter row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RootKind {
    /// Every card, playlists first and then shows.
    #[default]
    All,
    /// Only playlists.
    Playlists,
    /// The reference's chip. There are no artists yet, so it is inert.
    Artists,
    /// The reference's chip. There are no albums yet, so it is inert.
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

    /// Whether the chip has data yet, so the row can draw it live or inert.
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

    /// Presses a chip. There is no toggle: pressing a waiting chip is a no-op and
    /// returns `false`, so the row can draw it pressed without acting.
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
}

/// The cards for the chosen chip: playlists first, then shows.
///
/// All shows both, Playlists narrows to playlists, Podcasts narrows to shows,
/// and Artists and Albums show nothing yet — an empty list, not an error, so a
/// waiting chip's cards are simply none.
pub fn items<'a>(
    chosen: RootKind,
    playlists: &'a [PlaylistEntry],
    followed: &'a [FollowedShow],
) -> Vec<Item<'a>> {
    match chosen {
        RootKind::All => playlists
            .iter()
            .map(Item::Playlist)
            .chain(followed.iter().map(Item::Show))
            .collect(),
        RootKind::Playlists => playlists.iter().map(Item::Playlist).collect(),
        RootKind::Podcasts => followed.iter().map(Item::Show).collect(),
        RootKind::Artists | RootKind::Albums => Vec::new(),
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
    fn only_all_playlists_and_podcasts_act() {
        for kind in ROW {
            let acts = matches!(
                kind,
                RootKind::All | RootKind::Playlists | RootKind::Podcasts
            );
            assert_eq!(kind.acts(), acts, "{kind:?}");
        }
    }

    #[test]
    fn choosing_a_waiting_chip_changes_nothing() {
        for kind in [RootKind::Artists, RootKind::Albums] {
            let mut filter = RootFilter::default();
            assert!(
                !filter.choose(kind),
                "a waiting chip reports that it did not act"
            );
            assert_eq!(filter.chosen(), RootKind::All, "and leaves the choice");
        }
    }

    #[test]
    fn all_shows_playlists_then_shows() {
        let playlists = vec![playlist("p")];
        let shows = vec![show("s")];
        let items = items(RootKind::All, &playlists, &shows);
        assert_eq!(items[0], Item::Playlist(&playlist("p")));
        assert_eq!(items[1], Item::Show(&show("s")));
    }

    #[test]
    fn playlists_narrows_to_playlists() {
        let playlists = vec![playlist("p")];
        let shows = vec![show("s")];
        let items = items(RootKind::Playlists, &playlists, &shows);
        assert_eq!(items, vec![Item::Playlist(&playlist("p"))]);
    }

    #[test]
    fn podcasts_narrows_to_shows() {
        let playlists = vec![playlist("p")];
        let shows = vec![show("s")];
        let items = items(RootKind::Podcasts, &playlists, &shows);
        assert_eq!(items, vec![Item::Show(&show("s"))]);
    }
}
