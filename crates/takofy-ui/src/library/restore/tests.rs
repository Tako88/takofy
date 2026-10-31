use std::time::Instant;

use takofy_model::{Account, LikedTrack};

use super::super::tests::test_support::*;
use super::super::*;
use crate::cover_refresh::CoverRefresh;
use crate::library_filter::RootKind;
use crate::naming::Surface;
use crate::sidebar_kind::SidebarKind;

const NOW_MS: u64 = 1_761_134_400_000;

fn account(name: &str) -> Account {
    Account::from_wire(name).expect("an account")
}

fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: None,
    }
}

/// A copy holding one row in every list.
fn copy_of(account: Account) -> Snapshot {
    let mut snapshot = Snapshot::new(account);
    snapshot.playlists = Some(vec![playlist("spotify:playlist:a", "A")]);
    snapshot.shows = Some(vec![followed_show("spotify:show:s")]);
    snapshot.artists = Some(vec![followed_artist("spotify:artist:r")]);
    snapshot.albums = Some(vec![saved_album("spotify:album:r")]);
    snapshot.liked = Some(vec![liked("spotify:track:t")]);
    snapshot
}

fn restored() -> Library {
    let mut library = Library::default();
    library.restore(copy_of(account("dana")));
    library
}

/// A restored library whose account is known and whose session is ready.
fn seated() -> Library {
    let mut library = restored();
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    library
}

#[test]
fn a_restore_draws_every_list_it_holds() {
    let library = restored();

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("spotify:playlist:a", "A")])
    );
    assert_eq!(
        library.followed,
        Load::Ready(vec![followed_show("spotify:show:s")])
    );
    assert_eq!(
        library.followed_artists,
        Load::Ready(vec![followed_artist("spotify:artist:r")])
    );
    assert_eq!(
        library.saved_albums,
        Load::Ready(vec![saved_album("spotify:album:r")])
    );
    assert!(library.likes.is_restored());
    assert_eq!(library.likes.kept(), Some(vec![liked("spotify:track:t")]));
    assert_eq!(library.copy_of, Some(account("dana")));
}

#[test]
fn a_restore_marks_each_list_as_last_session_s() {
    let library = restored();

    for source in [
        library.sources.playlists,
        library.sources.shows,
        library.sources.artists,
        library.sources.albums,
    ] {
        assert_eq!(source, Source::LastSession);
    }
}

#[test]
fn a_list_the_copy_does_not_hold_is_left_unread() {
    let mut library = Library::default();
    let mut snapshot = Snapshot::new(account("dana"));
    snapshot.playlists = Some(vec![playlist("spotify:playlist:a", "A")]);
    library.restore(snapshot);

    assert_eq!(library.followed, Load::Idle);
    assert_eq!(library.followed_artists, Load::Idle);
    assert_eq!(library.saved_albums, Load::Idle);
    assert_eq!(library.likes.kept(), None, "a copy without a like list");
    assert_eq!(library.sources.shows, Source::Live);
}

#[test]
fn a_list_the_session_already_read_is_not_overwritten() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:b",
        "B",
    )]));
    library.restore(copy_of(account("dana")));

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("spotify:playlist:b", "B")])
    );
    assert_eq!(library.sources.playlists, Source::Live);
}

#[test]
fn a_copy_for_another_account_is_not_drawn_at_all() {
    let mut library = Library::default();
    library.apply(&Update::Account(account("dana")));
    library.restore(copy_of(account("erin")));

    assert_eq!(library.copy_of, None);
    assert_eq!(library.playlists, Load::Idle);
    assert_eq!(library.likes.kept(), None);
}

