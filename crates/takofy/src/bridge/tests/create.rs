use super::super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use takofy_model::{CoverSet, Move, PlaylistAccess, PlaylistCover, PlaylistEntry};

/// A scripted one-playlist read; the default answers nothing.
#[derive(Clone, Default)]
pub(super) enum EntryScript {
    #[default]
    Missing,
    Ready(Box<PlaylistEntry>),
}

/// The playlist a scripted create makes.
pub(super) const CREATED: &str = "spotify:playlist:0JQ5DAqbMKFEC4WFtoNRpw";

/// A scripted create; the default makes [`CREATED`].
#[derive(Clone)]
pub(super) enum CreateScript {
    Ready(String),
    Fail,
    Pending,
}

impl Default for CreateScript {
    fn default() -> Self {
        Self::Ready(CREATED.to_owned())
    }
}

/// A scripted rootlist link; the default names a revision the window has
/// not seen.
#[derive(Clone)]
pub(super) enum LinkScript {
    Ready(Revision),
    Fail,
    Pending,
}

impl Default for LinkScript {
    fn default() -> Self {
        Self::Ready(Revision::from_wire(&[4]).expect("a revision"))
    }
}

#[test]
fn a_create_makes_links_reads_state_and_announces_the_entry() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let access = PlaylistAccess {
        revision: revision(9),
        editable: true,
    };
    *shared.state_script.lock().expect("lock") = StateScript::Ready(access.clone());

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("created")
    });

    assert_eq!(
        update,
        Update::PlaylistCreated {
            entry: PlaylistEntry {
                saved_at: None,
                composed: Vec::new(),
                uri: CREATED.to_owned(),
                name: "Night Drive".to_owned(),
                named: true,
                cover: None,
                description: None,
                tracks: Some(0),
                owner: Some(Owner::You),
                access: Some(PlaylistAccess {
                    revision: revision(4),
                    editable: true,
                }),
            },
            revision: revision(4),
        },
        "the row holds the rename's reply, which is what the next write names"
    );
    assert_eq!(shared.create_writes.load(Ordering::SeqCst), 1);
    assert_eq!(shared.link_writes.load(Ordering::SeqCst), 1);
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.rename_writes.load(Ordering::SeqCst),
        1,
        "it was named"
    );
    assert_eq!(
        shared.last_rename.lock().expect("lock").clone(),
        Some((CREATED.to_owned(), revision(9), "Night Drive".to_owned())),
        "the rename names the list the create made, on the read's revision"
    );
}

#[test]
fn a_created_row_carries_the_cover_and_the_mosaic_the_one_playlist_read_gives() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let cover = CoverUrl::from_wire("https://i.scdn.co/image/abcdef").expect("a cover");
    let mosaic: Vec<CoverSet> = ["ab", "cd"]
        .iter()
        .map(|tail| {
            CoverSet::single(
                CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover"),
            )
        })
        .collect();
    let read = PlaylistEntry {
        composed: mosaic.clone(),
        cover: Some(PlaylistCover::derived(CoverSet::single(cover.clone()))),
        description: Some("Made here".to_owned()),
        ..PlaylistEntry {
            saved_at: None,
            uri: CREATED.to_owned(),
            name: "Night Drive".to_owned(),
            named: true,
            cover: None,
            composed: Vec::new(),
            description: None,
            tracks: Some(0),
            owner: Some(Owner::You),
            access: None,
        }
    };
    *shared.entry_script.lock().expect("lock") = EntryScript::Ready(Box::new(read));

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("created")
    });

    let Update::PlaylistCreated { entry, .. } = update else {
        panic!("a create announces the row");
    };
    assert_eq!(
        entry.cover,
        Some(PlaylistCover::derived(CoverSet::single(cover))),
        "the row draws the cover the read gave, not an empty slot"
    );
    assert_eq!(
        entry.composed, mosaic,
        "the created row keeps the mosaic the read composed"
    );
    assert_eq!(entry.description.as_deref(), Some("Made here"));
    assert_eq!(shared.entry_reads.load(Ordering::SeqCst), 1);
}

#[test]
fn the_rename_answers_what_the_add_is_based_on() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });
    *shared.rename_script.lock().expect("lock") = RenameScript::Ready(revision(11));
    *shared.add_script.lock().expect("lock") = AddScript::Ready(revision(12));

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: vec!["spotify:track:a".to_owned()],
            })
            .expect("create");
        ui_end.updates.recv().expect("created")
    });

    let Update::PlaylistCreated { entry, .. } = update else {
        panic!("a create announces the row");
    };
    assert_eq!(
        entry.access.map(|access| access.revision),
        Some(revision(12)),
        "the row holds the add's reply, not the read's revision"
    );
    assert_eq!(
        shared
            .last_add
            .lock()
            .expect("lock")
            .clone()
            .map(|add| add.1),
        Some(revision(11)),
        "the add is based on the rename's reply: a stale base would undo it"
    );
}

