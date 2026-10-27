use super::super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use takofy_core::instruction::Kind;
use takofy_model::{Device, DeviceKind, RequestId};

// ---- the account's devices ----

/// A scripted device refresh; the default answers the phone that holds the
/// sound.
#[derive(Clone, Default)]
pub(super) enum RefreshScript {
    #[default]
    Phone,
    TwoDevices,
    Fail,
    Pending,
}

/// A scripted steer. The default obeys.
#[derive(Clone, Copy, Default)]
pub(super) enum SteerScript {
    #[default]
    Ok,
    Fail,
}

/// A scripted hand-over. The default moves the sound.
#[derive(Clone, Copy, Default)]
pub(super) enum TransferScript {
    #[default]
    Ok,
    Fail,
    NoContext,
    Pending,
}

/// A scripted withdraw. The default answers.
#[derive(Clone, Copy, Default)]
pub(super) enum WithdrawScript {
    #[default]
    Ok,
    Pending,
}

/// A scripted play. The default starts the player.
#[derive(Clone, Copy, Default)]
pub(super) enum PlayScript {
    #[default]
    Ok,
    /// The machine has no output, so nothing can ever play here.
    NoAudioDevice,
    /// One track the player refuses; the player itself is fine.
    NotPlayable,
}

/// The other device every scripted device answer names.
pub(super) const PHONE: &str = "phone";

pub(super) fn tablet() -> Device {
    Device {
        id: "tablet".to_owned(),
        name: "Tablet".to_owned(),
        kind: DeviceKind::Tablet,
    }
}

#[test]
fn core_steer_speaks_every_action() {
    assert_eq!(core_steer(Steer::Pause), CoreSteer::Pause);
    assert_eq!(core_steer(Steer::Resume), CoreSteer::Resume);
    assert_eq!(core_steer(Steer::SkipNext), CoreSteer::SkipNext);
    assert_eq!(core_steer(Steer::SkipPrev), CoreSteer::SkipPrev);
    assert_eq!(core_steer(Steer::SeekTo(60_000)), CoreSteer::SeekTo(60_000));
}

#[test]
fn a_watch_before_connect_says_the_devices_are_unreachable() {
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
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        ui_end.updates.recv().expect("watch failed")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::Failed(Failure::Devices))
    );
    assert_eq!(
        shared.watches.load(Ordering::SeqCst),
        0,
        "a signed-out window never reaches the dealer"
    );
}

#[test]
fn a_pushed_view_reaches_the_window_as_updates() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        let tx = shared
            .device_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("a watch opened a stream");
        tx.send(ClusterEvent::View(Box::new(phone_view())))
            .expect("view");
        expect_updates(&ui_end, 2)
    });

    assert_eq!(updates, vec![device_roster(), device_playing()]);
    assert_eq!(shared.watches.load(Ordering::SeqCst), 1);
}

#[test]
fn a_second_watch_request_is_a_no_op() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        expect_no_update(&ui_end);
    });

    assert_eq!(
        shared.watches.load(Ordering::SeqCst),
        1,
        "the dealer's one start is spent on the first watch"
    );
}

#[test]
fn a_failed_watch_is_retried_by_a_refresh() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Fail, WatchScript::Ready];

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);

        ui_end.commands.send(Command::WatchDevices).expect("watch");
        let failed = ui_end.updates.recv().expect("watch failed");
        assert_eq!(
            failed,
            Update::Devices(DeviceUpdate::Failed(Failure::Devices)),
            "a failed watch is the retryable failure"
        );

        // A refresh after a failed watch starts it again rather than
        // believing it is running.
        let (started_tx, started) = channel();
        *shared.watch_started.lock().expect("lock") = Some(started_tx);
        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        started.recv().expect("watch retried");
        let tx = shared
            .device_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("the retry opened a stream");
        tx.send(ClusterEvent::View(Box::new(phone_view())))
            .expect("view");
        expect_updates(&ui_end, 2)
    });

    assert_eq!(updates, vec![device_roster(), device_playing()]);
    assert_eq!(shared.watches.load(Ordering::SeqCst), 2);
    assert_eq!(shared.refreshes.load(Ordering::SeqCst), 0);
}

#[test]
fn a_spent_dealer_stays_terminal_on_watch_and_refresh() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Spent];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        assert_eq!(
            ui_end.updates.recv().expect("watch failed"),
            Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff)),
            "a start that is spent cannot be retried by asking again"
        );
        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        ui_end.updates.recv().expect("refresh stayed off")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff)),
        "a later refresh keeps the terminal answer"
    );
    assert_eq!(shared.watches.load(Ordering::SeqCst), 2);
    assert_eq!(
        shared.refreshes.load(Ordering::SeqCst),
        0,
        "a spent watch never reaches the announce path"
    );
}