#[test]
fn every_write_gate_is_closed_while_the_copy_is_drawn() {
    // The copy names an editable access and a rootlist revision, so the gates
    // below are the copy's alone: a row from another session may not carry a
    // write, whatever it says about itself.
    let mut library = Library::default();
    let mut snapshot = Snapshot::new(account("dana"));
    snapshot.playlists = Some(vec![with_access(
        playlist("spotify:playlist:a", "A"),
        0x07,
        true,
        Some(3),
    )]);
    snapshot.artists = Some(vec![followed_artist("spotify:artist:r")]);
    snapshot.albums = Some(vec![saved_album("spotify:album:r")]);
    snapshot.shows = Some(vec![followed_show("spotify:show:s")]);
    snapshot.liked = Some(vec![liked("spotify:track:t")]);
    library.restore(snapshot);
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

    assert!(!library.can_remove(SidebarKind::Playlists));
    assert!(!library.can_remove(SidebarKind::Albums));
    assert!(!library.can_remove(SidebarKind::Artists));
    assert!(!library.can_remove(SidebarKind::Podcasts));
    assert!(
        library
            .save_to_library(&item(SidebarKind::Albums, "spotify:album:x"))
            .is_none()
    );
    assert!(
        library
            .save_to_library(&item(SidebarKind::Artists, "spotify:artist:x"))
            .is_none()
    );
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Albums, "spotify:album:r"))
            .is_none()
    );
    assert!(library.move_playlist(0, 1, "spotify:playlist:a").is_none());
    assert!(!library.begin_rename("spotify:playlist:a", Surface::Band));
    assert!(library.create_playlist().is_none());
    assert!(
        library
            .set_liked(names(&["spotify:track:t"]), false)
            .is_none()
    );
    assert!(!library.write_in_flight());
    assert!(
        !library.rootlist_movable(),
        "a restored list offered a lift"
    );
    assert!(!library.creatable(), "a restored list offered a create");

    // An add names a row's own access, and a copy stores none, so no row of it
    // offers a target whatever the fixture above pretends.
    assert!(restored().add_targets().is_empty());
}

#[test]
fn a_live_reply_marks_only_its_own_list() {
    let mut library = restored();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:b",
        "B",
    )]));

    assert_eq!(library.sources.playlists, Source::Live);
    assert_eq!(library.sources.shows, Source::LastSession);
    assert_eq!(library.sources.artists, Source::LastSession);
    assert_eq!(library.sources.albums, Source::LastSession);

    library.apply(&Update::FollowedShows(Vec::new()));
    assert_eq!(library.sources.shows, Source::Live);
    assert_eq!(library.sources.albums, Source::LastSession);
}

#[test]
fn the_asks_fire_once_the_session_is_ready_and_not_before() {
    let mut library = restored();
    library.root_filter.choose(RootKind::All);

    assert_eq!(library.followed_request(), None);
    assert_eq!(library.followed_artists_request(), None);
    assert_eq!(library.saved_albums_request(), None);
    assert_eq!(library.likes_request(), None);

    library.apply(&Update::LibraryReady);
    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums)
    );
    assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
}

#[test]
fn a_restored_list_keeps_its_rows_while_the_refresh_is_asked() {
    let mut library = restored();
    library.apply(&Update::LibraryReady);

    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(
        library.followed,
        Load::Ready(vec![followed_show("spotify:show:s")]),
        "the copy stays on screen while its refresh is in flight"
    );
    assert_eq!(library.sources.shows, Source::Asked);
    assert_eq!(library.followed_request(), None, "one ask per list");
}

#[test]
fn a_failed_refresh_keeps_the_copy_and_says_it_is_old() {
    let mut library = seated();
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert!(matches!(library.playlists, Load::Ready(_)));
    assert_eq!(
        library.sources.playlists,
        Source::Unrefreshed(Failure::Load)
    );
    assert!(library.copy_failed());

    library.apply(&Update::FollowedShowsFailed(Failure::Followed));
    assert!(matches!(library.followed, Load::Ready(_)));
    assert!(library.copy_failed());

    assert!(library.likes_request().is_some());
    library.apply(&Update::LikedTracksFailed(Failure::Load));
    assert!(
        library.likes.kept().is_some(),
        "a failed read keeps the copy's likes"
    );
}

#[test]
fn a_failed_live_read_still_replaces_the_list() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.playlists, Load::Failed(Failure::Load));
    assert!(
        !library.copy_failed(),
        "the session's own list had nothing to keep"
    );

    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
    )]));
    library.apply(&Update::FollowedShowsFailed(Failure::Followed));
    assert_eq!(library.followed, Load::Failed(Failure::Followed));
}

#[test]
fn a_retry_keeps_the_copy_and_hides_the_sentence() {
    let mut library = restored();
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert!(library.copy_failed());

    assert_eq!(library.reload(), Command::ReloadLibrary);
    assert!(
        matches!(library.playlists, Load::Ready(_)),
        "the retry keeps the rows it is refreshing"
    );
    assert_eq!(library.sources.playlists, Source::Asked);
    assert!(!library.copy_failed(), "the sentence goes with the retry");
}

