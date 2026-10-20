//! The sidebar's kind chips, and which kind the list shows.

use crate::availability::{Availability, Control, availability};

/// A chip in the sidebar's kind row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SidebarKind {
    /// Only playlists.
    #[default]
    Playlists,
    /// The reference's chip. There are no artists yet, so it is inert.
    Artists,
    /// The reference's chip. There are no albums yet, so it is inert.
    Albums,
    /// Only followed shows.
    Podcasts,
}

/// The chips in the reference's order.
pub const ROW: [SidebarKind; 4] = [
    SidebarKind::Playlists,
    SidebarKind::Artists,
    SidebarKind::Albums,
    SidebarKind::Podcasts,
];

impl SidebarKind {
    /// This chip's control, so the row gives each chip its own widget id.
    pub fn control(self) -> Control {
        match self {
            SidebarKind::Playlists => Control::FilterPlaylists,
            SidebarKind::Artists => Control::FilterArtists,
            SidebarKind::Albums => Control::FilterAlbums,
            SidebarKind::Podcasts => Control::FilterPodcasts,
        }
    }

    /// Whether the chip has data yet, so the row can draw it live or inert.
    pub fn acts(self) -> bool {
        availability(self.control()) == Availability::Ready
    }
}

/// The sidebar's last-pressed chip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SidebarFilter {
    /// The chip last pressed.
    chosen: SidebarKind,
}

impl SidebarFilter {
    /// The chip last pressed.
    pub fn chosen(&self) -> SidebarKind {
        self.chosen
    }

    /// Presses a chip. There is no toggle: pressing a waiting chip is a no-op
    /// and returns `false`, so the row can draw it pressed without acting.
    pub fn choose(&mut self, kind: SidebarKind) -> bool {
        if !kind.acts() {
            return false;
        }
        self.chosen = kind;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_is_the_four_reference_chips_in_order() {
        assert_eq!(
            ROW,
            [
                SidebarKind::Playlists,
                SidebarKind::Artists,
                SidebarKind::Albums,
                SidebarKind::Podcasts,
            ]
        );
        assert_eq!(
            SidebarKind::default(),
            SidebarKind::Playlists,
            "the list shows playlists until the user picks another kind"
        );
    }

    #[test]
    fn each_kind_owns_its_own_control() {
        assert_eq!(SidebarKind::Playlists.control(), Control::FilterPlaylists);
        assert_eq!(SidebarKind::Artists.control(), Control::FilterArtists);
        assert_eq!(SidebarKind::Albums.control(), Control::FilterAlbums);
        assert_eq!(SidebarKind::Podcasts.control(), Control::FilterPodcasts);
    }

    #[test]
    fn acts_follows_the_availability_of_the_control() {
        for kind in ROW {
            assert_eq!(
                kind.acts(),
                availability(kind.control()) == Availability::Ready,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn choosing_a_waiting_chip_changes_nothing() {
        for kind in [SidebarKind::Artists, SidebarKind::Albums] {
            let mut filter = SidebarFilter::default();
            assert!(
                !filter.choose(kind),
                "a waiting chip reports that it did not act"
            );
            assert_eq!(
                filter.chosen(),
                SidebarKind::Playlists,
                "and leaves the choice"
            );
        }
    }
}
