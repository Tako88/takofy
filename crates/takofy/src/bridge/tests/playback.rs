use super::super::*; // the module under test
use super::*; // the shared test harness

#[test]
fn transport_of_maps_every_event() {
    assert_eq!(
        transport_of(&PlaybackEvent::Loading),
        Update::Transport(TransportUpdate::Loading)
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Playing { position_ms: 5 }),
        Update::Transport(TransportUpdate::Position {
            position_ms: 5,
            paused: false,
        })
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Paused { position_ms: 5 }),
        Update::Transport(TransportUpdate::Position {
            position_ms: 5,
            paused: true,
        })
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Ended),
        Update::Transport(TransportUpdate::Ended)
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Unavailable),
        Update::Transport(TransportUpdate::Unavailable)
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Stopped),
        Update::Transport(TransportUpdate::Stopped)
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Duration {
            duration_ms: 222_000,
        }),
        Update::Transport(TransportUpdate::Duration {
            duration_ms: 222_000,
        })
    );
    assert_eq!(
        transport_of(&PlaybackEvent::Seeked {
            position_ms: 41_000
        }),
        Update::Transport(TransportUpdate::Seeked {
            position_ms: 41_000,
        })
    );
}

#[test]
fn playback_failure_names_a_missing_device() {
    assert_eq!(
        playback_failure(&SessionError::Playback(PlaybackError::NoAudioDevice)),
        Failure::NoAudioDevice
    );
    assert_eq!(
        playback_failure(&SessionError::NotConnected),
        Failure::Other
    );
    assert_eq!(
        playback_failure(&SessionError::LinkLost),
        Failure::Reconnect,
        "a load refused for a dead link is a rebuild, not a bad track"
    );
    assert_eq!(
        playback_failure(&SessionError::Playback(PlaybackError::NoAudioBackend)),
        Failure::Other
    );
}

#[test]
fn a_second_play_does_not_start_a_second_player() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        // Reload is processed after the second play, so it is a barrier.
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
    });

    assert_eq!(shared.plays.load(Ordering::SeqCst), 2);
    assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn pause_resume_and_stop_with_no_player_do_not_start_one() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::Pause).expect("pause");
        ui_end.commands.send(Command::Resume).expect("resume");
        ui_end.commands.send(Command::Stop).expect("stop");
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
    });

    assert_eq!(shared.pauses.load(Ordering::SeqCst), 1);
    assert_eq!(shared.resumes.load(Ordering::SeqCst), 1);
    assert_eq!(shared.stops.load(Ordering::SeqCst), 1);
    assert_eq!(shared.players.load(Ordering::SeqCst), 0);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_seek_with_no_player_does_not_start_one() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Seek { position_ms: 12 })
            .expect("seek");
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
    });

    assert_eq!(shared.seeks.load(Ordering::SeqCst), 1);
    assert_eq!(shared.players.load(Ordering::SeqCst), 0);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_seek_reaches_the_host() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
        ui_end
            .commands
            .send(Command::Seek {
                position_ms: 41_000,
            })
            .expect("seek");
        ui_end
            .commands
            .send(Command::Volume { percent: 35 })
            .expect("volume");
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
    });

    assert_eq!(shared.seeks.load(Ordering::SeqCst), 1);
    assert_eq!(shared.last_seek.load(Ordering::SeqCst), 41_000);
    assert_eq!(shared.volumes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_volume.load(Ordering::SeqCst),
        35,
        "the level the window drew is the level the player was told"
    );
    assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_volume_command_ahead_of_the_play_is_the_level_the_player_starts_at() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        ui_end
            .commands
            .send(Command::Volume { percent: 35 })
            .expect("volume");
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
    });

    assert_eq!(
        shared.level_at_player_start.load(Ordering::SeqCst),
        35,
        "the queue delivers in order, and the player is built with the level the host holds"
    );
    assert_eq!(
        shared.volumes.load(Ordering::SeqCst),
        1,
        "the queue delivers in order, so the one level sent arrives once"
    );
    assert_eq!(
        shared.players.load(Ordering::SeqCst),
        1,
        "the queue delivers in order, so the play after the level starts one player"
    );
}

#[test]
fn a_player_with_no_volume_command_starts_at_full() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
    });

    assert_eq!(
        shared.level_at_player_start.load(Ordering::SeqCst),
        100,
        "a player started without a level is at full, not silent"
    );
    assert_eq!(shared.volumes.load(Ordering::SeqCst), 0);
}

#[test]
fn playback_events_reach_the_window() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let window_shared = Arc::clone(&shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

    let update = run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
        let tx = window_shared
            .event_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("event sender");
        tx.send(PlaybackEvent::Playing { position_ms: 7 })
            .expect("event");
        ui_end.updates.recv().expect("transport")
    });

    assert_eq!(
        update,
        Update::Transport(TransportUpdate::Position {
            position_ms: 7,
            paused: false,
        })
    );
}

#[test]
fn a_stop_during_tracks_shuts_down_before_the_task_returns() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.tracks_script.lock().expect("lock") = TracksScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.tracks_started.lock().expect("lock") = Some(started_tx);

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
            .send(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            })
            .expect("load");
        started_rx.recv().expect("tracks started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the track load");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the track load"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_with_the_player_started_shuts_down_before_the_task_returns() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

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
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts playback");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run after a player was started"
        );
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_burst_of_play_seek_pause_resume_then_stop_shuts_down_once() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);
    let commands = ui_end.commands.clone();

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");
        ui_end
            .commands
            .send(Command::Seek {
                position_ms: 41_000,
            })
            .expect("seek");
        ui_end.commands.send(Command::Pause).expect("pause");
        ui_end.commands.send(Command::Resume).expect("resume");
        ui_end.commands.send(Command::Stop).expect("stop");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve after a burst of commands");
        assert!(
            commands.is_closed(),
            "the task must have returned before finish resolves"
        );
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "a burst of commands must still shut down exactly once"
        );
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn commands_sent_after_the_stop_are_dropped_not_run() {
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
        // The task is parked on the pending rootlist, so these queue up
        // behind it and the stop is what resumes the task.
        ui_end
            .commands
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("queued play");
        ui_end
            .commands
            .send(Command::Seek { position_ms: 5 })
            .expect("queued seek");
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve");
        assert!(
            ui_end.commands.is_closed(),
            "the task must have returned before finish resolves"
        );
        assert_eq!(shared.plays.load(Ordering::SeqCst), 0);
        assert_eq!(shared.seeks.load(Ordering::SeqCst), 0);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn playback_events_after_the_window_closes_do_not_wedge_the_drain_thread() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let window_shared = Arc::clone(&shared);
    let (play_started_tx, play_started_rx) = channel();
    *shared.play_started.lock().expect("lock") = Some(play_started_tx);

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
            .send(Command::Play {
                uri: "spotify:track:x".to_owned(),
            })
            .expect("play");
        play_started_rx.recv().expect("play started");

        // The window goes away, then playback events keep arriving. The
        // drain thread must survive the closed receiver.
        let UiEnd { updates, commands } = ui_end;
        drop(updates);
        drop(commands);
        let tx = window_shared
            .event_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("event sender");
        tx.send(PlaybackEvent::Ended).expect("event");
        tx.send(PlaybackEvent::Unavailable).expect("event");
        window_shared.event_tx.lock().expect("lock").take();
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve with a window that closed first");
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}