#[test]
fn a_retry_asks_again_for_every_list_the_copy_drew() {
    let mut library = restored();
    library.apply(&Update::Account(account("dana")));
    library.root_filter.choose(RootKind::All);
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.reload(), Command::ReloadLibrary);
    assert!(
        matches!(library.playlists, Load::Ready(_)),
        "the playlists keep their rows while the walk runs"
    );
    assert_eq!(library.sources.playlists, Source::Asked);

    library.apply(&Update::LibraryReady);
    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums)
    );
    assert_eq!(library.sources.shows, Source::Asked);
    assert_eq!(library.sources.artists, Source::Asked);
    assert_eq!(library.sources.albums, Source::Asked);
}

#[test]
fn the_sentence_is_off_for_a_library_that_restored_nothing() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    assert!(!library.copy_failed());
}

#[test]
fn the_same_account_keeps_the_copy_and_another_discards_it() {
    let mut kept = restored();
    kept.apply(&Update::Account(account("dana")));
    assert_eq!(kept.account, Some(account("dana")));
    assert_eq!(kept.copy_of, Some(account("dana")));
    assert!(matches!(kept.playlists, Load::Ready(_)));
    assert!(kept.likes.is_restored());

    let mut dropped = restored();
    dropped.apply(&Update::Account(account("erin")));
    assert_eq!(dropped.account, Some(account("erin")));
    assert_eq!(dropped.copy_of, None);
    assert_eq!(dropped.playlists, Load::Idle);
    assert_eq!(dropped.followed, Load::Idle);
    assert_eq!(dropped.followed_artists, Load::Idle);
    assert_eq!(dropped.saved_albums, Load::Idle);
    assert!(!dropped.likes.is_restored());
    assert_eq!(dropped.likes.kept(), None);
}

#[test]
fn an_account_with_no_copy_is_only_recorded() {
    let mut library = Library::default();
    library.apply(&Update::Account(account("dana")));

    assert_eq!(library.account, Some(account("dana")));
    assert_eq!(library.copy_of, None);
    assert_eq!(library.playlists, Load::Idle);
}

#[test]
fn a_second_different_account_never_repeats_the_discard() {
    let mut library = restored();
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::Account(account("erin")));

    assert_eq!(library.account, Some(account("dana")));
    assert!(
        matches!(library.playlists, Load::Ready(_)),
        "the copy belongs to the account the window kept"
    );
}

#[test]
fn discarding_twice_spares_the_rows_the_session_read() {
    let mut library = restored();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:b",
        "B",
    )]));

    library.discard_copy();
    library.discard_copy();

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("spotify:playlist:b", "B")])
    );
    assert_eq!(library.followed, Load::Idle);
    assert_eq!(library.copy_of, None);
}

#[test]
fn a_copy_waits_for_the_account_before_it_is_saved() {
    let mut library = restored();
    assert_eq!(library.snapshot(), None, "the account is not known yet");

    library.apply(&Update::Account(account("dana")));
    assert!(library.snapshot().is_some());
}

#[test]
fn a_save_waits_for_every_write_in_flight() {
    let mut library = seated();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    assert!(library.snapshot().is_some());

    library.sidebar_pending = Some(SidebarPending::Create);
    assert_eq!(library.snapshot(), None, "a sidebar write is in flight");
    library.sidebar_pending = None;

    assert!(library.likes_request().is_some());
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:t")]));
    assert!(
        library
            .set_liked(names(&["spotify:track:x"]), true)
            .is_some()
    );
    assert_eq!(library.snapshot(), None, "a like write is in flight");
}

#[test]
fn a_save_holds_only_the_lists_that_are_ready() {
    let mut library = Library::default();
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::Playlists(Vec::new()));

    let snapshot = library.snapshot().expect("a copy");

    assert_eq!(snapshot.playlists, Some(Vec::new()));
    assert_eq!(snapshot.shows, None);
    assert_eq!(snapshot.artists, None);
    assert_eq!(snapshot.albums, None);
    assert_eq!(snapshot.liked, None);
}

#[test]
fn a_restored_library_saves_the_copy_it_was_given() {
    let library = seated();
    assert_eq!(library.snapshot(), Some(copy_of(account("dana"))));
}

