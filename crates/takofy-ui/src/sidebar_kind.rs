//! The sidebar's kind chips, and which kind the list shows.

use crate::availability::{Availability, Control, availability};

/// A chip in the sidebar's kind row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SidebarKind {
    /// Only playlists.
    #[default]
    Playlists,
    /// Only the artists the user follows.
    Artists,
    /// Only the albums the user saved.
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

    /// Whether the chip has a list behind it, so the row can draw it live or
    /// inert.
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

    /// Presses a chip. There is no toggle: pressing the chosen chip again is
    /// harmless because the choice is not a toggle. A chip with no list behind
    /// it is a no-op that returns `false`, so the row can draw it pressed
    /// without acting.
    pub fn choose(&mut self, kind: SidebarKind) -> bool {
        if !kind.acts() {
            return false;
        }
        self.chosen = kind;
        true
    }
}

#[cfg(test)]
mod tests;
