use super::super::*; // the module under test
use super::*; // the shared test harness

// ---- serving a landing ----

/// The lines the bridge logged about its own decisions, so a test can tell
/// two arms that answer the same request apart.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Recorder;

impl log::Log for Recorder {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() != LANDING_LOG_TARGET || !self.enabled(record.metadata()) {
            return;
        }
        LOGGED.lock().expect("lock").push(record.args().to_string());
    }

    fn flush(&self) {}
}

/// Begins recording the bridge's log lines, once for the whole test binary.
fn record_logs() {
    static RECORDER: Recorder = Recorder;
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        if log::set_logger(&RECORDER).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
    });
}

/// Whether the bridge logged exactly `line` about one of its decisions.
fn logged(line: &str) -> bool {
    LOGGED.lock().expect("lock").iter().any(|seen| seen == line)
}

#[test]
fn a_phone_side_play_of_a_popular_list_lands() {
    use librespot::core::dealer::protocol::SkipTo;

    // An artist's popular tracks arrive as a `spotify:list:` context, which
    // resolves to its rows exactly like a playlist's does.
    const POPULAR: &str = "spotify:list:popular-release-segments-main-roles:artist_x";

    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let rows = vec![SECOND.to_owned(), TRACK.to_owned()];
    *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(rows.clone());
    *shared.accept_script.lock().expect("lock") = AcceptScript::Landed(Box::new(Landing {
        rows: rows.clone(),
        index: 1,
        ..default_landing()
    }));
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            51,
            Kind::Play,
            play_instruction(
                POPULAR,
                &[],
                Some(SkipTo {
                    track_uri: Some(TRACK.to_owned()),
                    ..Default::default()
                }),
            ),
        );
        until("the play to land", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    let log = calls(&shared);
    assert!(
        !log.iter().any(|call| matches!(call, Call::Refuse(_))),
        "a popular list is not refused: {log:?}"
    );
    assert!(
        log.iter().any(|call| matches!(
            call,
            Call::Load { uri, .. } if uri == TRACK
        )),
        "the row the phone named is the row that loads: {log:?}"
    );
}

#[test]
fn a_window_that_never_played_announces_that_it_can_play() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        let (tx, rx) = channel();
        *shared.watch_started.lock().expect("lock") = Some(tx);
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        rx.recv().expect("watch started");
    });

    // The player is readied before the announcement, so the device is
    // pickable from launch rather than only after a local play.
    let log = calls(&shared);
    assert_eq!(
        log.iter()
            .filter(|call| matches!(call, Call::Prepare))
            .count(),
        1,
        "the watch readies the player once: {log:?}"
    );
    assert!(
        log.iter()
            .any(|call| matches!(call, Call::SetPlayable(true))),
        "the device is announced as able to play: {log:?}"
    );
}

#[test]
fn a_machine_with_no_audio_device_announces_a_controller() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.prepare_script.lock().expect("lock") = PrepareScript::NoAudioDevice;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        let (tx, rx) = channel();
        *shared.watch_started.lock().expect("lock") = Some(tx);
        ui_end.commands.send(Command::WatchDevices).expect("watch");
        rx.recv().expect("watch started");
    });

    // A device that cannot play says so: the phone lists it greyed out.
    let log = calls(&shared);
    assert!(
        !log.iter()
            .any(|call| matches!(call, Call::SetPlayable(true))),
        "nothing claims this machine can play: {log:?}"
    );
    assert!(
        log.iter()
            .any(|call| matches!(call, Call::SetPlayable(false))),
        "the stage is held down honestly: {log:?}"
    );
}