#[test]
fn a_refresh_answers_with_the_roster() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        expect_updates(&ui_end, 2)
    });

    assert_eq!(updates, vec![device_roster(), device_playing()]);
    assert_eq!(shared.refreshes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_refresh_roster_is_not_undone_by_an_older_push() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.refresh_script.lock().expect("lock") = RefreshScript::TwoDevices;

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        let tx = shared
            .device_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("a watch opened a stream");

        tx.send(ClusterEvent::View(Box::new(phone_view())))
            .expect("old roster");
        let _ = expect_updates(&ui_end, 2);

        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        assert_eq!(
            expect_updates(&ui_end, 2),
            vec![
                Update::Devices(DeviceUpdate::Roster {
                    devices: vec![phone(), tablet()],
                    holder: Some(PHONE.to_owned()),
                    held_here: false,
                }),
                device_playing(),
            ]
        );

        tx.send(ClusterEvent::View(Box::new(phone_view())))
            .expect("an older push");
        expect_updates(&ui_end, 2)
    });

    assert_eq!(
        updates,
        vec![device_roster(), device_playing()],
        "a push carrying the old roster replaces the newer one"
    );
}

#[test]
fn a_failed_refresh_says_the_devices_are_unreachable() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.refresh_script.lock().expect("lock") = RefreshScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        ui_end.updates.recv().expect("refresh failed")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::Failed(Failure::Devices))
    );
    assert_eq!(shared.refreshes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_steer_before_connect_never_reaches_the_host() {
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
            .send(Command::Steer {
                device: PHONE.to_owned(),
                steer: Steer::Pause,
            })
            .expect("steer");
        ui_end.updates.recv().expect("steer failed")
    });

    assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
    assert!(shared.steers.lock().expect("lock").is_empty());
}

#[test]
fn a_steer_that_lands_sends_nothing() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    // The first steer lands, the second is refused: the refusal is the
    // receipt that the landed one was processed and sent nothing.
    *shared.steer_scripts.lock().expect("lock") = vec![SteerScript::Ok, SteerScript::Fail];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Steer {
                device: PHONE.to_owned(),
                steer: Steer::SkipNext,
            })
            .expect("steer");
        ui_end
            .commands
            .send(Command::Steer {
                device: PHONE.to_owned(),
                steer: Steer::Resume,
            })
            .expect("steer");
        ui_end.updates.recv().expect("the refused steer")
    });

    assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
    assert_eq!(
        *shared.steers.lock().expect("lock"),
        vec![
            (PHONE.to_owned(), CoreSteer::SkipNext),
            (PHONE.to_owned(), CoreSteer::Resume),
        ]
    );
}

#[test]
fn a_steer_that_fails_is_reported_once() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.steer_scripts.lock().expect("lock") = vec![SteerScript::Fail];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Steer {
                device: PHONE.to_owned(),
                steer: Steer::SeekTo(60_000),
            })
            .expect("steer");
        ui_end.updates.recv().expect("steer failed")
    });

    assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
    assert_eq!(
        *shared.steers.lock().expect("lock"),
        vec![(PHONE.to_owned(), CoreSteer::SeekTo(60_000))],
        "a refused steer is never retried"
    );
}

#[test]
fn a_pick_before_connect_reports_failure_without_a_call() {
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
            .send(Command::Transfer {
                to: PHONE.to_owned(),
            })
            .expect("transfer");
        ui_end.updates.recv().expect("transfer failed")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer))
    );
    assert!(shared.transfers.lock().expect("lock").is_empty());
}

#[test]
fn a_successful_pick_sends_no_update() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let elsewhere = tablet().id;
    // The first pick lands and reports nothing; the second is refused, and
    // that failure is the receipt the first was processed. The two targets
    // differ, so a failure raised by the landed pick names a target it was
    // never sent to; and the script flips only once the host has the first
    // call in hand, so it cannot answer for it.
    let (transfer_tx, transfer_rx) = channel();
    *shared.transfer_started.lock().expect("lock") = Some(transfer_tx);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end
            .commands
            .send(Command::Transfer {
                to: PHONE.to_owned(),
            })
            .expect("transfer");
        transfer_rx.recv().expect("the hand-over started");
        expect_no_update(&ui_end);
        *shared.transfer_script.lock().expect("lock") = TransferScript::Fail;
        ui_end
            .commands
            .send(Command::Transfer {
                to: elsewhere.clone(),
            })
            .expect("transfer");
        ui_end.updates.recv().expect("the refused transfer")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer))
    );
    assert_eq!(
        *shared.transfers.lock().expect("lock"),
        vec![PHONE.to_owned(), elsewhere],
        "a landed pick is not retried and reports nothing"
    );
}

