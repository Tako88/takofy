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
/// All is every kind in one column, in the chips' own order: the playlists and
/// the shows the library has always drawn first, then the artists and the
/// albums. A chip of its own narrows to its list. An empty list is not an
/// error, so a chip with nothing behind it draws the empty copy.
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
            .chain(artists.iter().map(Item::Artist))
            .chain(albums.iter().map(Item::Album))
            .collect(),
        RootKind::Playlists => playlists.iter().map(Item::Playlist).collect(),
        RootKind::Artists => artists.iter().map(Item::Artist).collect(),
        RootKind::Albums => albums.iter().map(Item::Album).collect(),
        RootKind::Podcasts => followed.iter().map(Item::Show).collect(),
    }
}

#[cfg(test)]
mod tests;