#[test]
fn a_save_mid_refresh_keeps_the_lists_nothing_replaced() {
    let mut library = seated();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:b",
        "B",
    )]));

    let snapshot = library.snapshot().expect("a copy");

    assert_eq!(
        snapshot.playlists,
        Some(vec![playlist("spotify:playlist:b", "B")])
    );
    assert_eq!(
        snapshot.shows,
        Some(vec![followed_show("spotify:show:s")]),
        "a list no reply replaced is saved as it stands"
    );
}

#[test]
fn a_re_armed_list_is_left_out_and_a_discarded_copy_is_not_written_back() {
    let mut live = Library::default();
    live.apply(&Update::Account(account("dana")));
    live.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    live.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
    )]));
    assert!(live.snapshot().is_some_and(|copy| copy.shows.is_some()));

    live.reload();
    let snapshot = live.snapshot().expect("a copy");
    assert_eq!(
        snapshot.shows, None,
        "a list re-armed for a read says nothing about the library"
    );
    assert_eq!(snapshot.playlists, None);

    let mut dropped = restored();
    dropped.apply(&Update::Account(account("dana")));
    dropped.discard_copy();
    let snapshot = dropped.snapshot().expect("a copy");
    assert_eq!(snapshot.playlists, None);
    assert_eq!(snapshot.liked, None);
}

#[test]
fn a_like_this_session_lands_in_the_copy_beside_the_read() {
    let mut library = seated();
    assert!(library.likes_request().is_some());
    library.apply(&Update::LikedTracks(Vec::new()));

    assert!(
        library
            .set_liked(names(&["spotify:track:x"]), true)
            .is_some()
    );
    library.apply(&Update::Liked {
        tracks: names(&["spotify:track:x"]),
        liked: true,
    });

    let liked = library
        .snapshot()
        .expect("a copy")
        .liked
        .expect("a liked list");
    let uris: Vec<&str> = liked.iter().map(|track| track.uri.as_str()).collect();
    assert_eq!(uris, ["spotify:track:x"]);
}

#[test]
fn an_unlike_drops_the_track_from_the_copy() {
    let mut library = seated();
    assert!(library.likes_request().is_some());
    library.apply(&Update::LikedTracks(vec![
        liked("spotify:track:t"),
        liked("spotify:track:u"),
    ]));

    assert!(
        library
            .set_liked(names(&["spotify:track:t"]), false)
            .is_some()
    );
    library.apply(&Update::Liked {
        tracks: names(&["spotify:track:t"]),
        liked: false,
    });

    let liked = library.snapshot().expect("a copy").liked.expect("a list");
    let uris: Vec<&str> = liked.iter().map(|track| track.uri.as_str()).collect();
    assert_eq!(uris, ["spotify:track:u"]);
}

#[test]
fn a_failed_like_puts_the_track_back_in_the_copy() {
    let mut library = seated();
    assert!(library.likes_request().is_some());
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:t")]));

    assert!(
        library
            .set_liked(names(&["spotify:track:t"]), false)
            .is_some()
    );
    library.apply(&Update::LikeFailed {
        tracks: names(&["spotify:track:t"]),
        failure: Failure::Save,
    });

    let liked = library.snapshot().expect("a copy").liked.expect("a list");
    assert_eq!(liked.len(), 1);
    assert_eq!(liked[0].uri, "spotify:track:t");
}

#[test]
fn a_rename_a_removal_and_an_add_reach_the_copy() {
    let mut library = sidebar_library(&["start"], 0x07, true, Some(1));
    library.apply(&Update::Account(account("dana")));

    library.renaming.open("start", "Start", Surface::Band);
    library.renaming.edit().push_str("Start again");
    assert!(library.rename_playlist().is_some());
    library.apply(&Update::PlaylistRenamed {
        playlist: "start".to_owned(),
        name: "Start again".to_owned(),
        revision: revision(0x08),
    });
    let names: Vec<String> = library
        .snapshot()
        .expect("a copy")
        .playlists
        .expect("playlists")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["Start again"]);

    assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());
    library.apply(&Update::TrackAdded {
        playlist: "start".to_owned(),
        revision: revision(0x09),
    });
    let count = library
        .snapshot()
        .expect("a copy")
        .playlists
        .expect("lists")[0]
        .tracks;
    assert_eq!(count, Some(2));

    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "start"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "start".to_owned(),
        revision: revision(0x0a),
    });
    let left = library
        .snapshot()
        .expect("a copy")
        .playlists
        .expect("lists");
    assert!(left.is_empty(), "a removed playlist leaves the copy too");
}