#[test]
fn a_failed_pick_sends_transfer_failed() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.transfer_script.lock().expect("lock") = TransferScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end
            .commands
            .send(Command::Transfer {
                to: PHONE.to_owned(),
            })
            .expect("transfer");
        ui_end.updates.recv().expect("transfer failed")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer))
    );
    assert_eq!(
        *shared.transfers.lock().expect("lock"),
        vec![PHONE.to_owned()],
        "a refused pick is never retried"
    );
}

#[test]
fn a_pick_with_nothing_to_hand_over_says_what_is_missing() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.transfer_script.lock().expect("lock") = TransferScript::NoContext;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end
            .commands
            .send(Command::Transfer {
                to: PHONE.to_owned(),
            })
            .expect("transfer");
        ui_end.updates.recv().expect("transfer failed")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::TransferFailed(Failure::NoContext)),
        "the copy names the real problem: no list is playing"
    );
}

#[test]
fn a_stop_during_a_pending_pick_leaves_the_loop() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.transfer_script.lock().expect("lock") = TransferScript::Pending;

    let (transfer_tx, transfer_rx) = channel();
    *shared.transfer_started.lock().expect("lock") = Some(transfer_tx);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        ui_end
            .commands
            .send(Command::Transfer {
                to: PHONE.to_owned(),
            })
            .expect("transfer");
        transfer_rx.recv().expect("the hand-over started");
        expect_no_update(&ui_end);
    });

    assert_eq!(
        shared.exit_calls.lock().expect("lock").as_slice(),
        ["withdraw", "shutdown"],
        "the stop leaves the pending hand-over and still withdraws"
    );
    assert_eq!(
        *shared.transfers.lock().expect("lock"),
        vec![PHONE.to_owned()],
        "nothing is handed over twice"
    );
}

#[test]
fn a_label_answer_names_the_remote_track() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let named = track("spotify:track:remote");
    *shared.release_script.lock().expect("lock") = ReleaseScript::Named(vec![named.clone()]);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LabelRemote {
                track: "spotify:track:remote".to_owned(),
            })
            .expect("label");
        ui_end.updates.recv().expect("labelled")
    });

    assert_eq!(update, Update::Devices(DeviceUpdate::Labelled(named)));
    assert_eq!(shared.releases.load(Ordering::SeqCst), 1);
}

#[test]
fn a_missing_label_sends_nothing() {
    for script in [ReleaseScript::Ready, ReleaseScript::Fail] {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.release_script.lock().expect("lock") = script;
        *shared.steer_scripts.lock().expect("lock") = vec![SteerScript::Fail];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LabelRemote {
                    track: "spotify:track:remote".to_owned(),
                })
                .expect("label");
            // A steer that fails answers after the label, so its own update
            // is the receipt that the label sent nothing.
            ui_end
                .commands
                .send(Command::Steer {
                    device: PHONE.to_owned(),
                    steer: Steer::Pause,
                })
                .expect("steer");
            ui_end.updates.recv().expect("steer failed")
        });

        assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
    }
}

#[test]
fn a_window_that_never_watches_hears_nothing_about_devices() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let updates = expect_updates(&ui_end, 2);
        assert!(
            !updates
                .iter()
                .any(|update| matches!(update, Update::Devices(_))),
            "nothing asks for the devices, so nothing reports them"
        );
        expect_no_update(&ui_end);
    });
}

#[test]
fn a_drain_that_ends_while_the_window_lives_says_the_devices_are_lost() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        // The pump is gone while the window is not.
        shared.device_tx.lock().expect("lock").take();
        ui_end
            .updates
            .recv_timeout(Duration::from_secs(2))
            .expect("the drain must report its own end")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff)),
        "a dead pump is terminal, not a silence to retry"
    );
}

#[test]
fn a_refresh_after_the_drain_dies_stays_off() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        shared.device_tx.lock().expect("lock").take();
        assert_eq!(
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("the drain must report its own end"),
            Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff))
        );

        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        ui_end
            .updates
            .recv_timeout(Duration::from_secs(2))
            .expect("a dead watch cannot announce again")
    });

    assert_eq!(
        update,
        Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff))
    );
    assert_eq!(
        shared.refreshes.load(Ordering::SeqCst),
        0,
        "the dead watch is not asked for a roster"
    );
}