#[test]
fn a_transfer_runs_prepare_resolve_accept_load_label_in_that_order() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let id = RequestId(41);
    *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(vec![TRACK.to_owned()]);
    *shared.accept_script.lock().expect("lock") = AcceptScript::Landed(Box::new(Landing {
        position_ms: 7_000,
        ..default_landing()
    }));
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            41,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
        );
        until("the label to run", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Label(_)))
        });
    });

    assert_eq!(
        calls(&shared),
        vec![
            Call::Prepare,
            Call::SetPlayable(true),
            Call::Resolve("spotify:playlist:p".to_owned()),
            Call::Accept(id),
            Call::Answer(id, true),
            Call::Load {
                uri: TRACK.to_owned(),
                position_ms: 7_000,
                play: true,
            },
            Call::Label(vec![TRACK.to_owned()]),
        ],
        "the hand-over runs its steps in order, and the label waits for the load"
    );
    assert_eq!(
        shared.event_takes.load(Ordering::SeqCst),
        1,
        "the player's events are handed to one drain"
    );
}

#[test]
fn a_play_command_lands_like_a_transfer() {
    use librespot::core::dealer::protocol::SkipTo;

    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let id = RequestId(44);
    let rows = vec![TRACK.to_owned(), SECOND.to_owned()];
    *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(rows.clone());
    // The row the position named is the row the landing anchors on.
    *shared.accept_script.lock().expect("lock") = AcceptScript::Landed(Box::new(Landing {
        rows: rows.clone(),
        index: 1,
        position_ms: 4_000,
        ..default_landing()
    }));
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            44,
            Kind::Play,
            play_instruction(
                PLAYLIST,
                &[],
                Some(SkipTo {
                    track_index: Some(1),
                    ..Default::default()
                }),
            ),
        );
        until("the label to run", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Label(_)))
        });
    });

    assert_eq!(
        calls(&shared),
        vec![
            Call::Prepare,
            Call::SetPlayable(true),
            Call::Resolve(PLAYLIST.to_owned()),
            Call::Accept(id),
            Call::Answer(id, true),
            Call::Load {
                uri: SECOND.to_owned(),
                position_ms: 4_000,
                play: true,
            },
            Call::Label(vec![SECOND.to_owned()]),
        ],
        "a play runs the hand-over's own steps and loads the row it anchored on"
    );
}

#[test]
fn a_play_with_nothing_to_anchor_on_is_refused() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let id = RequestId(45);
    // A play that names its row neither by uri nor by pages has only the
    // read to say what to load, and this read brought nothing.
    *shared.resolve_script.lock().expect("lock") = ResolveScript::Fail;
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            45,
            Kind::Play,
            play_instruction(PLAYLIST, &[], None),
        );
        until("the play to be refused", || answers(&shared, id) == 1);
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![
            Call::Prepare,
            Call::SetPlayable(true),
            Call::Resolve(PLAYLIST.to_owned()),
            Call::Refuse(id),
        ],
        "nothing is put and nothing is loaded when there is no row to play"
    );
}

#[test]
fn a_play_of_a_local_or_unsupported_row_is_refused_before_the_put() {
    use librespot::core::dealer::protocol::SkipTo;

    // A local file is a row the phone can hold and this window cannot fetch,
    // whether it names that row by uri or lands on it by position.
    let cases = [
        (
            47,
            vec![TRACK.to_owned(), SECOND.to_owned()],
            SkipTo {
                track_uri: Some(LOCAL.to_owned()),
                ..Default::default()
            },
        ),
        (
            48,
            vec![TRACK.to_owned(), LOCAL.to_owned()],
            SkipTo {
                track_index: Some(1),
                ..Default::default()
            },
        ),
    ];

    for (number, rows, skip) in cases {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let id = RequestId(number);
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(rows);
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                number,
                Kind::Play,
                play_instruction(PLAYLIST, &[], Some(skip)),
            );
            until("the play to be refused", || answers(&shared, id) == 1);
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Prepare,
                Call::SetPlayable(true),
                Call::Resolve(PLAYLIST.to_owned()),
                Call::Refuse(id),
            ],
            "a row that cannot be fetched must not take the sound and fail"
        );
    }
}

