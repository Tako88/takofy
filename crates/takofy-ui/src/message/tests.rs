use super::*;
use takofy_model::Show;

fn revision() -> Revision {
    Revision::from_wire(&[1]).expect("a revision")
}

fn access() -> PlaylistAccess {
    PlaylistAccess {
        revision: revision(),
        editable: true,
    }
}

fn entry() -> PlaylistEntry {
    PlaylistEntry {
        uri: "spotify:playlist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner: None,
        access: None,
        saved_at: None,
    }
}

fn cover_url() -> CoverUrl {
    CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover URL")
}

fn album() -> AlbumDetail {
    AlbumDetail {
        uri: "spotify:album:a".to_owned(),
        name: "A".to_owned(),
        artists: String::new(),
        artist_uri: None,
        year: None,
        cover: None,
        tracks: Vec::new(),
    }
}

fn artist() -> ArtistDetail {
    ArtistDetail {
        uri: "spotify:artist:a".to_owned(),
        name: "A".to_owned(),
        portrait: None,
        top_tracks: Vec::new(),
        releases: Vec::new(),
        related: Vec::new(),
    }
}

fn show() -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: "spotify:show:a".to_owned(),
            name: "A".to_owned(),
            publisher: String::new(),
            description: String::new(),
            cover: None,
        },
        episodes: Vec::new(),
    }
}

fn account() -> Account {
    Account::from_wire("tako").expect("a name")
}