#[test]
fn a_stop_after_watching_withdraws_before_the_player_goes() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let runtime = runtime();
    let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let watched = Arc::clone(&shared);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &watched);
        // Returned to the caller so it lives through teardown: the stop is
        // the only exit this test exercises.
        ui_end
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(2), worker.finish())
            .await
            .expect("finish resolves");
        assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(
            &*shared.exit_calls.lock().expect("lock"),
            &["withdraw", "shutdown"],
            "the account hears about the withdraw before the player stops"
        );
    };

    let _ui_end = run_ordered(runtime, window, teardown);
}

#[test]
fn a_closed_window_after_watching_withdraws_before_the_player_goes() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    let runtime = runtime();
    let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let watched = Arc::clone(&shared);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &watched);
        // Dropping the window's end ends the command loop by itself; there
        // is no stop signal to lean on.
        drop(ui_end);
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(2), worker.finish())
            .await
            .expect("finish resolves");
        assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(
            &*shared.exit_calls.lock().expect("lock"),
            &["withdraw", "shutdown"]
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_during_a_pending_watch_still_withdraws() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Pending];

    let runtime = runtime();
    let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let watched = Arc::clone(&shared);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &watched);
        drop(ui_end);
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(2), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the watch");
        assert_eq!(
            shared.withdrawals.load(Ordering::SeqCst),
            1,
            "a watch that was asked for is still withdrawn"
        );
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(
            &*shared.exit_calls.lock().expect("lock"),
            &["withdraw", "shutdown"]
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_stop_during_a_pending_refresh_still_withdraws() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.refresh_script.lock().expect("lock") = RefreshScript::Pending;

    let runtime = runtime();
    let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
    slot.install(Arc::new(RecordingWaker::default()));

    let (refresh_tx, refresh_rx) = channel();
    *shared.refresh_started.lock().expect("lock") = Some(refresh_tx);
    let watched = Arc::clone(&shared);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &watched);
        ui_end
            .commands
            .send(Command::RefreshDevices)
            .expect("refresh");
        refresh_rx.recv().expect("refresh started");
        drop(ui_end);
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(2), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the refresh");
        assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(
            &*shared.exit_calls.lock().expect("lock"),
            &["withdraw", "shutdown"]
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_withdraw_that_never_answers_is_released() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.withdraw_script.lock().expect("lock") = WithdrawScript::Pending;

    let runtime = runtime();
    let bound = Duration::from_millis(50);
    let (ui_end, slot, worker) =
        start_with(runtime.handle(), host, store, source, bound, Pacing::LIVE);
    slot.install(Arc::new(RecordingWaker::default()));

    let watched = Arc::clone(&shared);
    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &watched);
    };
    let teardown = move || async move {
        let started = std::time::Instant::now();
        tokio::time::timeout(Duration::from_secs(2), worker.finish())
            .await
            .expect("a hung withdraw cannot hold the exit");
        let waited = started.elapsed();
        assert!(waited >= bound, "the outer bound is the wait");
        assert!(waited < Duration::from_secs(1), "the exit is bounded");
        assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_window_that_never_watched_withdraws_nothing() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
    });

    assert_eq!(
        shared.withdrawals.load(Ordering::SeqCst),
        0,
        "a window that never announced has nothing to withdraw"
    );
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

// ---- the account's requests ----

#[test]
fn an_inbound_command_is_refused_and_nothing_else_happens() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);

        let id = RequestId(7);
        inbound_sender(&shared)
            .send(Inbound {
                id,
                kind: Kind::Transfer,
                instruction: Instruction::Other(Kind::Transfer),
            })
            .expect("inbound");
        until("the request to be refused", || {
            !refusals(&shared).is_empty()
        });
        assert_eq!(refusals(&shared), vec![id]);
        // A refusal is the whole answer: the window hears nothing.
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Refuse(RequestId(7))],
        "the request is refused once and no other call is made"
    );
}

