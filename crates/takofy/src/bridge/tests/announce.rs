use super::super::*;
use super::test_support::*;
use crate::session::{SessionError, TrackPage};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{RecvTimeoutError, channel};
use takofy_model::{AlbumDetail, CoverSet, FollowedArtist, FollowedShow, SavedAlbum, ShowDetail};

#[test]
fn announce_sends_tracks_for_a_page() {
    let (tx, rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let playlist = "spotify:playlist:a".to_owned();
    let cover =
        CoverSet::single(CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover"));
    announce(
        &notifier,
        Ok(TrackPage {
            tracks: vec![track("t")],
            cover: Some(cover.clone()),
            access: None,
        }),
        |page| Update::Tracks {
            playlist: playlist.clone(),
            tracks: page.tracks,
            cover: page.cover,
            access: None,
        },
        |failure| Update::TracksFailed {
            playlist: playlist.clone(),
            failure,
        },
    );
    assert_eq!(
        rx.recv().expect("update"),
        Update::Tracks {
            playlist,
            tracks: vec![track("t")],
            cover: Some(cover),
            access: None,
        }
    );
}

#[test]
fn announce_sends_tracks_failed_for_an_error() {
    let (tx, rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let playlist = "spotify:playlist:a".to_owned();
    let result: Result<TrackPage, SessionError> = Err(SessionError::NotConnected);
    announce(
        &notifier,
        result,
        |page| Update::Tracks {
            playlist: playlist.clone(),
            tracks: page.tracks,
            cover: page.cover,
            access: None,
        },
        |failure| Update::TracksFailed {
            playlist: playlist.clone(),
            failure,
        },
    );
    assert_eq!(
        rx.recv().expect("update"),
        Update::TracksFailed {
            playlist,
            failure: Failure::Load,
        }
    );
}

#[test]
fn a_load_tracks_before_connect_announces_a_failure() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        ui_end
            .commands
            .send(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::TracksFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.tracks.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_load_album_before_connect_announces_a_failure() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        ui_end
            .commands
            .send(Command::LoadAlbum {
                album: "spotify:album:a".to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.albums.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

/// Drives one scripted album through the live `load_album` path and returns
/// the update the window receives, so every hop from the host's answer to the
/// window's vocabulary is real.
fn run_album(detail: AlbumDetail) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.album_script.lock().expect("lock") = AlbumScript::Ready(detail);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadAlbum {
                album: "spotify:album:a".to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("album")
    })
}

#[test]
fn a_scripted_album_reaches_the_window() {
    let detail = album_detail("spotify:album:a");
    assert_eq!(run_album(detail.clone()), Update::Album(detail));
}

#[test]
fn a_failed_album_load_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.album_script.lock().expect("lock") = AlbumScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadAlbum {
                album: "spotify:album:a".to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.albums.load(Ordering::SeqCst), 1);
}

#[test]
fn a_load_show_before_connect_announces_a_failure() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        ui_end
            .commands
            .send(Command::LoadShow {
                show: SHOW.to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::ShowFailed {
            show: SHOW.to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.shows.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

/// Drives one scripted show through the live `load_show` path and returns
/// the update the window receives, so every hop from the host's answer to the
/// window's vocabulary is real.
fn run_show(detail: ShowDetail) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.show_script.lock().expect("lock") = ShowScript::Ready(detail);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadShow {
                show: SHOW.to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("show")
    })
}

#[test]
fn a_scripted_show_reaches_the_window() {
    let detail = show_detail(SHOW);
    assert_eq!(run_show(detail.clone()), Update::Show(detail));
}

#[test]
fn a_failed_show_load_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.show_script.lock().expect("lock") = ShowScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadShow {
                show: SHOW.to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::ShowFailed {
            show: SHOW.to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.shows.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_during_a_show_load_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.show_script.lock().expect("lock") = ShowScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.show_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        ui_end
            .commands
            .send(Command::LoadShow {
                show: SHOW.to_owned(),
            })
            .expect("load");
        started_rx.recv().expect("show started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the show load");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the show load"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_followed_load_before_connect_announces_followed_failure() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::FollowedShowsFailed(Failure::Followed),
        "a followed failure is not the library's"
    );
    assert_eq!(shared.followed_lists.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

/// Drives one scripted followed list through the live `load_followed_shows`
/// path and returns the update the window receives.
fn run_followed(shows: Vec<FollowedShow>) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.followed_script.lock().expect("lock") = FollowedScript::Ready(shows);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("load");
        ui_end.updates.recv().expect("followed")
    })
}

/// One followed-show card, named after its URI so a test can tell two apart.
fn followed_show(uri: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

#[test]
fn a_scripted_followed_list_reaches_the_window() {
    let shows = vec![
        followed_show("spotify:show:a"),
        followed_show("spotify:show:b"),
    ];
    assert_eq!(run_followed(shows.clone()), Update::FollowedShows(shows));
}

#[test]
fn a_failed_followed_load_is_announced_as_followed() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.followed_script.lock().expect("lock") = FollowedScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(update, Update::FollowedShowsFailed(Failure::Followed));
    assert_eq!(shared.followed_lists.load(Ordering::SeqCst), 1);
}

#[test]
fn a_signed_in_window_gets_no_followed_update_unprompted() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        assert_eq!(
            ui_end.updates.recv_timeout(Duration::from_millis(250)),
            Err(RecvTimeoutError::Timeout),
            "the worker must not load the followed list on its own"
        );
    });
    assert_eq!(
        shared.followed_lists.load(Ordering::SeqCst),
        0,
        "the list is want-driven, never chained onto the rootlist"
    );
}

