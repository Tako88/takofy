//! Which chrome slots exist, and whether the view behind them exists yet.
//!
//! A control's liveness is a fact about the build, not about the moment, so it
//! is one table read in one place. Adding a slot fails to compile until the
//! match says what it is, and no component ever passes a bare `false`.

/// One addressable slot in the chrome.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Control {
    NavHome,
    NavSearch,
    LibraryCreate,
    LibraryExpand,
    FilterPlaylists,
    FilterArtists,
    FilterAlbums,
    FilterPodcasts,
    DrawerToggle,
    HistoryBack,
    HistoryForward,
    SearchField,
    TrailHome,
    TrailPlaylists,
    Account,
    NowPlayingMore,
    Like,
    Miniplayer,
    Shuffle,
    Previous,
    PlayPause,
    Next,
    Repeat,
    Seek,
    Queue,
    Devices,
    Mute,
    Volume,
    Fullscreen,
}

impl Control {
    pub const ALL: [Self; 29] = [
        Self::NavHome,
        Self::NavSearch,
        Self::LibraryCreate,
        Self::LibraryExpand,
        Self::FilterPlaylists,
        Self::FilterArtists,
        Self::FilterAlbums,
        Self::FilterPodcasts,
        Self::DrawerToggle,
        Self::HistoryBack,
        Self::HistoryForward,
        Self::SearchField,
        Self::TrailHome,
        Self::TrailPlaylists,
        Self::Account,
        Self::NowPlayingMore,
        Self::Like,
        Self::Miniplayer,
        Self::Shuffle,
        Self::Previous,
        Self::PlayPause,
        Self::Next,
        Self::Repeat,
        Self::Seek,
        Self::Queue,
        Self::Devices,
        Self::Mute,
        Self::Volume,
        Self::Fullscreen,
    ];
}

/// Whether a slot's view is live.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    /// The view exists and the control works.
    Ready,
    /// Drawn present but inert, until its view arrives.
    Waiting,
}

/// What the build can do today.
///
/// The match has no catch-all: a new `Control` variant does not compile until
/// it is decided here. The comments cite the nearest durable issue and say so
/// when there is none.
pub fn availability(control: Control) -> Availability {
    match control {
        // The drawer and the crumb are the two controls whose views ship with
        // the shell.
        Control::DrawerToggle => Availability::Ready, // #17
        Control::TrailPlaylists => Availability::Ready, // #3

        Control::NavHome => Availability::Waiting,   // #17
        Control::NavSearch => Availability::Waiting, // #17
        Control::LibraryCreate => Availability::Waiting, // #3
        Control::LibraryExpand => Availability::Waiting, // #3
        Control::FilterPlaylists => Availability::Waiting, // #3
        Control::FilterArtists => Availability::Waiting, // #3
        Control::FilterAlbums => Availability::Waiting, // #3
        Control::FilterPodcasts => Availability::Waiting, // #3
        Control::HistoryBack => Availability::Waiting, // #16
        Control::HistoryForward => Availability::Waiting, // #16
        Control::SearchField => Availability::Waiting, // #16
        Control::TrailHome => Availability::Waiting, // #17
        Control::Account => Availability::Waiting,   // #15
        Control::NowPlayingMore => Availability::Waiting, // #16

        // Transport and seek are their own issue.
        Control::Previous => Availability::Waiting, // #13
        Control::PlayPause => Availability::Waiting, // #13
        Control::Next => Availability::Waiting,     // #13
        Control::Seek => Availability::Waiting,     // #13

        // Playbar chrome with no separate issue yet.
        Control::Like => Availability::Waiting,
        Control::Miniplayer => Availability::Waiting,
        Control::Shuffle => Availability::Waiting,
        Control::Repeat => Availability::Waiting,
        Control::Queue => Availability::Waiting,
        Control::Devices => Availability::Waiting,
        Control::Mute => Availability::Waiting,
        Control::Volume => Availability::Waiting,
        Control::Fullscreen => Availability::Waiting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_drawer_toggle_and_the_playlists_crumb_are_ready() {
        for control in Control::ALL {
            let ready = matches!(control, Control::DrawerToggle | Control::TrailPlaylists);
            assert_eq!(
                availability(control) == Availability::Ready,
                ready,
                "{control:?}"
            );
        }
    }

    #[test]
    fn every_control_is_listed_once() {
        let mut seen = Vec::new();
        for control in Control::ALL {
            assert!(!seen.contains(&control), "{control:?} is listed twice");
            seen.push(control);
        }
        assert_eq!(seen.len(), 29);
    }
}
