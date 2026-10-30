use super::super::wake::Waker;
use super::super::*;
use super::test_support::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use takofy_core::engine::RootlistRead;
use takofy_model::PlaylistAccess;

/// A waker that peeks at the channel when it runs, so the test can see
/// whether the update was queued first.
pub(super) struct QueueProbe {
    rx: Mutex<Receiver<Update>>,
    seen: Mutex<Option<Update>>,
    wakes: AtomicUsize,
}

impl QueueProbe {
    pub(super) fn new(rx: Receiver<Update>) -> Self {
        Self {
            rx: Mutex::new(rx),
            seen: Mutex::new(None),
            wakes: AtomicUsize::new(0),
        }
    }

    pub(super) fn seen(&self) -> Option<Update> {
        self.seen.lock().expect("lock").clone()
    }

    pub(super) fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

impl Waker for QueueProbe {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        let seen = self.rx.lock().expect("lock").try_recv().ok();
        *self.seen.lock().expect("lock") = seen;
    }
}

#[test]
fn every_update_wakes_once_after_it_is_queued() {
    let (tx, rx) = channel();
    let slot = WakeSlot::default();
    let probe = Arc::new(QueueProbe::new(rx));
    slot.install(probe.clone());
    let notifier = Notifier { tx, slot };

    notifier.send(Update::SignInFailed(Failure::PortBusy));

    assert_eq!(probe.wakes(), 1);
    assert_eq!(probe.seen(), Some(Update::SignInFailed(Failure::PortBusy)));
}

#[test]
fn an_update_before_the_slot_is_installed_is_queued_without_waking() {
    let (tx, rx) = channel();
    let slot = WakeSlot::default();
    let notifier = Notifier {
        tx,
        slot: slot.clone(),
    };

    notifier.send(Update::NeedsAuthorization);

    let waker = Arc::new(RecordingWaker::default());
    slot.install(waker.clone());
    assert_eq!(waker.wakes(), 0);
    assert_eq!(rx.try_recv().expect("queued"), Update::NeedsAuthorization);
}

#[test]
fn a_closed_receiver_is_not_a_panic() {
    let (tx, rx) = channel();
    let slot = WakeSlot::default();
    let waker = Arc::new(RecordingWaker::default());
    slot.install(waker.clone());
    let notifier = Notifier { tx, slot };

    drop(rx);
    notifier.send(Update::SignedIn);

    assert_eq!(waker.wakes(), 0);
}

#[test]
fn resume_thread_reports_signed_in() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        ui_end.updates.recv().expect("resume update")
    });

    assert_eq!(update, Update::SignedIn);
}

#[test]
fn authorize_command_reports_the_outcome() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::on_interactive(Ok(token_set(
        "access",
        Some("refresh"),
    ))));

    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let first = ui_end.updates.recv().expect("resume update");
        ui_end.commands.send(Command::Authorize).expect("send");
        let second = ui_end.updates.recv().expect("authorize update");
        (first, second)
    });

    assert_eq!(updates, (Update::NeedsAuthorization, Update::SignedIn));
}

#[test]
fn finish_stops_the_command_task_while_the_ui_end_is_alive() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let runtime = current_thread_runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
    let shared = Arc::clone(&host.shared);
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    // The assertion must run between `finish` and `drop(runtime)`, while
    // the runtime still owns the command task. After the runtime drops, the
    // channel would read as closed even if `finish` never joined.
    let teardown = move || async move {
        worker.finish().await;
        assert!(
            ui_end.commands.is_closed(),
            "closing the command channel is the proof that the task was joined"
        );
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the task must shut the session down on its way out"
        );
    };

    run_ordered(runtime, || {}, teardown);
}

#[test]
fn teardown_runs_when_the_window_closure_returns_an_error() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let runtime = current_thread_runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let ran = Arc::new(AtomicBool::new(false));
    let teardown = {
        let ran = Arc::clone(&ran);
        move || async move {
            // Set before `finish`, so the flag proves teardown ran at all.
            ran.store(true, Ordering::SeqCst);
            worker.finish().await;
            assert!(
                ui_end.commands.is_closed(),
                "closing the command channel is the proof that the task was joined"
            );
        }
    };

    let result: Result<(), &str> = run_ordered(runtime, || Err("window failed"), teardown);

    assert!(result.is_err());
    assert!(
        ran.load(Ordering::SeqCst),
        "teardown must run on the Err path"
    );
}