#[test]
fn a_followed_artists_load_before_connect_announces_followed_artists_failure() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        ui_end
            .commands
            .send(Command::LoadFollowedArtists)
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::FollowedArtistsFailed(Failure::FollowedArtists),
        "a followed-artists failure is not the library's"
    );
    assert_eq!(shared.followed_artist_lists.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

/// Drives one scripted followed-artist list through the live
/// `load_followed_artists` path and returns the update the window receives.
fn run_followed_artists(artists: Vec<FollowedArtist>) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.followed_artists_script.lock().expect("lock") = FollowedArtistsScript::Ready(artists);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadFollowedArtists)
            .expect("load");
        ui_end.updates.recv().expect("followed artists")
    })
}

#[test]
fn a_scripted_followed_artist_list_reaches_the_window() {
    let artists = vec![
        followed_artist("spotify:artist:a"),
        followed_artist("spotify:artist:b"),
    ];
    assert_eq!(
        run_followed_artists(artists.clone()),
        Update::FollowedArtists(artists)
    );
}

#[test]
fn a_failed_followed_artists_load_is_announced_as_followed_artists() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.followed_artists_script.lock().expect("lock") = FollowedArtistsScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadFollowedArtists)
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::FollowedArtistsFailed(Failure::FollowedArtists)
    );
    assert_eq!(shared.followed_artist_lists.load(Ordering::SeqCst), 1);
}

#[test]
fn a_signed_in_window_gets_no_followed_artist_update_unprompted() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        assert_eq!(
            ui_end.updates.recv_timeout(Duration::from_millis(250)),
            Err(RecvTimeoutError::Timeout),
            "the worker must not load the followed-artist list on its own"
        );
    });
    assert_eq!(
        shared.followed_artist_lists.load(Ordering::SeqCst),
        0,
        "the list is want-driven, never chained onto the rootlist"
    );
}

#[test]
fn a_saved_albums_load_before_connect_announces_saved_albums_failure() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        ui_end
            .commands
            .send(Command::LoadSavedAlbums)
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::SavedAlbumsFailed(Failure::SavedAlbums),
        "a saved-albums failure is not the library's"
    );
    assert_eq!(shared.saved_album_lists.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

/// Drives one scripted saved-album list through the live `load_saved_albums`
/// path and returns the update the window receives.
fn run_saved_albums(albums: Vec<SavedAlbum>) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.saved_albums_script.lock().expect("lock") = SavedAlbumsScript::Ready(albums);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadSavedAlbums)
            .expect("load");
        ui_end.updates.recv().expect("saved albums")
    })
}

/// One saved-album card, named after its URI so a test can tell two apart.
fn saved_album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        year: None,
        cover: None,
        tracks: None,
    }
}

#[test]
fn a_scripted_saved_album_list_reaches_the_window() {
    let albums = vec![
        saved_album("spotify:album:a"),
        saved_album("spotify:album:b"),
    ];
    assert_eq!(
        run_saved_albums(albums.clone()),
        Update::SavedAlbums(albums)
    );
}

#[test]
fn a_failed_saved_albums_load_is_announced_as_saved_albums() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.saved_albums_script.lock().expect("lock") = SavedAlbumsScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadSavedAlbums)
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(update, Update::SavedAlbumsFailed(Failure::SavedAlbums));
    assert_eq!(shared.saved_album_lists.load(Ordering::SeqCst), 1);
}

#[test]
fn a_signed_in_window_gets_no_saved_album_update_unprompted() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        assert_eq!(
            ui_end.updates.recv_timeout(Duration::from_millis(250)),
            Err(RecvTimeoutError::Timeout),
            "the worker must not load the saved-album list on its own"
        );
    });
    assert_eq!(
        shared.saved_album_lists.load(Ordering::SeqCst),
        0,
        "the list is want-driven, never chained onto the rootlist"
    );
}
