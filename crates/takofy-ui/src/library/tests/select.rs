use super::super::*;
use super::test_support::*;
use crate::message::{DeviceUpdate, TransportUpdate};
use takofy_model::AddedAt;

fn dated(uri: &str, wire: &str) -> TrackEntry {
    TrackEntry {
        added_at: AddedAt::from_wire(wire),
        ..track(uri)
    }
}

#[test]
fn selected_is_none_before_a_click() {
    assert_eq!(Library::default().selected(), None);
}

#[test]
fn signed_in_starts_the_playlists_loading() {
    let mut library = Library::default();
    library.apply(&Update::SignedIn);
    assert_eq!(library.playlists, Load::Loading);
}

#[test]
fn signed_in_leaves_ready_playlists_alone() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::SignedIn);
    assert_eq!(library.playlists, Load::Ready(vec![playlist("a", "A")]));
}

#[test]
fn playlists_become_ready_without_touching_the_tracks() {
    let mut library = Library::default();
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(library.playlists, Load::Ready(vec![playlist("a", "A")]));
    assert_eq!(library.tracks, Load::Idle);
}

#[test]
fn a_playlist_failure_is_recorded() {
    let mut library = Library::default();
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert_eq!(library.playlists, Load::Failed(Failure::Load));
}

#[test]
fn select_marks_the_load_and_returns_the_command() {
    let mut library = Library::default();
    let command = library.select("spotify:playlist:a");

    assert_eq!(
        command,
        Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        }
    );
    assert_eq!(library.tracks, Load::Loading);
    assert_eq!(library.selected(), Some("spotify:playlist:a"));
    assert_eq!(
        library.page(),
        Some(&Page::Playlist("spotify:playlist:a".to_owned()))
    );
    assert_eq!(library.wanted.as_deref(), Some("spotify:playlist:a"));
}

#[test]
fn select_of_the_current_playlist_reloads_without_growing_the_history() {
    let mut library = Library::default();
    library.select("a");

    let command = library.select("a");

    assert_eq!(
        command,
        Command::LoadTracks {
            playlist: "a".to_owned()
        }
    );
    assert_eq!(library.tracks, Load::Loading);
    assert_eq!(library.back(), None);
    assert_eq!(library.page(), None);
    assert!(
        !library.can_back(),
        "re-selecting the current page must add no history entry"
    );
}

#[test]
fn selected_is_none_on_an_album_page() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");

    library.open(album("album"));

    assert_eq!(library.page(), Some(&album("album")));
    assert_eq!(library.selected(), None, "the sidebar highlight clears");
    assert_eq!(library.selected_name(), None);
    assert_eq!(library.selected_entry(), None);
}

#[test]
fn open_an_album_starts_its_load_and_keeps_the_tracks() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(
        library.open(album("spotify:album:rec")),
        Some(Command::LoadAlbum {
            album: "spotify:album:rec".to_owned()
        })
    );
    assert_eq!(library.page(), Some(&album("spotify:album:rec")));
    assert_eq!(library.album, Load::Loading);
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")])),
        "the playlist behind the album stays ready for Back"
    );
}

/// A lyrics reply is keyed by track, not by page, so it reaches the panel
/// whatever page is on screen.

#[test]
fn tracks_keep_their_added_at_through_apply() {
    let mut library = Library::default();
    library.select("a");
    let tracks = vec![
        dated("t0", "1700000000000"),
        track("t1"),
        dated("t2", "1792357200000"),
    ];
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: tracks.clone(),
        cover: None,
        access: None,
    });

    assert_eq!(library.tracks, Load::Ready(tracks_for("a", tracks)));
}

#[test]
fn a_reply_for_the_selected_playlist_is_applied() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")]))
    );
}

#[test]
fn a_reply_for_a_stale_playlist_is_discarded() {
    let mut library = Library::default();
    library.select("a");
    library.select("b");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(library.tracks, Load::Loading);
    assert_eq!(library.selected(), Some("b"));
}

#[test]
fn a_failure_for_a_stale_playlist_is_discarded() {
    let mut library = Library::default();
    library.select("a");
    library.select("b");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn a_failure_for_the_selected_playlist_is_recorded() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.tracks, Load::Failed(Failure::Load));
}

#[test]
fn the_selected_name_comes_from_a_ready_list() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    assert_eq!(library.selected_name(), Some("A"));
}

#[test]
fn the_selected_name_is_none_while_the_list_reloads() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.reload();
    assert_eq!(library.selected(), Some("a"));
    assert_eq!(library.selected_name(), None);
}

#[test]
fn transport_updates_are_ignored() {
    let mut library = Library::default();
    library.apply(&Update::Transport(TransportUpdate::Ended));
    assert_eq!(library.playlists, Load::Idle);
    assert_eq!(library.tracks, Load::Idle);
}

#[test]
fn device_updates_are_ignored() {
    let mut library = Library::default();
    library.apply(&Update::Devices(DeviceUpdate::Playing(None)));
    library.apply(&Update::Devices(DeviceUpdate::Roster {
        devices: Vec::new(),
        holder: Some("phone".to_owned()),
        held_here: false,
    }));
    assert_eq!(library.playlists, Load::Idle);
    assert_eq!(library.tracks, Load::Idle);
    assert_eq!(library.followed, Load::Idle);
}
