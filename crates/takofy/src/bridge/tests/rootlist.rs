use super::super::*; // the module under test
use super::*; // the shared test harness

#[test]
fn rootlist_replies_keep_the_order_their_commands_were_sent_in() {
    // the sidebar's read rule rests on this order
    fn run(move_first: bool) -> Vec<Update> {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Ready(revision(2));

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            let move_command = Command::MovePlaylist {
                revision: revision(1),
                from: 0,
                to: 1,
            };
            let reload = Command::ReloadLibrary;
            if move_first {
                ui_end.commands.send(move_command).expect("move");
                ui_end.commands.send(reload).expect("reload");
            } else {
                ui_end.commands.send(reload).expect("reload");
                ui_end.commands.send(move_command).expect("move");
            }
            vec![
                ui_end.updates.recv().expect("first reply"),
                ui_end.updates.recv().expect("second reply"),
            ]
        })
    }

    fn moved(update: &Update) -> bool {
        matches!(update, Update::PlaylistMoved { .. })
    }

    fn read(update: &Update) -> bool {
        matches!(update, Update::Playlists(_))
    }

    let move_first = run(true);
    assert!(
        moved(&move_first[0]) && read(&move_first[1]),
        "{move_first:?}"
    );

    let reload_first = run(false);
    assert!(
        read(&reload_first[0]) && moved(&reload_first[1]),
        "{reload_first:?}"
    );
}

#[test]
fn an_empty_rootlist_is_announced_as_empty() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 2)
    });

    assert!(updates.contains(&Update::Playlists(Vec::new())));
}

#[test]
fn a_failed_connect_is_announced_as_a_library_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Fail, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 2)
    });

    assert!(updates.contains(&Update::SignedIn));
    assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 0);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_rootlist_is_announced_as_a_library_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Fail);
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 2)
    });

    assert!(updates.contains(&Update::SignedIn));
    assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
    assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn reload_library_runs_the_rootlist_again() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let mut seen = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        seen.extend(expect_updates(&ui_end, 1));
        seen
    });

    let listed = updates
        .iter()
        .filter(|update| matches!(update, Update::Playlists(_)))
        .count();
    assert_eq!(listed, 2);
    assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 2);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_second_token_does_not_reconnect() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::with(
        Ok(token_set("access-one", None)),
        Ok(token_set("access-two", Some("refresh-two"))),
    ));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let mut seen = expect_updates(&ui_end, 2);
        let mut sign_ins = seen
            .iter()
            .filter(|update| **update == Update::SignedIn)
            .count();

        ui_end.commands.send(Command::Authorize).expect("authorize");
        while sign_ins < 2 {
            let update = ui_end.updates.recv().expect("update");
            if update == Update::SignedIn {
                sign_ins += 1;
            }
            seen.push(update);
        }

        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        loop {
            let update = ui_end.updates.recv().expect("update");
            let listed = matches!(update, Update::Playlists(_));
            seen.push(update);
            if listed {
                break;
            }
        }
        seen
    });

    assert_eq!(
        shared.connects.load(Ordering::SeqCst),
        1,
        "a second token must not open a second session"
    );
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 2);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    let listed = updates
        .iter()
        .filter(|update| matches!(update, Update::Playlists(_)))
        .count();
    assert_eq!(listed, 2);
}

#[test]
fn a_stop_during_the_rootlist_shuts_down_before_the_task_returns() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
    let shared = Arc::clone(&host.shared);
    let (started_tx, started_rx) = channel();
    *shared.rootlist_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        started_rx.recv().expect("rootlist started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the rootlist");
        assert!(
            ui_end.commands.is_closed(),
            "the task must have returned before finish resolves"
        );
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the rootlist"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_during_the_connect_shuts_down_before_the_task_returns() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Pending, RootlistScript::default());
    let shared = Arc::clone(&host.shared);
    let (started_tx, started_rx) = channel();
    *shared.connect_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        started_rx.recv().expect("connect started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the connect");
        assert!(
            ui_end.commands.is_closed(),
            "the task must have returned before finish resolves"
        );
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the connect"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn reload_library_retries_a_failed_connect() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::with_connect_scripts(
        vec![ConnectScript::Fail, ConnectScript::Ok],
        RootlistScript::Ready(list.clone()),
    );
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let mut seen = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        seen.extend(expect_updates(&ui_end, 1));
        seen
    });

    assert!(updates.contains(&Update::SignedIn));
    assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
    assert!(updates.contains(&Update::Playlists(list)));
    assert_eq!(
        shared.connects.load(Ordering::SeqCst),
        2,
        "a retry after a failed connect must re-enter connect"
    );
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn reload_with_no_token_announces_a_library_failure() {
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
            .send(Command::ReloadLibrary)
            .expect("reload");
        ui_end.updates.recv().expect("library failure")
    });

    assert_eq!(update, Update::PlaylistFailed(Failure::Load));
    assert_eq!(shared.connects.load(Ordering::SeqCst), 0);
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 0);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn run_ordered_returns_the_window_result() {
    let result = run_ordered(runtime(), || 42, || async {});
    assert_eq!(result, 42);
}