#[test]
fn window_runs_with_runtime_entered() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());

    let entered = run_window(
        runtime(),
        FakeHost::new(ConnectScript::Ok, RootlistScript::Pending),
        store,
        source,
        |_ui_end, _slot| {
            assert!(tokio::runtime::Handle::try_current().is_ok());
            tokio::spawn(async {}).abort();
            true
        },
    );

    assert!(entered);
}

struct DropGuard {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.log.lock().expect("lock").push("task dropped");
    }
}

#[test]
fn a_pending_task_is_dropped_after_teardown_and_before_runtime_returns() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    let window = {
        let log = Arc::clone(&log);
        move || {
            let (started_tx, started_rx) = channel();
            let guard = DropGuard {
                log: Arc::clone(&log),
            };
            tokio::spawn(async move {
                let _guard = guard;
                let _ = started_tx.send(());
                let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
                let _ = rx.await;
            });
            started_rx.recv().expect("probe task started");
        }
    };

    let teardown = {
        let log = Arc::clone(&log);
        move || async move {
            assert!(
                log.lock().expect("lock").is_empty(),
                "the probe was already dropped when teardown began, so the order proves nothing"
            );
            log.lock().expect("lock").push("teardown");
            worker.finish().await;
        }
    };

    run_ordered(runtime, window, teardown);

    assert_eq!(&*log.lock().expect("lock"), &["teardown", "task dropped"]);
    drop(ui_end);
}

#[test]
fn teardown_can_await_spawn_blocking() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let runtime = runtime();
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
    let (_ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let ran = Arc::new(AtomicBool::new(false));
    let teardown = {
        let ran = Arc::clone(&ran);
        move || async move {
            let inner = Arc::clone(&ran);
            tokio::task::spawn_blocking(move || inner.store(true, Ordering::SeqCst))
                .await
                .expect("blocking task");
            worker.finish().await;
        }
    };

    run_ordered(runtime, || {}, teardown);

    assert!(ran.load(Ordering::SeqCst));
}

#[test]
fn a_token_connects_and_lists_the_rootlist() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list.clone()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 3)
    });

    assert!(updates.contains(&Update::SignedIn));
    assert!(updates.contains(&Update::LibraryReady));
    assert!(updates.contains(&Update::Playlists(list)));
    assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn a_rootlist_read_that_named_a_revision_announces_its_access_right_after_the_list() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let access = PlaylistAccess {
        revision: revision(9),
        editable: true,
    };
    let host = FakeHost::new(
        ConnectScript::Ok,
        RootlistScript::ReadyWith(RootlistRead {
            entries: list.clone(),
            access: Some(access.clone()),
        }),
    );

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 4)
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::LibraryReady,
            Update::Playlists(list),
            Update::RootlistAccess(access),
        ],
        "the access follows the list it belongs to"
    );
}

#[test]
fn a_rootlist_read_with_no_revision_announces_no_access_update() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::new(
        ConnectScript::Ok,
        RootlistScript::ReadyWith(RootlistRead {
            entries: list.clone(),
            access: None,
        }),
    );

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 3)
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::LibraryReady,
            Update::Playlists(list),
        ]
    );
}

#[test]
fn the_walk_is_announced_before_the_labels() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Ready(list)],
        vec![LabelScript::Pending],
    );
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let seen = expect_updates(&ui_end, 2);
        until("the label pass to start", || {
            shared.labelled.load(Ordering::SeqCst) == 1
        });
        expect_no_update(&ui_end);
        seen
    });

    assert_eq!(updates, vec![Update::SignedIn, Update::LibraryReady]);
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 1);
}

#[test]
fn labels_replace_the_walk_and_follow_readiness() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let walked = vec![playlist("spotify:playlist:one", "One")];
    let named = vec![playlist("spotify:playlist:two", "Two")];
    let access = PlaylistAccess {
        revision: revision(5),
        editable: true,
    };
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Ready(walked)],
        vec![LabelScript::Ready(RootlistRead {
            entries: named.clone(),
            access: Some(access.clone()),
        })],
    );

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_updates(&ui_end, 4)
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::LibraryReady,
            Update::Playlists(named),
            Update::RootlistAccess(access),
        ],
        "the walk is drawn only once its labels have named the rows"
    );
}

