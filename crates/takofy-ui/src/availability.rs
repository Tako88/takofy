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
    TrackLike,
    TrackMore,
    /// The queue tab of the Now Playing panel. The list behind it exists.
    NowPlayingQueueTab,
    /// The lyrics tab. Drawn present and dim: lyrics data belongs to the next
    /// phase's fetch and parse.
    NowPlayingLyricsTab,
    HeroShuffle,
    HeroLike,
    HeroMore,
    HeroPlay,
    /// The artist page's Follow/Following toggle. The write exists.
    Follow,
    /// The artist page's discography filter row. It is one slot for the whole
    /// row: each chip is drawn inside its own id scope, so the row has one
    /// control and every chip still has its own widget id.
    DiscographyFilter,
    /// A release block's play tile. The block's rows are already loaded state.
    ReleasePlay,
    /// A release block's like and more. Saving needs the Web API and the menu is
    /// a later phase.
    ReleaseLike,
    ReleaseMore,
    /// A card's own play control in the library grid. The card selects its playlist;
    /// playing from the card needs the tracks loaded first, which no path does yet.
    CardPlay,
    Authorize,
}

impl Control {
    pub const ALL: [Self; 43] = [
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
        Self::TrackLike,
        Self::TrackMore,
        Self::NowPlayingQueueTab,
        Self::NowPlayingLyricsTab,
        Self::HeroShuffle,
        Self::HeroLike,
        Self::HeroMore,
        Self::HeroPlay,
        Self::Follow,
        Self::DiscographyFilter,
        Self::ReleasePlay,
        Self::ReleaseLike,
        Self::ReleaseMore,
        Self::CardPlay,
        Self::Authorize,
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
        // The drawer, the crumb and the history pair are the controls whose
        // views ship with the shell.
        Control::DrawerToggle => Availability::Ready, // #17
        Control::TrailPlaylists => Availability::Ready, // #3
        Control::HistoryBack => Availability::Ready,  // #16
        Control::HistoryForward => Availability::Ready, // #16

        Control::NavHome => Availability::Waiting,   // #17
        Control::NavSearch => Availability::Waiting, // #17
        Control::LibraryCreate => Availability::Waiting, // #3
        Control::LibraryExpand => Availability::Waiting, // #3
        Control::FilterPlaylists => Availability::Waiting, // #3
        Control::FilterArtists => Availability::Waiting, // #3
        Control::FilterAlbums => Availability::Waiting, // #3
        Control::FilterPodcasts => Availability::Waiting, // #3
        Control::SearchField => Availability::Waiting, // #16
        Control::TrailHome => Availability::Waiting, // #17
        Control::NowPlayingMore => Availability::Waiting, // #16

        // Transport and seek are their own issue.
        Control::Previous => Availability::Ready,  // #13
        Control::PlayPause => Availability::Ready, // #13
        Control::Next => Availability::Ready,      // #13
        Control::Seek => Availability::Ready,      // #13

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

        // A row's like and more are not the playbar's like or the Now Playing
        // more: they act on another track and must carry their own ids. Both
        // are drawn but have no command yet (Q5(a)).
        Control::TrackLike => Availability::Waiting,
        Control::TrackMore => Availability::Waiting,

        // The panel's tabs: the queue is state the window already holds, and
        // lyrics are a fetch and a parse it does not make. #15
        Control::NowPlayingQueueTab => Availability::Ready,
        Control::NowPlayingLyricsTab => Availability::Waiting,

        // The hero's tools have no command yet. Its play has one, so the view
        // is Ready and the moment decides whether it can act.
        Control::HeroShuffle => Availability::Waiting,
        Control::HeroLike => Availability::Waiting,
        Control::HeroMore => Availability::Waiting,
        Control::HeroPlay => Availability::Ready,
        Control::CardPlay => Availability::Waiting,

        // The artist page: following is a real write, the filter row is state
        // the page already holds, and the other three are the reference's chrome
        // with nothing behind it yet.
        Control::Follow => Availability::Ready,
        Control::DiscographyFilter => Availability::Ready,
        Control::ReleasePlay => Availability::Ready,
        Control::ReleaseLike => Availability::Waiting,
        Control::ReleaseMore => Availability::Waiting,

        // The sign-in card's one control; the state decides whether it acts.
        Control::Authorize => Availability::Ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_controls_with_a_way_to_act_are_ready() {
        for control in Control::ALL {
            let ready = matches!(
                control,
                Control::DrawerToggle
                    | Control::TrailPlaylists
                    | Control::HistoryBack
                    | Control::HistoryForward
                    | Control::Previous
                    | Control::PlayPause
                    | Control::Next
                    | Control::Seek
                    | Control::HeroPlay
                    | Control::Follow
                    | Control::DiscographyFilter
                    | Control::ReleasePlay
                    | Control::NowPlayingQueueTab
                    | Control::Authorize
            );
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
        assert_eq!(seen.len(), 43);
    }
}
