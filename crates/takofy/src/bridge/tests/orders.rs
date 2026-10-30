use super::super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use takofy_model::PlaylistAccess;

#[test]
fn a_playlist_move_announces_the_reply_revision_and_sends_what_was_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let base = revision(1);
    let reply = revision(2);
    *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Ready(reply.clone());

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::MovePlaylist {
                revision: base.clone(),
                from: 1,
                to: 4,
            })
            .expect("move");
        ui_end.updates.recv().expect("moved")
    });

    assert_eq!(update, Update::PlaylistMoved { revision: reply });
    assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_root_move.lock().expect("lock").clone(),
        Some((base, 1, 4)),
        "the write names the revision and the gap that were asked for"
    );
}

#[test]
fn a_failed_playlist_move_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::MovePlaylist {
                revision: revision(1),
                from: 0,
                to: 1,
            })
            .expect("move");
        ui_end.updates.recv().expect("move failed")
    });

    assert_eq!(update, Update::PlaylistMoveFailed);
    assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_playlist_move_before_connect_is_announced_as_a_failure() {
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
            .send(Command::MovePlaylist {
                revision: revision(1),
                from: 0,
                to: 1,
            })
            .expect("move");
        ui_end.updates.recv().expect("move failed")
    });

    assert_eq!(update, Update::PlaylistMoveFailed);
    assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_with_the_playlist_move_pending_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.rootlist_move_started.lock().expect("lock") = Some(started_tx);

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
            .send(Command::MovePlaylist {
                revision: revision(1),
                from: 0,
                to: 1,
            })
            .expect("move");
        started_rx.recv().expect("move started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the move");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the move"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_playlist_removal_announces_the_reply_revision_and_sends_what_was_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let base = revision(1);
    let reply = revision(2);
    *shared.remove_playlist_script.lock().expect("lock") = RootlistMoveScript::Ready(reply.clone());

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::RemovePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                index: 3,
            })
            .expect("remove");
        ui_end.updates.recv().expect("removed")
    });

    assert_eq!(
        update,
        Update::PlaylistRemoved {
            playlist: "spotify:playlist:a".to_owned(),
            revision: reply,
        }
    );
    assert_eq!(shared.remove_playlist_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_playlist_remove.lock().expect("lock").clone(),
        Some((base, 3)),
        "the unlink names the revision and the row that were asked for"
    );
}

#[test]
fn a_failed_playlist_removal_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.remove_playlist_script.lock().expect("lock") = RootlistMoveScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::RemovePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                index: 0,
            })
            .expect("remove");
        ui_end.updates.recv().expect("remove failed")
    });

    assert_eq!(
        update,
        Update::PlaylistRemoveFailed {
            playlist: "spotify:playlist:a".to_owned(),
        }
    );
    assert_eq!(shared.remove_playlist_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_with_the_playlist_removal_pending_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.remove_playlist_script.lock().expect("lock") = RootlistMoveScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.remove_playlist_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::RemovePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                index: 0,
            })
            .expect("remove");
        started_rx.recv().expect("remove started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the removal");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the removal"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_saved_probe_reaches_the_host_and_its_answer_comes_back() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.probe_answer.lock().expect("lock") = Some(true);
    let album = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LoadSaved {
                uri: album.to_owned(),
            })
            .expect("probe");
        ui_end.updates.recv().expect("answered")
    });

    assert_eq!(
        update,
        Update::SavedState {
            uri: album.to_owned(),
            saved: Some(true),
        }
    );
    assert_eq!(shared.probes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_probe.lock().expect("lock").clone(),
        Some(album.to_owned())
    );
}

#[test]
fn a_link_puts_the_playlist_on_the_rootlist_and_announces_the_new_revision() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let gone = "spotify:playlist:gone";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LinkPlaylist {
                playlist: gone.to_owned(),
                revision: revision(0x01),
            })
            .expect("link");
        ui_end.updates.recv().expect("linked")
    });

    assert!(matches!(
        update,
        Update::PlaylistLinked { playlist, .. } if playlist == gone
    ));
    assert_eq!(shared.link_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared
            .last_link
            .lock()
            .expect("lock")
            .clone()
            .map(|(_, playlist)| playlist),
        Some(gone.to_owned())
    );
}