#[test]
fn a_reload_while_labels_are_pending_walks_again_and_labels_each_walk_once() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let gate = Arc::new(tokio::sync::Notify::new());
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Ready(list.clone())],
        vec![LabelScript::Gated(Arc::clone(&gate))],
    );
    let shared = Arc::clone(&host.shared);
    let in_flight = Arc::clone(&shared);

    let updates = run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let mut seen = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        gate.notify_one();
        seen.push(ui_end.updates.recv().expect("first list"));
        seen.push(ui_end.updates.recv().expect("second readiness"));
        until("the second label pass", || {
            in_flight.labelled.load(Ordering::SeqCst) == 2
        });
        gate.notify_one();
        seen.push(ui_end.updates.recv().expect("second list"));
        seen
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::LibraryReady,
            Update::Playlists(list.clone()),
            Update::LibraryReady,
            Update::Playlists(list),
        ],
        "each walk is labeled once, and its own list follows its own readiness"
    );
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 2);
    assert_eq!(shared.labelled.load(Ordering::SeqCst), 2);
    assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_attempt_then_a_reload_announces_readiness() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Fail, RootlistScript::Ready(list.clone())],
        Vec::new(),
    );
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let mut seen = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::ReloadLibrary)
            .expect("reload");
        seen.extend(expect_library(&ui_end));
        seen
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::PlaylistFailed(Failure::Load),
            Update::LibraryReady,
            Update::Playlists(list),
        ],
        "a failed attempt announces nothing ready; the retry does"
    );
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 2);
    assert_eq!(shared.labelled.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_walk_announces_no_readiness() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Fail);
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_library(&ui_end)
    });

    assert!(
        !updates.contains(&Update::LibraryReady),
        "a walk that failed opens no gate"
    );
    assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
    assert_eq!(shared.labelled.load(Ordering::SeqCst), 0);
}

#[test]
fn a_failed_connect_announces_no_readiness() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Fail, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_library(&ui_end)
    });

    assert!(
        !updates.contains(&Update::LibraryReady),
        "a session that never opened opens no gate"
    );
    assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
    assert_eq!(shared.rootlists.load(Ordering::SeqCst), 0);
}

#[test]
fn a_failed_label_pass_announces_playlist_failed_after_readiness() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Ready(list)],
        vec![LabelScript::Fail],
    );
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        expect_library(&ui_end)
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::LibraryReady,
            Update::PlaylistFailed(Failure::Load),
        ],
        "the readiness stands; only the labels failed"
    );
    assert_eq!(shared.labelled.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_during_the_label_pass_ends_the_worker() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Ready(list)],
        vec![LabelScript::Pending],
    );
    let shared = Arc::clone(&host.shared);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = {
        let shared = Arc::clone(&shared);
        move || {
            until("the label pass to start", || {
                shared.labelled.load(Ordering::SeqCst) == 1
            });
        }
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the label pass");
        assert!(
            ui_end.commands.is_closed(),
            "the task must have returned before finish resolves"
        );
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_write_waits_for_the_label_pass() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:one", "One")];
    let gate = Arc::new(tokio::sync::Notify::new());
    let host = FakeHost::with_rootlist_scripts(
        vec![ConnectScript::Ok],
        vec![RootlistScript::Ready(list.clone())],
        vec![LabelScript::Gated(Arc::clone(&gate))],
    );
    let shared = Arc::clone(&host.shared);
    let in_flight = Arc::clone(&shared);
    *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Ready(revision(2));

    let updates = run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let mut seen = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::MovePlaylist {
                revision: revision(1),
                from: 0,
                to: 1,
            })
            .expect("move");
        expect_no_update(&ui_end);
        assert_eq!(
            in_flight.rootlist_move_writes.load(Ordering::SeqCst),
            0,
            "the write waits for the labels"
        );
        gate.notify_one();
        seen.push(ui_end.updates.recv().expect("list"));
        seen.push(ui_end.updates.recv().expect("move reply"));
        seen
    });

    assert_eq!(
        updates,
        vec![
            Update::SignedIn,
            Update::LibraryReady,
            Update::Playlists(list),
            Update::PlaylistMoved {
                revision: revision(2),
            },
        ],
        "the labels land before the write they held"
    );
    assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
}
