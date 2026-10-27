use super::super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use takofy_model::{Move, Removal};

#[test]
fn a_scripted_release_announces_its_labelled_tracks() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.release_script.lock().expect("lock") =
        ReleaseScript::Named(vec![track("spotify:track:a")]);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadReleaseTracks {
                release: "spotify:album:r".to_owned(),
                tracks: vec!["spotify:track:a".to_owned()],
            })
            .expect("labels");
        ui_end.updates.recv().expect("tracks")
    });

    assert_eq!(
        update,
        Update::ReleaseTracks {
            release: "spotify:album:r".to_owned(),
            tracks: vec![track("spotify:track:a")],
        }
    );
    assert_eq!(shared.releases.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_release_load_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.release_script.lock().expect("lock") = ReleaseScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadReleaseTracks {
                release: "spotify:album:r".to_owned(),
                tracks: Vec::new(),
            })
            .expect("labels");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::ReleaseTracksFailed {
            release: "spotify:album:r".to_owned(),
            failure: Failure::Load,
        }
    );
}

#[test]
fn a_set_follow_announces_the_requested_value() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            })
            .expect("follow");
        ui_end.updates.recv().expect("followed")
    });

    assert_eq!(
        update,
        Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        }
    );
    assert_eq!(shared.follow_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_follow.lock().expect("lock").clone(),
        Some((ARTIST.to_owned(), true)),
        "the write is sent with the state the user asked for"
    );
}

#[test]
fn a_failed_follow_write_is_announced_as_a_save_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.follow_script.lock().expect("lock") = FollowScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: false,
            })
            .expect("follow");
        ui_end.updates.recv().expect("follow failed")
    });

    assert_eq!(
        update,
        Update::FollowFailed {
            artist: ARTIST.to_owned(),
            failure: Failure::Save,
        }
    );
}

#[test]
fn a_move_announces_the_reply_revision_and_sends_what_was_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let base = revision(1);
    let reply = revision(2);
    *shared.move_script.lock().expect("lock") = MoveScript::Ready(reply.clone());

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                moves: vec![Move {
                    from: 1,
                    length: 1,
                    to: 4,
                }],
            })
            .expect("move");
        ui_end.updates.recv().expect("moved")
    });

    assert_eq!(
        update,
        Update::TrackMoved {
            playlist: "spotify:playlist:a".to_owned(),
            revision: reply,
        }
    );
    assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_move.lock().expect("lock").clone(),
        Some((
            "spotify:playlist:a".to_owned(),
            base,
            vec![Move {
                from: 1,
                length: 1,
                to: 4,
            }],
        )),
        "the write is sent with the revision and the indices that were asked for"
    );
}

#[test]
fn a_two_move_command_is_one_write_and_one_revision() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let base = revision(1);
    let reply = revision(2);
    *shared.move_script.lock().expect("lock") = MoveScript::Ready(reply.clone());
    let moves = vec![
        Move {
            from: 0,
            length: 1,
            to: 4,
        },
        Move {
            from: 1,
            length: 1,
            to: 4,
        },
    ];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                moves: moves.clone(),
            })
            .expect("move");
        ui_end.updates.recv().expect("moved")
    });

    assert_eq!(
        update,
        Update::TrackMoved {
            playlist: "spotify:playlist:a".to_owned(),
            revision: reply,
        }
    );
    assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_move.lock().expect("lock").clone(),
        Some(("spotify:playlist:a".to_owned(), base, moves)),
        "both moves are one write with one revision"
    );
}

#[test]
fn a_removal_announces_the_reply_revision_and_sends_what_was_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let base = revision(1);
    let reply = revision(2);
    *shared.remove_script.lock().expect("lock") = MoveScript::Ready(reply.clone());
    let removals = vec![
        Removal { from: 5, length: 1 },
        Removal { from: 3, length: 1 },
    ];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RemoveTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                removals: removals.clone(),
            })
            .expect("remove");
        ui_end.updates.recv().expect("removed")
    });

    assert_eq!(
        update,
        Update::TracksRemoved {
            playlist: "spotify:playlist:a".to_owned(),
            revision: reply,
        }
    );
    assert_eq!(shared.remove_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_remove.lock().expect("lock").clone(),
        Some(("spotify:playlist:a".to_owned(), base, removals)),
        "the write is sent with the revision and the chain that was asked for"
    );
}

