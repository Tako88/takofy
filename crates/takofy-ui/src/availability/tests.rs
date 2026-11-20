use super::*;

#[test]
fn only_the_controls_with_a_way_to_act_are_ready() {
    for control in Control::ALL {
        let ready = matches!(
            control,
            Control::DrawerToggle
                | Control::NavHome
                | Control::NavLibrary
                | Control::LibraryCreate
                | Control::TrailHome
                | Control::TrailPlaylists
                | Control::HistoryBack
                | Control::HistoryForward
                | Control::Previous
                | Control::PlayPause
                | Control::Next
                | Control::Seek
                | Control::Shuffle
                | Control::Repeat
                | Control::Queue
                | Control::Devices
                | Control::Mute
                | Control::Volume
                | Control::Fullscreen
                | Control::HeroPlay
                | Control::CardPlay
                | Control::StationPlay
                | Control::DiscoveryCard
                | Control::DiscoveryMore
                | Control::DiscoveryLoadMore
                | Control::DiscoveryPlay
                | Control::DiscoveryReroll
                | Control::DiscoveryRowPlay
                | Control::DiscoveryFollow
                | Control::DiscoverySwitch
                | Control::DrawAnother
                | Control::HeroMore
                | Control::HeroLike
                | Control::HeroShuffle
                | Control::Follow
                | Control::DiscographyFilter
                | Control::ReleasePlay
                | Control::ReleaseLike
                | Control::ReleaseMore
                | Control::EpisodeToggle
                | Control::EpisodeOrder
                | Control::TrackMore
                | Control::TrackLike
                | Control::Like
                | Control::NowPlayingMore
                | Control::NowPlayingQueueTab
                | Control::NowPlayingLyricsTab
                | Control::Authorize
                | Control::LibraryFilterAll
                | Control::LibraryFilterPlaylists
                | Control::LibraryFilterArtists
                | Control::LibraryFilterAlbums
                | Control::LibraryFilterPodcasts
                | Control::LibraryMore
                | Control::CardMore
                | Control::LayoutGrid
                | Control::LayoutList
                | Control::FilterPlaylists
                | Control::FilterArtists
                | Control::FilterAlbums
                | Control::FilterPodcasts
                | Control::SearchField
                | Control::HomeSearch
                | Control::SettingsEntry
                | Control::EngineBuiltIn
                | Control::EngineSoloist
                | Control::OpenDownload
                | Control::OpenKeyPage
                | Control::RescanProgram
                | Control::PathField
                | Control::KeyField
                | Control::StartPairing
                | Control::StopPairing
                | Control::AutoplaySwitch
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
    assert_eq!(seen.len(), Control::ALL.len());
}