#[test]
fn update_context_is_answered_without_changing_anything() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let id = RequestId(46);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(&shared, 46, Kind::UpdateContext, Instruction::Ignored);
        until("the context to be answered", || answers(&shared, id) == 1);
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Answer(id, true)],
        "a context this window does not use is answered, not refused"
    );
}

#[test]
fn landed_precedes_every_playback_update() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    // The load holds the loop where it stands, so the rows can only be
    // waiting in the window if they were sent before it was asked for.
    *shared.load_script.lock().expect("lock") = LoadScript::Held;
    let (load_started, started) = std::sync::mpsc::channel();
    let (release, in_flight) = std::sync::mpsc::channel();
    *shared.load_gate.lock().expect("lock") = Some((load_started, in_flight));
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            42,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        started
            .recv_timeout(Duration::from_secs(2))
            .expect("the load is asked for");
        let landed = ui_end
            .updates
            .recv_timeout(Duration::from_secs(1))
            .expect("the rows are already in the window");
        assert!(
            matches!(landed, Update::Remote(RemoteUpdate::Landed(_))),
            "the rows land before the load that follows them: {landed:?}"
        );
        release.send(()).expect("release the load");

        shared
            .event_tx
            .lock()
            .expect("lock")
            .clone()
            .expect("the drain is open")
            .send(PlaybackEvent::Loading)
            .expect("playback event");

        assert_eq!(
            expect_updates(&ui_end, 1),
            vec![Update::Transport(TransportUpdate::Loading)],
            "the player's first event follows the rows"
        );
    });
}

#[test]
fn a_refused_instruction_never_reaches_the_wire() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            43,
            Kind::Transfer,
            Instruction::Refused(Refusal::NoData),
        );
        until("the refusal", || answers(&shared, RequestId(43)) == 1);
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Refuse(RequestId(43))],
        "a refusal prepares nothing, reads nothing and puts nothing"
    );
    assert!(
        !calls(&shared)
            .iter()
            .any(|call| matches!(call, Call::Answer(answered, _) if *answered == RequestId(43))),
        "the request is answered as a refusal, never taken"
    );
}

#[test]
fn a_failed_prepare_refuses_before_resolve_or_accept() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.prepare_script.lock().expect("lock") = PrepareScript::NoAudioDevice;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            44,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
        );
        until("the refusal", || answers(&shared, RequestId(44)) == 1);
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![
            Call::Prepare,
            Call::SetPlayable(false),
            Call::Refuse(RequestId(44)),
        ],
        "a player that cannot start refuses before any read or put"
    );
}

#[test]
fn a_failed_resolve_falls_back_to_the_one_track_and_still_accepts() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.resolve_script.lock().expect("lock") = ResolveScript::Fail;
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            45,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
        );
        until("the landing to load", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    assert_eq!(
        *shared.last_resolution.lock().expect("lock"),
        Some(Resolution::Failed),
        "a failed read still hands the accept a resolution"
    );
    assert!(
        calls(&shared).iter().any(|call| matches!(
            call,
            Call::Load { uri, .. } if uri == TRACK
        )),
        "the one track is what lands"
    );
    assert_eq!(answers(&shared, RequestId(45)), 1);
}

#[test]
fn a_failed_accept_does_not_load_or_land_and_answers_once() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.accept_script.lock().expect("lock") = AcceptScript::Fail;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            46,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the answer", || answers(&shared, RequestId(46)) == 1);
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![
            Call::Prepare,
            Call::SetPlayable(true),
            Call::Accept(RequestId(46)),
            Call::Answer(RequestId(46), false),
        ],
        "a failed accept is the whole answer: no label, no load"
    );
    assert_eq!(answers(&shared, RequestId(46)), 1);
}