#[test]
fn every_write_that_changes_a_list_moves_the_generation() {
    let updates = [
        Update::Playlists(Vec::new()),
        Update::PlaylistFailed(Failure::Load),
        Update::PlaylistMoved {
            revision: revision(0x01),
        },
        Update::PlaylistMoveFailed,
        Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x01),
        },
        Update::PlaylistRemoveFailed {
            playlist: "a".to_owned(),
        },
        Update::PlaylistCreated {
            entry: playlist("a", "A"),
            revision: revision(0x01),
        },
        Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "B".to_owned(),
            revision: revision(0x01),
        },
        Update::PlaylistRenameFailed {
            playlist: "a".to_owned(),
        },
        Update::Saved {
            uri: "spotify:album:r".to_owned(),
            saved: true,
        },
        Update::SavedFailed {
            uri: "spotify:album:r".to_owned(),
            saved: true,
        },
        Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x01),
        },
        Update::AddFailed {
            playlist: "a".to_owned(),
        },
        Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        },
        Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x01),
        },
        Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        },
        Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x01),
        },
        Update::RemoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        },
        Update::FollowedShows(Vec::new()),
        Update::FollowedShowsFailed(Failure::Followed),
        Update::FollowedArtists(Vec::new()),
        Update::FollowedArtistsFailed(Failure::FollowedArtists),
        Update::SavedAlbums(Vec::new()),
        Update::SavedAlbumsFailed(Failure::SavedAlbums),
        Update::LikedTracks(Vec::new()),
        Update::LikedTracksFailed(Failure::Load),
        Update::Liked {
            tracks: Vec::new(),
            liked: true,
        },
        Update::LikeFailed {
            tracks: Vec::new(),
            failure: Failure::Save,
        },
        Update::Followed {
            artist: "spotify:artist:r".to_owned(),
            followed: true,
        },
        Update::FollowFailed {
            artist: "spotify:artist:r".to_owned(),
            failure: Failure::Save,
        },
    ];

    for update in updates {
        let mut library = Library::default();
        let before = library.generation();
        library.apply(&update);
        assert!(
            library.generation() > before,
            "{update:?} would not ask for a save"
        );
    }

    let mut quiet = Library::default();
    let before = quiet.generation();
    quiet.apply(&Update::Account(account("dana")));
    quiet.apply(&Update::LibraryReady);
    assert_eq!(
        quiet.generation(),
        before,
        "a non-list update saves nothing"
    );
}

#[test]
fn a_restored_row_asks_for_no_cover_before_the_session_reads_it() {
    let mut library = restored();
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    library.drop_derived_cover("spotify:playlist:a");

    let mut due = CoverRefresh::default();
    due.drop_for("spotify:playlist:a");
    assert_eq!(library.tracks_request(), None);
    assert_eq!(
        library.cover_refresh, due,
        "a closed gate takes no ask, so the cover stays due"
    );

    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        })
    );
}

#[test]
fn a_failed_playlists_refresh_keeps_the_covers_due_until_a_retry_lands() {
    let mut library = restored();
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    library.drop_derived_cover("spotify:playlist:a");
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.tracks_request(), None);

    assert_eq!(library.reload(), Command::ReloadLibrary);
    assert_eq!(
        library.tracks_request(),
        None,
        "the walk has not answered yet"
    );

    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        })
    );
}

#[test]
fn the_live_gate_is_readiness_alone_for_a_library_that_restored_nothing() {
    let mut library = Library::default();
    assert!(!library.playlists_live());

    for load in [Load::Idle, Load::Loading, Load::Failed(Failure::Load)] {
        library.playlists = load;
        assert!(!library.playlists_live());
        assert_eq!(
            library.playlists_live(),
            matches!(library.playlists, Load::Ready(_))
        );
    }

    library.apply(&Update::Playlists(Vec::new()));
    assert!(library.playlists_live());

    let restored = restored();
    assert!(matches!(restored.playlists, Load::Ready(_)));
    assert!(!restored.playlists_live());
}