#[test]
fn a_removal_failure_announces_remove_failed() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.remove_script.lock().expect("lock") = MoveScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RemoveTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                removals: vec![Removal { from: 0, length: 1 }],
            })
            .expect("remove");
        ui_end.updates.recv().expect("remove failed")
    });

    assert_eq!(
        update,
        Update::RemoveFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Save,
        }
    );
    assert_eq!(shared.remove_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_two_run_removal_is_one_write() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.remove_script.lock().expect("lock") = MoveScript::Ready(revision(2));

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::RemoveTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                removals: vec![
                    Removal { from: 6, length: 2 },
                    Removal { from: 1, length: 1 },
                ],
            })
            .expect("remove");
        ui_end.updates.recv().expect("removed")
    });

    assert!(matches!(update, Update::TracksRemoved { .. }));
    assert_eq!(
        shared.remove_writes.load(Ordering::SeqCst),
        1,
        "a gapped pick is one change set, not one write per run"
    );
    assert_eq!(
        shared
            .last_remove
            .lock()
            .expect("lock")
            .as_ref()
            .map(|(_, _, removals)| removals.len()),
        Some(2)
    );
}

#[test]
fn the_edge_steps_encode_the_gap_the_server_reads() {
    let base = revision(1);
    let write = |len: usize, from: usize, boundary: usize| {
        let found = takofy_ui::reorder::step(len, from, boundary).expect("a move");
        takofy_core::playlist_edit::move_body(
            &base,
            &[Move {
                from: found.from,
                length: 1,
                to: found.to,
            }],
        )
    };

    // The server reads `to` as a gap in the list before the move: moving down
    // has to name the gap after the row, or the move lands one short and a
    // one-place nudge reads as a move onto itself.
    assert!(write(5, 0, 2).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x02]));
    assert!(write(5, 0, 5).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x05]));
    assert!(write(5, 3, 0).ends_with(&[0x08, 0x03, 0x10, 0x01, 0x18, 0x00]));
    assert!(write(5, 2, 5).ends_with(&[0x08, 0x02, 0x10, 0x01, 0x18, 0x05]));
}
#[test]
fn a_failed_move_is_a_save_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.move_script.lock().expect("lock") = MoveScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 1,
                }],
            })
            .expect("move");
        ui_end.updates.recv().expect("move failed")
    });

    assert_eq!(
        update,
        Update::MoveFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Save,
        }
    );
    assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_move_before_connect_is_announced_as_a_failure() {
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
            .send(Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 1,
                }],
            })
            .expect("move");
        ui_end.updates.recv().expect("move failed")
    });

    assert_eq!(
        update,
        Update::MoveFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Save,
        }
    );
    assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_with_the_move_pending_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.move_script.lock().expect("lock") = MoveScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.move_started.lock().expect("lock") = Some(started_tx);

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
            .send(Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 1,
                }],
            })
            .expect("move");
        started_rx.recv().expect("move started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
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
fn replies_keep_the_order_their_commands_were_sent_in() {
    // the page's read rule rests on this order
    fn run(move_first: bool) -> Vec<Update> {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.move_script.lock().expect("lock") = MoveScript::Ready(revision(2));

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            let move_command = Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: revision(1),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 1,
                }],
            };
            let load_command = Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            };
            if move_first {
                ui_end.commands.send(move_command).expect("move");
                ui_end.commands.send(load_command).expect("load");
            } else {
                ui_end.commands.send(load_command).expect("load");
                ui_end.commands.send(move_command).expect("move");
            }
            vec![
                ui_end.updates.recv().expect("first reply"),
                ui_end.updates.recv().expect("second reply"),
            ]
        })
    }

    fn moved(update: &Update) -> bool {
        matches!(update, Update::TrackMoved { .. })
    }

    fn read(update: &Update) -> bool {
        matches!(update, Update::Tracks { .. })
    }

    let move_first = run(true);
    assert!(
        moved(&move_first[0]) && read(&move_first[1]),
        "a move sent first must answer first: {move_first:?}"
    );

    let read_first = run(false);
    assert!(
        read(&read_first[0]) && moved(&read_first[1]),
        "a read sent first must answer first: {read_first:?}"
    );
}