#[test]
fn a_load_failure_after_accept_releases_the_sound() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.load_script.lock().expect("lock") = LoadScript::Fail;
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            47,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the sound to be given back", || {
            calls(&shared).contains(&Call::Release)
        });
    });

    let logged = calls(&shared);
    let load = logged
        .iter()
        .position(|call| matches!(call, Call::Load { .. }))
        .expect("the load was attempted");
    let release = logged
        .iter()
        .position(|call| *call == Call::Release)
        .expect("the sound was given back");
    assert!(
        load < release,
        "a load that failed gives the sound back: {logged:?}"
    );
}

#[test]
fn a_load_failure_pauses_the_window_it_just_adopted() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.load_script.lock().expect("lock") = LoadScript::Fail;
    hold_with(&shared, &[true]);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            58,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the sound to be given back", || {
            calls(&shared).contains(&Call::Release)
        });
        expect_updates_within(&ui_end, 3)
    });

    assert!(
        matches!(updates[0], Update::Remote(RemoteUpdate::Landed(_))),
        "the rows land before the load that failed: {updates:?}"
    );
    assert_eq!(
        updates[1],
        Update::Transport(TransportUpdate::Failed(Failure::Other)),
        "the track that could not be loaded is reported"
    );
    assert_eq!(
        updates[2],
        Update::Remote(RemoteUpdate::Lost { epoch: 1 }),
        "the transport the landing left waiting is paused, not left in Loading"
    );
}

#[test]
fn a_failed_release_after_a_load_failure_is_not_silent() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.load_script.lock().expect("lock") = LoadScript::Fail;
    shared.sound_release_fails.store(true, Ordering::SeqCst);
    hold_with(&shared, &[true]);

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            59,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the give-back to be refused", || {
            calls(&shared).contains(&Call::Release)
        });
        expect_updates_within(&ui_end, 4)
    });

    assert_eq!(
        &updates[1..],
        [
            Update::Transport(TransportUpdate::Failed(Failure::Other)),
            Update::Transport(TransportUpdate::Failed(Failure::Other)),
            Update::Remote(RemoteUpdate::Lost { epoch: 1 }),
        ],
        "a give-back that failed is reported, not dropped"
    );
}

#[test]
fn a_loss_before_landed_sends_no_landed_and_no_load() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[false]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            48,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the answer", || answers(&shared, RequestId(48)) == 1);
        expect_no_update(&ui_end);
    });

    let logged = calls(&shared);
    assert!(
        !logged
            .iter()
            .any(|call| matches!(call, Call::Load { .. } | Call::Label(_))),
        "a landing that lost the sound is neither named nor loaded: {logged:?}"
    );
}

#[test]
fn a_loss_between_landed_and_load_loads_paused() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[true, false]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            49,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        let landed = ui_end.updates.recv().expect("the landing");
        assert!(
            matches!(landed, Update::Remote(RemoteUpdate::Landed(_))),
            "the landing is announced before the load"
        );
        until("the load", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    assert!(
        calls(&shared)
            .iter()
            .any(|call| matches!(call, Call::Load { play: false, .. })),
        "a loss between the announcement and the load pauses it"
    );
}

#[test]
fn a_transfer_that_creates_the_player_hands_its_events_to_a_drain() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            50,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the load", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Load { .. }))
        });
    });

    assert_eq!(
        shared.event_takes.load(Ordering::SeqCst),
        1,
        "a player created by the landing still gets a drain"
    );
    assert_eq!(shared.players.load(Ordering::SeqCst), 1);
}