#[test]
fn a_saved_write_announces_the_uri_and_the_direction_it_was_asked_for() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let album = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::SetSaved {
                uri: album.to_owned(),
                saved: true,
            })
            .expect("save");
        ui_end.updates.recv().expect("saved")
    });

    assert_eq!(
        update,
        Update::Saved {
            uri: album.to_owned(),
            saved: true,
        },
        "the reply is empty, so the window hears what it asked for"
    );
    assert_eq!(shared.remove_saved_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_saved_remove.lock().expect("lock").clone(),
        Some((album.to_owned(), true))
    );
}

#[test]
fn a_failed_saved_write_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.remove_saved_script.lock().expect("lock") = LikeScript::Fail;
    let show = "spotify:show:4uLU6hMCjMI75M1A2tKUQC";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::SetSaved {
                uri: show.to_owned(),
                saved: false,
            })
            .expect("remove");
        ui_end.updates.recv().expect("remove failed")
    });

    assert_eq!(
        update,
        Update::SavedFailed {
            uri: show.to_owned(),
            saved: false,
        }
    );
    assert_eq!(shared.remove_saved_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_during_a_saved_removal_ends_the_task() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.remove_saved_script.lock().expect("lock") = LikeScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.remove_saved_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::SetSaved {
                uri: "spotify:album:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
                saved: false,
            })
            .expect("remove");
        started_rx.recv().expect("remove started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the removal");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the removal"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn an_add_announces_the_reply_revision_and_sends_what_was_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let base = revision(1);
    let reply = revision(2);
    *shared.add_script.lock().expect("lock") = AddScript::Ready(reply.clone());
    let tracks = vec![
        "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
        "spotify:track:4uLU6hMCjMI75M1A2tKUQD".to_owned(),
        "spotify:track:4uLU6hMCjMI75M1A2tKUQE".to_owned(),
    ];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::AddTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                from_index: 7,
                tracks: tracks.clone(),
            })
            .expect("add");
        ui_end.updates.recv().expect("added")
    });

    assert_eq!(
        update,
        Update::TrackAdded {
            playlist: "spotify:playlist:a".to_owned(),
            revision: reply,
        }
    );
    assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1, "one write");
    assert_eq!(
        shared.last_add.lock().expect("lock").clone(),
        Some(("spotify:playlist:a".to_owned(), base, 7, tracks)),
        "the write names the playlist, the revision, the index and the whole pick"
    );
}

#[test]
fn a_failed_add_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.add_script.lock().expect("lock") = AddScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::AddTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                from_index: 0,
                tracks: vec!["spotify:track:x".to_owned()],
            })
            .expect("add");
        ui_end.updates.recv().expect("add failed")
    });

    assert_eq!(
        update,
        Update::AddFailed {
            playlist: "spotify:playlist:a".to_owned(),
        }
    );
    assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn an_add_before_connect_is_announced_as_a_failure() {
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
            .send(Command::AddTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                from_index: 0,
                tracks: vec!["spotify:track:x".to_owned()],
            })
            .expect("add");
        ui_end.updates.recv().expect("add failed")
    });

    assert_eq!(
        update,
        Update::AddFailed {
            playlist: "spotify:playlist:a".to_owned(),
        }
    );
    assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_with_the_add_pending_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.add_script.lock().expect("lock") = AddScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.add_started.lock().expect("lock") = Some(started_tx);

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
            .send(Command::AddTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                from_index: 0,
                tracks: vec!["spotify:track:x".to_owned()],
            })
            .expect("add");
        started_rx.recv().expect("add started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the add");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the add"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_with_the_rename_pending_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });
    *shared.rename_script.lock().expect("lock") = RenameScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.rename_started.lock().expect("lock") = Some(started_tx);

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
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        started_rx.recv().expect("rename started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the rename");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the rename"
        );
    };

    run_ordered(runtime, window, teardown);
}