/// Every variant, with the answer this build's debounce needs from it.
///
/// The table is the contract: a variant that starts changing a list has to be
/// added here and in [`Update::changes_lists`].
fn cases() -> Vec<(bool, Update)> {
    vec![
        (false, Update::SignedIn),
        (false, Update::NeedsAuthorization),
        (false, Update::SignInFailed(Failure::Other)),
        (false, Update::Account(account())),
        (false, Update::LibraryReady),
        (true, Update::Playlists(Vec::new())),
        (true, Update::PlaylistFailed(Failure::Load)),
        (false, Update::RootlistAccess(access())),
        (
            true,
            Update::PlaylistMoved {
                revision: revision(),
            },
        ),
        (true, Update::PlaylistMoveFailed),
        (
            true,
            Update::PlaylistRemoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(),
            },
        ),
        (
            true,
            Update::PlaylistRemoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
            },
        ),
        (
            true,
            Update::PlaylistLinked {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(),
            },
        ),
        (
            true,
            Update::PlaylistLinkFailed {
                playlist: "spotify:playlist:a".to_owned(),
            },
        ),
        (
            false,
            Update::SavedState {
                uri: "spotify:album:a".to_owned(),
                saved: None,
            },
        ),
        (
            true,
            Update::Saved {
                uri: "spotify:album:a".to_owned(),
                saved: true,
            },
        ),
        (
            true,
            Update::SavedFailed {
                uri: "spotify:album:a".to_owned(),
                saved: true,
            },
        ),
        (
            true,
            Update::TrackAdded {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(),
            },
        ),
        (
            true,
            Update::AddFailed {
                playlist: "spotify:playlist:a".to_owned(),
            },
        ),
        (
            true,
            Update::PlaylistUnfilled {
                playlist: "spotify:playlist:a".to_owned(),
            },
        ),
        (
            true,
            Update::PlaylistUnnamed {
                playlist: "spotify:playlist:a".to_owned(),
            },
        ),
        (
            true,
            Update::PlaylistRenamed {
                playlist: "spotify:playlist:a".to_owned(),
                name: "A".to_owned(),
                revision: revision(),
            },
        ),
        (
            true,
            Update::PlaylistRenameFailed {
                playlist: "spotify:playlist:a".to_owned(),
            },
        ),
        (
            true,
            Update::PlaylistCreated {
                entry: entry(),
                revision: revision(),
            },
        ),
        (true, Update::PlaylistCreateFailed),
        (true, Update::PlaylistUnlinked),
        (
            true,
            Update::Tracks {
                playlist: "spotify:playlist:a".to_owned(),
                tracks: Vec::new(),
                cover: None,
                access: None,
            },
        ),
        (
            true,
            Update::TrackMoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(),
            },
        ),
        (
            true,
            Update::MoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Save,
            },
        ),
        (
            true,
            Update::TracksRemoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(),
            },
        ),
        (
            true,
            Update::RemoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Save,
            },
        ),
        (
            false,
            Update::TracksFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Load,
            },
        ),
        (false, Update::Album(album())),
        (
            false,
            Update::AlbumFailed {
                album: "spotify:album:a".to_owned(),
                failure: Failure::Load,
            },
        ),
        (false, Update::Artist(artist())),
        (
            false,
            Update::ArtistFailed {
                artist: "spotify:artist:a".to_owned(),
                failure: Failure::Load,
            },
        ),
        (false, Update::Show(show())),
        (
            false,
            Update::ShowFailed {
                show: "spotify:show:a".to_owned(),
                failure: Failure::Load,
            },
        ),
        (true, Update::FollowedShows(Vec::new())),
        (true, Update::FollowedShowsFailed(Failure::Followed)),
        (true, Update::FollowedArtists(Vec::new())),
        (
            false,
            Update::Related {
                artist: "spotify:artist:a".to_owned(),
                cards: Vec::new(),
            },
        ),
        (
            false,
            Update::RelatedFailed {
                artist: "spotify:artist:a".to_owned(),
                failure: Failure::Related,
            },
        ),
        (
            true,
            Update::FollowedArtistsFailed(Failure::FollowedArtists),
        ),
        (true, Update::SavedAlbums(Vec::new())),
        (true, Update::SavedAlbumsFailed(Failure::SavedAlbums)),
        (true, Update::LikedTracks(Vec::new())),
        (true, Update::LikedTracksFailed(Failure::Load)),
        (
            false,
            Update::SavedLabelled {
                asked: Vec::new(),
                named: Vec::new(),
            },
        ),
        (
            false,
            Update::SavedLabelFailed {
                asked: Vec::new(),
                failure: Failure::Load,
            },
        ),
        (
            true,
            Update::Liked {
                tracks: Vec::new(),
                liked: true,
            },
        ),
        (
            true,
            Update::LikeFailed {
                tracks: Vec::new(),
                failure: Failure::Save,
            },
        ),
        (
            false,
            Update::ReleaseTracks {
                release: "spotify:album:a".to_owned(),
                tracks: Vec::new(),
            },
        ),
        (
            false,
            Update::ReleaseTracksFailed {
                release: "spotify:album:a".to_owned(),
                failure: Failure::Load,
            },
        ),
        (
            true,
            Update::Followed {
                artist: "spotify:artist:a".to_owned(),
                followed: false,
            },
        ),
        (
            true,
            Update::FollowFailed {
                artist: "spotify:artist:a".to_owned(),
                failure: Failure::Save,
            },
        ),
        (
            false,
            Update::Cover {
                url: cover_url(),
                image: CoverImage::new(1, 1, vec![0, 0, 0, 0]).expect("an image"),
            },
        ),
        (false, Update::CoverFailed { url: cover_url() }),
        (
            false,
            Update::Lyrics {
                track: "spotify:track:a".to_owned(),
                lyrics: TrackLyrics::default(),
            },
        ),
        (
            false,
            Update::LyricsFailed {
                track: "spotify:track:a".to_owned(),
                failure: Failure::Lyrics,
            },
        ),
        (
            false,
            Update::Search {
                query: "a".to_owned(),
                tracks: Vec::new(),
            },
        ),
        (
            false,
            Update::SearchFailed {
                query: "a".to_owned(),
                failure: Failure::Search,
            },
        ),
        (
            false,
            Update::Portraits {
                query: "a".to_owned(),
                portraits: Vec::new(),
            },
        ),
        (
            false,
            Update::PortraitsFailed {
                query: "a".to_owned(),
            },
        ),
        (false, Update::Home(HomeAnswer::Episodes(Vec::new()))),
        (
            false,
            Update::HomeFailed {
                ask: HomeAsk::Related {
                    artist: "spotify:artist:a".to_owned(),
                },
                failure: Failure::Load,
            },
        ),
        (false, Update::Transport(TransportUpdate::Loading)),
        (false, Update::Devices(DeviceUpdate::SteerFailed)),
        (false, Update::Remote(RemoteUpdate::Lost { epoch: 0 })),
    ]
}

#[test]
fn only_the_updates_that_touch_a_list_ask_for_a_new_copy() {
    for (expected, update) in cases() {
        assert_eq!(update.changes_lists(), expected, "{update:?}");
    }
}