#[test]
fn the_current_track_is_named_when_the_label_answers_and_a_placeholder_when_it_does_not() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let named = TrackEntry {
        title: "Named".to_owned(),
        ..track(TRACK)
    };
    *shared.release_script.lock().expect("lock") = ReleaseScript::Named(vec![named.clone()]);
    hold_with(&shared, &[true, true, true, true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            51,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the first load", || {
            calls(&shared)
                .iter()
                .filter(|call| matches!(call, Call::Load { .. }))
                .count()
                == 1
        });
        let updates = expect_updates(&ui_end, 2);
        match &updates[0] {
            Update::Remote(RemoteUpdate::Landed(landing)) => assert_eq!(
                landing.report().track.map(|track| track.title),
                Some(TRACK.to_owned()),
                "the landing carries a placeholder until the label answers"
            ),
            other => panic!("expected a landing, got {other:?}"),
        }
        assert_eq!(
            updates[1],
            Update::Remote(RemoteUpdate::Labelled(vec![named])),
            "the answer names the current track"
        );

        *shared.release_script.lock().expect("lock") = ReleaseScript::Ready;
        inbound(
            &shared,
            52,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the second load", || {
            calls(&shared)
                .iter()
                .filter(|call| matches!(call, Call::Load { .. }))
                .count()
                == 2
        });
        let updates = expect_updates(&ui_end, 1);
        match &updates[0] {
            Update::Remote(RemoteUpdate::Landed(landing)) => assert_eq!(
                landing.report().track.map(|track| track.title),
                Some(TRACK.to_owned()),
                "a miss leaves the placeholder in place"
            ),
            other => panic!("expected a landing, got {other:?}"),
        }
        expect_no_update(&ui_end);
    });
}

#[test]
fn a_stop_during_a_pending_accept_leaves_the_loop_and_still_withdraws() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.accept_script.lock().expect("lock") = AcceptScript::Pending;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            53,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        until("the accept to start", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Accept(_)))
        });
    });

    assert_eq!(
        shared.exit_calls.lock().expect("lock").as_slice(),
        ["withdraw", "shutdown"],
        "the stop leaves the pending accept and still withdraws"
    );
    assert!(
        !calls(&shared)
            .iter()
            .any(|call| matches!(call, Call::Load { .. })),
        "nothing is loaded after the stop"
    );
}

#[test]
fn a_stop_during_a_pending_resolve_leaves_the_loop_and_still_withdraws() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.resolve_script.lock().expect("lock") = ResolveScript::Pending;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            58,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
        );
        until("the resolve to start", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Resolve(_)))
        });
    });

    assert_eq!(
        shared.exit_calls.lock().expect("lock").as_slice(),
        ["withdraw", "shutdown"],
        "the stop leaves the pending read and still withdraws"
    );
    assert!(
        !calls(&shared)
            .iter()
            .any(|call| matches!(call, Call::Accept(_))),
        "nothing is accepted after the stop"
    );
}

#[test]
fn every_inbound_is_answered_exactly_once() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        inbound(
            &shared,
            54,
            Kind::Transfer,
            Instruction::Refused(Refusal::NoData),
        );
        inbound(&shared, 55, Kind::Pause, Instruction::Other(Kind::Pause));
        inbound(
            &shared,
            56,
            Kind::Unknown,
            Instruction::Other(Kind::Unknown),
        );
        inbound(
            &shared,
            57,
            Kind::Transfer,
            transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
        );
        // An order is answered by the window, so this test answers it the
        // way a frame does: once, after the effect.
        hold_request(&shared, RequestId(58));
        inbound(&shared, 58, Kind::Resume, Instruction::Order(Order::Resume));
        let _ = order_update(&ui_end, RequestId(58));
        ui_end
            .commands
            .send(Command::Answer {
                id: RequestId(58),
                ok: true,
            })
            .expect("answer");
        until("every inbound to be answered", || {
            (54..=57).all(|id| answers(&shared, RequestId(id)) == 1)
                && answers(&shared, RequestId(58)) == 1
        });
    });

    for id in 54..=58 {
        assert_eq!(
            answers(&shared, RequestId(id)),
            1,
            "request {id} is answered exactly once"
        );
    }
}

