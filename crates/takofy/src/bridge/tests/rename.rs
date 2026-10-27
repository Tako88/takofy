use super::super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use takofy_model::PlaylistAccess;

#[test]
fn a_rename_reads_the_state_then_writes_and_announces_the_reply_revision() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });
    *shared.rename_script.lock().expect("lock") = RenameScript::Ready(revision(11));
    let uri = "spotify:playlist:a";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RenamePlaylist {
                playlist: uri.to_owned(),
                name: "Night Drive".to_owned(),
            })
            .expect("rename");
        ui_end.updates.recv().expect("renamed")
    });

    assert_eq!(
        update,
        Update::PlaylistRenamed {
            playlist: uri.to_owned(),
            name: "Night Drive".to_owned(),
            revision: revision(11),
        }
    );
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
    assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 1);
    assert_eq!(shared.create_writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        shared.last_rename.lock().expect("lock").clone(),
        Some((uri.to_owned(), revision(9), "Night Drive".to_owned())),
        "the rename must name the revision the state read answered"
    );
}

#[test]
fn a_rename_of_a_list_that_is_not_editable_is_refused_without_a_write() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: false,
    });
    let uri = "spotify:playlist:a";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RenamePlaylist {
                playlist: uri.to_owned(),
                name: "Night Drive".to_owned(),
            })
            .expect("rename");
        ui_end.updates.recv().expect("failed")
    });

    assert_eq!(
        update,
        Update::PlaylistRenameFailed {
            playlist: uri.to_owned(),
        }
    );
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
    assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 0);
}

#[test]
fn a_rename_with_no_state_fails_without_a_write() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let uri = "spotify:playlist:a";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RenamePlaylist {
                playlist: uri.to_owned(),
                name: "Night Drive".to_owned(),
            })
            .expect("rename");
        ui_end.updates.recv().expect("failed")
    });

    assert_eq!(
        update,
        Update::PlaylistRenameFailed {
            playlist: uri.to_owned(),
        }
    );
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
    assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 0);
}

#[test]
fn a_failed_rename_write_announces_the_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });
    *shared.rename_script.lock().expect("lock") = RenameScript::Fail;
    let uri = "spotify:playlist:a";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RenamePlaylist {
                playlist: uri.to_owned(),
                name: "Night Drive".to_owned(),
            })
            .expect("rename");
        ui_end.updates.recv().expect("failed")
    });

    assert_eq!(
        update,
        Update::PlaylistRenameFailed {
            playlist: uri.to_owned(),
        }
    );
    assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_with_a_rename_in_flight_shuts_down() {
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
            .send(Command::RenamePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                name: "Night Drive".to_owned(),
            })
            .expect("rename");
        started_rx.recv().expect("rename started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the rename");
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}
