//! Which chrome slots exist, and whether the view behind them exists yet.
//!
//! A control's liveness is a fact about the build, not about the moment, so it
//! is one table read in one place. Adding a slot fails to compile until the
//! match says what it is, and no component ever passes a bare `false`.

/// One addressable slot in the chrome.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Control {
    NavHome,
    NavLibrary,
    LibraryCreate,
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
    /// An episode row's play toggle. The show page's episodes are loaded state.
    EpisodeToggle,
    /// A release block's like and more. Saving needs the Web API and the menu is
    /// a later phase.
    ReleaseLike,
    ReleaseMore,
    /// The show page's Newest/Oldest chips. Both orders are a sort over episodes
    /// the page already holds, so there is nothing to leave.
    EpisodeOrder,
    /// The library root's filter row, one control per chip. All, Playlists and
    /// Podcasts exist; Artists and Albums are the reference's chips with no data
    /// yet, drawn dim and inert like the sidebar's waiting filters.
    LibraryFilterAll,
    LibraryFilterPlaylists,
    LibraryFilterArtists,
    LibraryFilterAlbums,
    LibraryFilterPodcasts,
    /// The library row's overflow menu.
    LibraryMore,
    /// The library page's two densities.
    LayoutGrid,
    LayoutList,
    /// The day-one page's door into the catalogue, which focuses the search
    /// field.
    HomeSearch,
    /// A library item's own play, in the grid and in the list. It is keyed by the
    /// item's uri, so one control serves every card and every row.
    CardPlay,
    /// A station's own play. It is keyed by the station's uri, so one control
    /// serves every station of the band.
    StationPlay,
    Authorize,
}

impl Control {
    pub const ALL: [Self; 53] = [
        Self::NavHome,
        Self::NavLibrary,
        Self::LibraryCreate,
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
        Self::LibraryFilterAll,
        Self::LibraryFilterPlaylists,
        Self::LibraryFilterArtists,
        Self::LibraryFilterAlbums,
        Self::LibraryFilterPodcasts,
        Self::LibraryMore,
        Self::LayoutGrid,
        Self::LayoutList,
        Self::ReleasePlay,
        Self::EpisodeToggle,
        Self::ReleaseLike,
        Self::ReleaseMore,
        Self::EpisodeOrder,
        Self::CardPlay,
        Self::StationPlay,
        Self::HomeSearch,
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

        Control::NavHome => Availability::Ready,
        Control::NavLibrary => Availability::Ready,
        Control::LibraryCreate => Availability::Ready, // #35
        Control::FilterPlaylists => Availability::Ready, // #3
        Control::FilterArtists => Availability::Ready,
        Control::FilterAlbums => Availability::Ready,
        Control::FilterPodcasts => Availability::Ready, // #3
        Control::SearchField => Availability::Ready,
        Control::TrailHome => Availability::Ready,
        Control::NowPlayingMore => Availability::Ready, // #16

        // Transport and seek are their own issue.
        Control::Previous => Availability::Ready,  // #13
        Control::PlayPause => Availability::Ready, // #13
        Control::Next => Availability::Ready,      // #13
        Control::Seek => Availability::Ready,      // #13

        // Playbar chrome with no separate issue yet.
        Control::Like => Availability::Ready,       // #35
        Control::Shuffle => Availability::Ready,    // #35
        Control::Repeat => Availability::Ready,     // #35
        Control::Queue => Availability::Ready,      // #35
        Control::Devices => Availability::Ready,    // #38
        Control::Mute => Availability::Ready,       // #35
        Control::Volume => Availability::Ready,     // #35
        Control::Fullscreen => Availability::Ready, // #35

        // A row's like and more are not the playbar's like or the Now Playing
        // more: they act on another track and must carry their own ids.
        Control::TrackLike => Availability::Ready, // #35
        Control::TrackMore => Availability::Ready,

        // The panel's tabs: the queue is state the window already holds, and
        // lyrics are a fetch and a parse it does not make. #15
        Control::NowPlayingQueueTab => Availability::Ready,
        Control::NowPlayingLyricsTab => Availability::Ready,

        // The hero's tools have no command yet. Its play has one, so the view
        // is Ready and the moment decides whether it can act.
        Control::HeroShuffle => Availability::Ready, // #35
        Control::HeroLike => Availability::Ready,    // #35
        Control::HeroMore => Availability::Ready,
        Control::HeroPlay => Availability::Ready,
        Control::CardPlay => Availability::Ready,
        // A station's rows are loaded state, so its play is a want and not a
        // read.
        Control::StationPlay => Availability::Ready,

        // The artist page: following is a real write, the filter row is state
        // the page already holds, and the other three are the reference's chrome
        // with nothing behind it yet.
        Control::Follow => Availability::Ready,
        Control::DiscographyFilter => Availability::Ready,

        // The library root's filter row: All, Playlists and Podcasts are live,
        // The library root's filter row: every chip names a list it can draw.
        Control::LibraryFilterAll => Availability::Ready, // #3
        Control::LibraryFilterPlaylists => Availability::Ready, // #3
        Control::LibraryFilterArtists => Availability::Ready,
        Control::LibraryFilterAlbums => Availability::Ready,
        Control::LibraryFilterPodcasts => Availability::Ready, // #3
        Control::LibraryMore => Availability::Ready,
        Control::LayoutGrid | Control::LayoutList => Availability::Ready,
        Control::HomeSearch => Availability::Ready,
        Control::ReleasePlay => Availability::Ready,
        Control::EpisodeToggle => Availability::Ready,
        Control::ReleaseLike => Availability::Ready, // #35
        Control::ReleaseMore => Availability::Ready, // #35
        Control::EpisodeOrder => Availability::Ready,

        // The sign-in card's one control; the state decides whether it acts.
        Control::Authorize => Availability::Ready,
    }
}

#[cfg(test)]
mod tests;