#[test]
fn an_order_while_not_holding_is_refused() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[false]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        hold_request(&shared, RequestId(60));
        inbound(&shared, 60, Kind::Pause, Instruction::Order(Order::Pause));
        until("the order to be refused", || !refusals(&shared).is_empty());
        expect_no_update(&ui_end);
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Refuse(RequestId(60))],
        "a window with no sound to move refuses, and never forwards"
    );
}

#[test]
fn an_order_for_an_expired_entry_is_not_forwarded() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        record_logs();
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        // The budget answered this one already, so the phone has been told
        // it failed and the window is never asked about it.
        inbound(&shared, 61, Kind::Pause, Instruction::Order(Order::Pause));
        until("the order to be refused", || !refusals(&shared).is_empty());
        expect_no_update(&ui_end);
        until("the order path to say why", || {
            logged("order refused, entry gone: pause")
        });
    });

    assert_eq!(calls(&shared), vec![Call::Refuse(RequestId(61))]);
    assert!(
        !calls(&shared).contains(&Call::Prepare),
        "the order path hands nothing to a player it never prepares"
    );
}

#[test]
fn an_order_with_no_window_is_refused_rather_than_left_to_the_budget() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        hold_request(&shared, RequestId(63));
        // The window is gone: the receiver its updates would land on is
        // dropped, so the order reaches nobody.
        let UiEnd { updates, commands } = ui_end;
        drop(updates);
        inbound(&shared, 63, Kind::Pause, Instruction::Order(Order::Pause));
        until("the order to be refused", || !refusals(&shared).is_empty());
        drop(commands);
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Refuse(RequestId(63))],
        "the phone hears at once instead of waiting out its budget"
    );
}

#[test]
fn an_order_is_forwarded_and_answered_only_after_the_ui_replies() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    hold_with(&shared, &[true]);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        watch_now(&ui_end, &shared);
        let deadline = hold_request(&shared, RequestId(62));
        inbound(&shared, 62, Kind::Pause, Instruction::Order(Order::Pause));

        let (order, expires) = order_update(&ui_end, RequestId(62));
        assert_eq!(order, Order::Pause);
        assert_eq!(
            deadline.checked_sub(ORDER_MARGIN),
            Some(expires),
            "the window drops the order the margin before the entry's budget"
        );
        assert_eq!(
            answers(&shared, RequestId(62)),
            0,
            "the bridge does not answer for the window"
        );

        ui_end
            .commands
            .send(Command::Answer {
                id: RequestId(62),
                ok: true,
            })
            .expect("answer");
        until("the window's answer", || {
            answers(&shared, RequestId(62)) == 1
        });
    });

    assert_eq!(
        calls(&shared),
        vec![Call::Answer(RequestId(62), true)],
        "the order is neither refused nor answered before the window replies"
    );
}

#[test]
fn lost_becomes_an_update_with_its_epoch_and_the_drain_keeps_running() {
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
        tx.send(ClusterEvent::Lost { epoch: 9 }).expect("loss");
        let lost = ui_end.updates.recv().expect("loss update");
        assert_eq!(lost, Update::Remote(RemoteUpdate::Lost { epoch: 9 }));

        tx.send(ClusterEvent::View(Box::new(phone_view())))
            .expect("view");
        expect_updates(&ui_end, 2)
    });

    assert_eq!(updates, vec![device_roster(), device_playing()]);
}

#[test]
fn a_label_rows_command_replies_with_labelled_rows() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let named = TrackEntry {
        title: "Named".to_owned(),
        ..track(TRACK)
    };
    *shared.release_script.lock().expect("lock") = ReleaseScript::Named(vec![named.clone()]);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LabelRows {
                uris: vec![TRACK.to_owned()],
            })
            .expect("labels");
        ui_end.updates.recv().expect("labelled")
    });

    assert_eq!(update, Update::Remote(RemoteUpdate::Labelled(vec![named])));
    assert_eq!(calls(&shared), vec![Call::Label(vec![TRACK.to_owned()])]);
}