#[test]
fn a_create_whose_rename_fails_says_so_and_still_takes_the_pick() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });
    *shared.rename_script.lock().expect("lock") = RenameScript::Fail;

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: vec!["spotify:track:a".to_owned()],
            })
            .expect("create");
        expect_updates(&ui_end, 2)
    });

    let (first, second) = (updates[0].clone(), updates[1].clone());
    let Update::PlaylistCreated { entry, .. } = first else {
        panic!("a create announces the row");
    };
    assert!(!entry.named, "a row the server did not name claims a name");
    assert_eq!(entry.tracks, Some(1), "the pick still lands");
    assert_eq!(
        second,
        Update::PlaylistUnnamed {
            playlist: CREATED.to_owned(),
        },
        "the failure is said after the row, because the row clears the notice"
    );
    assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1);
}

#[test]
fn the_link_uses_the_revision_the_command_named_and_the_uri_step_one_returned() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let base = revision(7);
    *shared.create_script.lock().expect("lock") =
        CreateScript::Ready("spotify:playlist:another".to_owned());

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: base.clone(),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("created")
    });

    assert_eq!(
        shared.last_create.lock().expect("lock").clone(),
        Some("Night Drive".to_owned())
    );
    assert_eq!(
        shared.last_link.lock().expect("lock").clone(),
        Some((base, "spotify:playlist:another".to_owned())),
        "the link names the rootlist revision and the created URI"
    );
    assert!(matches!(
        update,
        Update::PlaylistCreated { entry, .. } if entry.uri == "spotify:playlist:another"
    ));
}

#[test]
fn a_make_failure_announces_create_failed_and_never_links() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.create_script.lock().expect("lock") = CreateScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("create failed")
    });

    assert_eq!(update, Update::PlaylistCreateFailed);
    assert_eq!(shared.create_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.link_writes.load(Ordering::SeqCst),
        0,
        "a create that made nothing linked nothing"
    );
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 0);
}

#[test]
fn a_link_failure_announces_unlinked_and_never_reads_state() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.link_script.lock().expect("lock") = LinkScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("unlinked")
    });

    assert_eq!(update, Update::PlaylistUnlinked);
    assert_eq!(shared.link_writes.load(Ordering::SeqCst), 1);
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 0);
}

#[test]
fn a_create_lands_the_pick_in_the_new_list_as_one_add() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let state = PlaylistAccess {
        revision: revision(9),
        editable: true,
    };
    *shared.state_script.lock().expect("lock") = StateScript::Ready(state.clone());
    *shared.add_script.lock().expect("lock") = AddScript::Ready(revision(10));
    let tracks = vec![
        "spotify:track:a".to_owned(),
        "spotify:track:b".to_owned(),
        "spotify:track:c".to_owned(),
    ];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: tracks.clone(),
            })
            .expect("create");
        ui_end.updates.recv().expect("created")
    });

    assert_eq!(
        update,
        Update::PlaylistCreated {
            entry: PlaylistEntry {
                saved_at: None,
                composed: Vec::new(),
                uri: CREATED.to_owned(),
                name: "Night Drive".to_owned(),
                named: true,
                cover: None,
                description: None,
                tracks: Some(3),
                owner: Some(Owner::You),
                access: Some(PlaylistAccess {
                    revision: revision(10),
                    editable: true,
                }),
            },
            revision: revision(4),
        },
        "the landed count is in the row, so the next append names index 3"
    );
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
    assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1, "one add");
    assert_eq!(
        shared.last_add.lock().expect("lock").clone(),
        Some((CREATED.to_owned(), revision(4), 0, tracks)),
        "the whole pick lands at index zero, on the rename's reply"
    );
}

#[test]
fn a_create_that_could_not_add_the_pick_says_so_after_the_create() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });
    *shared.add_script.lock().expect("lock") = AddScript::Fail;

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: vec!["spotify:track:a".to_owned()],
            })
            .expect("create");
        let created = ui_end.updates.recv().expect("created");
        // The rename landed, so the only failure to report is the pick.
        let unfilled = ui_end.updates.recv().expect("unfilled");
        (created, unfilled)
    });

    assert!(
        matches!(
            &updates.0,
            Update::PlaylistCreated { entry, .. }
                if entry.tracks == Some(0) && entry.named
        ),
        "the list exists, is named, and counts nothing: {:?}",
        updates.0
    );
    assert_eq!(
        updates.1,
        Update::PlaylistUnfilled {
            playlist: CREATED.to_owned(),
        }
    );
    assert_eq!(
        shared.add_writes.load(Ordering::SeqCst),
        1,
        "the refusal is spent with the gesture, not retried"
    );
}