#[test]
fn a_restored_playlist_is_neither_saved_nor_missing() {
    let library = restored();

    assert_eq!(library.is_saved("spotify:playlist:a"), Some(true));
    assert_eq!(
        library.is_saved("spotify:playlist:gone"),
        None,
        "the copy cannot say a playlist is not saved"
    );
}

#[test]
fn a_failed_playlists_list_answers_nothing_until_a_live_reply() {
    let failed = Library {
        playlists: Load::Failed(Failure::Load),
        ..Library::default()
    };
    assert_eq!(
        failed.is_saved("spotify:playlist:gone"),
        None,
        "a list that never restored claims nothing"
    );

    let mut restored = restored();
    assert_eq!(
        restored.is_saved("spotify:playlist:gone"),
        None,
        "the copy cannot say a playlist is not saved"
    );

    restored.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    assert_eq!(
        restored.is_saved("spotify:playlist:gone"),
        Some(false),
        "a live reply is what answers"
    );
    assert_eq!(restored.is_saved("spotify:playlist:a"), Some(true));
}

#[test]
fn home_asks_nothing_while_the_copy_waits_for_its_refresh() {
    let mut library = Library::starting_home(NOW_MS);
    library.restore(copy_of(account("dana")));
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    assert!(library.likes.any(), "arrange: the copy holds a liked song");

    assert!(library.restored_waiting());
    assert!(
        library.home_asks(Instant::now()).is_empty(),
        "Home draws the copy and asks for nothing against it"
    );
}

#[test]
fn home_resumes_once_every_list_is_the_session_s() {
    let mut library = Library::starting_home(NOW_MS);
    library.restore(copy_of(account("dana")));
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    assert!(library.home_asks(Instant::now()).is_empty());

    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A",
    )]));
    library.apply(&Update::FollowedShows(Vec::new()));
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.apply(&Update::SavedAlbums(Vec::new()));
    assert!(!library.restored_waiting());

    let asks = library.home_asks(Instant::now());
    assert!(
        asks.iter().any(|ask| matches!(
            ask,
            Command::LoadHome(takofy_model::home::HomeAsk::Labels { .. })
        )),
        "the liked cards' labels are asked for once the lists are live: {asks:?}"
    );
}

#[test]
fn a_failed_playlists_refresh_does_not_hold_home() {
    let mut library = Library::starting_home(NOW_MS);
    library.restore(copy_of(account("dana")));
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    library.apply(&Update::FollowedShows(Vec::new()));
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.apply(&Update::SavedAlbums(Vec::new()));
    assert!(library.restored_waiting());

    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert!(
        !library.restored_waiting(),
        "a failed playlists refresh is not a list Home still waits on"
    );
}

#[test]
fn a_failed_connect_releases_home_even_with_the_copy_still_drawn() {
    let mut library = Library::starting_home(NOW_MS);
    library.restore(copy_of(account("dana")));
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    assert!(library.restored_waiting(), "arrange: the copy waits");

    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(
        library.sources.playlists,
        Source::Unrefreshed(Failure::Load)
    );
    assert!(
        !library.restored_waiting(),
        "nothing else will refresh the copy until a Retry"
    );
    assert!(!library.home_held(), "Home waited on a connect that failed");
    assert!(
        !library.home_asks(Instant::now()).is_empty(),
        "Home never asks again if the failed connect keeps holding it"
    );
}

#[test]
fn a_dropped_playlists_reply_releases_home_and_the_reask_clears_the_mark() {
    let mut library = Library::starting_home(NOW_MS);
    library.restore(copy_of(account("dana")));
    library.apply(&Update::Account(account("dana")));
    library.apply(&Update::LibraryReady);
    library.apply(&Update::FollowedShows(Vec::new()));
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.apply(&Update::SavedAlbums(Vec::new()));
    assert!(library.restored_waiting());

    // A sidebar write in flight owns the rootlist, so this reply is dropped.
    library.sidebar_pending = Some(SidebarPending::Create);
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:b",
        "B",
    )]));
    assert!(library.rootlist_stale, "the dropped read owes a fresh one");
    assert!(
        !library.restored_waiting(),
        "the drop proves the session connected, so Home may read"
    );

    library.sidebar_pending = None;
    assert_eq!(library.rootlist_request(), Some(Command::ReloadLibrary));
    assert!(!library.rootlist_stale, "the re-ask takes the mark");
    assert_eq!(library.sources.playlists, Source::Asked);
}