#[test]
fn every_kind_is_refused_exactly_once() {
    const KINDS: [Kind; 15] = [
        Kind::Transfer,
        Kind::Play,
        Kind::Pause,
        Kind::Resume,
        Kind::SeekTo,
        Kind::SkipNext,
        Kind::SkipPrev,
        Kind::SetShufflingContext,
        Kind::SetRepeatingContext,
        Kind::SetRepeatingTrack,
        Kind::SetOptions,
        Kind::SetQueue,
        Kind::AddToQueue,
        Kind::UpdateContext,
        Kind::Unknown,
    ];

    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);

        let tx = inbound_sender(&shared);
        for (id, kind) in (0u64..).zip(KINDS) {
            tx.send(Inbound {
                id: RequestId(id),
                kind,
                instruction: Instruction::Other(kind),
            })
            .expect("inbound");
        }
        until("every request to be refused", || {
            refusals(&shared).len() == KINDS.len()
        });
    });

    let refused = refusals(&shared);
    let expected: Vec<RequestId> = (0u64..).map(RequestId).take(KINDS.len()).collect();
    assert_eq!(refused, expected, "each request is answered, in order");
    assert_eq!(
        calls(&shared),
        refused.into_iter().map(Call::Refuse).collect::<Vec<_>>(),
        "every kind is refused exactly once"
    );
}

#[test]
fn a_report_reaches_the_host_only_when_watching() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);

        // This command is in front of the watch in the same channel, so it
        // is handled before the watch starts.
        ui_end
            .commands
            .send(Command::Report(Some(report("spotify:track:before"))))
            .expect("report");
        watch_now(&ui_end, &shared);
        assert!(
            !calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Report(_))),
            "a report before the watch has no wire to reach"
        );

        ui_end
            .commands
            .send(Command::Report(Some(report("spotify:track:after"))))
            .expect("report");
        until("the report to reach the host", || {
            calls(&shared).iter().any(|call| {
                matches!(
                    call,
                    Call::Report(report)
                        if report.as_ref() == &Some(self::report("spotify:track:after"))
                )
            })
        });

        ui_end.commands.send(Command::Report(None)).expect("report");
        until("the clearing to reach the host", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Report(report) if report.is_none()))
        });
    });
}

#[test]
fn playing_a_track_makes_the_device_playable() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: TRACK.to_owned(),
            })
            .expect("play");
        until("the device to become playable", || {
            calls(&shared).contains(&Call::SetPlayable(true))
        });
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Play(TRACK.to_owned()), Call::SetPlayable(true)]
    );
    assert_eq!(shared.players.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_play_for_no_audio_device_makes_it_unplayable() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.play_script.lock().expect("lock") = PlayScript::NoAudioDevice;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: TRACK.to_owned(),
            })
            .expect("play");
        assert_eq!(
            ui_end.updates.recv().expect("play failed"),
            Update::Transport(TransportUpdate::Failed(Failure::NoAudioDevice))
        );
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Play(TRACK.to_owned()), Call::SetPlayable(false)],
        "a machine with no output can never be picked"
    );
}

#[test]
fn a_not_playable_track_leaves_the_device_playable() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Play {
                uri: TRACK.to_owned(),
            })
            .expect("play");
        until("the device to become playable", || {
            calls(&shared).contains(&Call::SetPlayable(true))
        });

        // One bad track is not a device that cannot play, so the stage
        // must stay where the good one left it.
        *shared.play_script.lock().expect("lock") = PlayScript::NotPlayable;
        ui_end
            .commands
            .send(Command::Play {
                uri: TRACK.to_owned(),
            })
            .expect("play");
        assert_eq!(
            ui_end.updates.recv().expect("play failed"),
            Update::Transport(TransportUpdate::Failed(Failure::Other))
        );
    });

    assert_eq!(
        calls(&shared),
        vec![
            Call::Play(TRACK.to_owned()),
            Call::SetPlayable(true),
            Call::Play(TRACK.to_owned()),
        ]
    );
}

#[test]
fn a_closed_inbound_stream_is_forgotten_and_its_arm_goes_quiet() {
    runtime().block_on(async {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Inbound>();
        let mut slot = Some(rx);
        drop(tx);

        assert_eq!(
            next_inbound(&mut slot).await,
            None,
            "a closed stream yields nothing to answer"
        );
        assert!(
            slot.is_none(),
            "the dead receiver is dropped instead of polled again"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(20), next_inbound(&mut slot))
                .await
                .is_err(),
            "an arm with no stream stays pending rather than resolving in a spin"
        );
    });
}

#[test]
fn commands_are_still_served_after_the_inbound_stream_closes() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);

        // The account stops sending requests; the loop must carry on with
        // the window's commands.
        shared.inbound_tx.lock().expect("lock").take();
        expect_no_update(&ui_end);

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

    assert!(
        calls(&shared).is_empty(),
        "a stream that ended refuses nothing"
    );
    assert_eq!(shared.watches.load(Ordering::SeqCst), 1);
}