#[test]
fn a_create_with_no_pick_makes_no_add_call() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
        revision: revision(9),
        editable: true,
    });

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("created")
    });

    assert!(matches!(update, Update::PlaylistCreated { entry, .. } if entry.tracks == Some(0)));
    assert_eq!(shared.add_writes.load(Ordering::SeqCst), 0);
}

#[test]
fn a_create_whose_state_read_names_no_revision_adds_nothing_and_says_so() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: vec!["spotify:track:a".to_owned()],
            })
            .expect("create");
        let created = ui_end.updates.recv().expect("created");
        let unnamed = ui_end.updates.recv().expect("unnamed");
        let unfilled = ui_end.updates.recv().expect("unfilled");
        (created, unnamed, unfilled)
    });

    assert!(
        matches!(
            &updates.0,
            Update::PlaylistCreated { entry, .. }
                if entry.tracks == Some(0) && entry.access.is_none() && !entry.named
        ),
        "{:?}",
        updates.0
    );
    assert_eq!(
        updates.1,
        Update::PlaylistUnnamed {
            playlist: CREATED.to_owned(),
        },
        "a list with no revision cannot be renamed either"
    );
    assert_eq!(
        updates.2,
        Update::PlaylistUnfilled {
            playlist: CREATED.to_owned(),
        }
    );
    assert_eq!(
        shared.add_writes.load(Ordering::SeqCst),
        0,
        "a list with no revision names no version to add on"
    );
}

#[test]
fn a_failed_state_read_still_announces_created_with_no_access() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        let created = ui_end.updates.recv().expect("created");
        let unnamed = ui_end.updates.recv().expect("unnamed");
        (created, unnamed)
    });

    assert_eq!(
        updates.0,
        Update::PlaylistCreated {
            entry: PlaylistEntry {
                saved_at: None,
                composed: Vec::new(),
                uri: CREATED.to_owned(),
                name: "Night Drive".to_owned(),
                // Nothing could name it: the read that gives the revision
                // the rename needs did not answer.
                named: false,
                cover: None,
                description: None,
                tracks: Some(0),
                owner: Some(Owner::You),
                access: None,
            },
            revision: revision(4),
        },
        "a list that could not be read is still a list to draw"
    );
    assert_eq!(
        updates.1,
        Update::PlaylistUnnamed {
            playlist: CREATED.to_owned(),
        }
    );
    assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_create_before_connect_is_announced_as_a_failure() {
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
            .send(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision(1),
                tracks: Vec::new(),
            })
            .expect("create");
        ui_end.updates.recv().expect("create failed")
    });

    assert_eq!(update, Update::PlaylistCreateFailed);
    assert_eq!(shared.link_writes.load(Ordering::SeqCst), 0);
}

#[test]
fn a_stop_during_each_step_ends_the_task() {
    for step in ["make", "link", "state"] {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let (started_tx, started_rx) = channel();
        match step {
            "make" => {
                *shared.create_script.lock().expect("lock") = CreateScript::Pending;
                *shared.create_started.lock().expect("lock") = Some(started_tx);
            }
            "link" => {
                *shared.link_script.lock().expect("lock") = LinkScript::Pending;
                *shared.link_started.lock().expect("lock") = Some(started_tx);
            }
            _ => {
                *shared.state_script.lock().expect("lock") = StateScript::Pending;
                *shared.state_started.lock().expect("lock") = Some(started_tx);
            }
        }

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
            started_rx.recv().expect("the step started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .unwrap_or_else(|_| panic!("finish must resolve when the stop interrupts {step}"));
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts {step}"
            );
        };

        run_ordered(runtime, window, teardown);
    }
}

#[test]
fn the_edge_steps_encode_the_gap_the_server_reads_for_the_rootlist() {
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

    // The rootlist reads `to` the same way a playlist does: the gap in the
    // list before the move.
    assert!(write(5, 0, 2).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x02]));
    assert!(write(5, 0, 5).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x05]));
    assert!(write(5, 3, 0).ends_with(&[0x08, 0x03, 0x10, 0x01, 0x18, 0x00]));
    assert!(write(5, 2, 5).ends_with(&[0x08, 0x02, 0x10, 0x01, 0x18, 0x05]));
}

#[test]
fn the_probes_swap_is_a_move_of_row_zero_to_gap_two() {
    // MOV 0, length 1, to 2 swaps entries 0 and 1 on the live account.
    let found = takofy_ui::reorder::step(23, 0, 2).expect("a move");
    assert_eq!((found.from, found.to, found.landed), (0, 2, 1));
    let body = takofy_core::playlist_edit::move_body(
        &revision(1),
        &[Move {
            from: found.from,
            length: 1,
            to: found.to,
        }],
    );
    assert!(body.ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x02]));
}
