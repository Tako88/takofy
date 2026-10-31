//! How commands are scheduled: which reads overlap, what has to wait, and
//! where the loop stops admitting.

use super::super::reads::{Class, IDLE_GRACE, Immediate, MAX_CONCURRENT_READS, Read, classify};
use super::super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;
use std::time::Duration;
use takofy_core::connect::WITHDRAW_TIMEOUT;
use takofy_core::instruction::{Instruction, Kind};
use takofy_core::login::{SourceError, TokenSet};
use takofy_core::receive::Inbound;
use takofy_model::home::{HomeAsk, SeedKind, StationSeed};
use takofy_model::{CoverUrl, Move, Removal, RequestId};
use tokio::sync::Semaphore;

/// Arms the hold with no permits, so every read a test scripts waits until
/// the test hands it one.
fn hold_reads(shared: &FakeState) -> Arc<Semaphore> {
    let hold = Arc::new(Semaphore::new(0));
    *shared.read_hold.lock().expect("lock") = Some(Arc::clone(&hold));
    hold
}

fn in_flight(shared: &FakeState) -> usize {
    shared.reads_in_flight.load(Ordering::SeqCst)
}

/// Every command, with the class it must keep.
#[test]
fn classes_are_pinned() {
    let base = revision(1);
    let seed = StationSeed {
        kind: SeedKind::Artist,
        uri: ARTIST.to_owned(),
    };
    let cover = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let table = vec![
        (
            Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LoadAlbum {
                album: "spotify:album:a".to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LoadArtist {
                artist: ARTIST.to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LoadRelated {
                artist: ARTIST.to_owned(),
                uris: vec![TRACK.to_owned()],
            },
            Class::Read,
        ),
        (
            Command::LoadLyrics {
                track: TRACK.to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LoadShow {
                show: "spotify:show:a".to_owned(),
            },
            Class::Read,
        ),
        (Command::LoadFollowedShows, Class::Read),
        (Command::LoadFollowedArtists, Class::Read),
        (Command::LoadSavedAlbums, Class::Read),
        (Command::LoadLikedTracks, Class::Read),
        (Command::StartRadio { seed: seed.clone() }, Class::Read),
        (Command::LoadHome(HomeAsk::Station { seed }), Class::Read),
        (
            Command::Search {
                query: "slow tide".to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LoadPortraits {
                query: "slow tide".to_owned(),
                artists: vec![ARTIST.to_owned()],
            },
            Class::Read,
        ),
        (
            Command::LoadReleaseTracks {
                release: "spotify:album:r".to_owned(),
                tracks: vec![TRACK.to_owned()],
            },
            Class::Read,
        ),
        (
            Command::LoadSaved {
                uri: "spotify:album:a".to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LabelRemote {
                track: TRACK.to_owned(),
            },
            Class::Read,
        ),
        (
            Command::LabelRows {
                uris: vec![TRACK.to_owned()],
            },
            Class::Read,
        ),
        (
            Command::LabelSaved {
                tracks: vec![TRACK.to_owned()],
            },
            Class::Read,
        ),
        (Command::Fetch { url: cover }, Class::Instant),
        (Command::Report(Some(report(TRACK))), Class::Instant),
        (Command::Volume { percent: 35 }, Class::Instant),
        (
            Command::Answer {
                id: RequestId(1),
                ok: true,
            },
            Class::Instant,
        ),
        (Command::Pause, Class::Instant),
        (Command::Resume, Class::Instant),
        (Command::Stop, Class::Instant),
        (Command::Seek { position_ms: 12 }, Class::Instant),
        (
            Command::Play {
                uri: TRACK.to_owned(),
            },
            Class::Barrier,
        ),
        (
            Command::Steer {
                device: "phone".to_owned(),
                steer: Steer::Pause,
            },
            Class::Barrier,
        ),
        (
            Command::Transfer {
                to: "phone".to_owned(),
            },
            Class::Barrier,
        ),
        (
            Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            },
            Class::Barrier,
        ),
        (
            Command::SetLiked {
                tracks: vec![TRACK.to_owned()],
                liked: true,
            },
            Class::Barrier,
        ),
        (
            Command::SetSaved {
                uri: "spotify:album:a".to_owned(),
                saved: true,
            },
            Class::Barrier,
        ),
        (
            Command::MoveTrack {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 1,
                }],
            },
            Class::Barrier,
        ),
        (
            Command::RemoveTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                removals: vec![Removal { from: 0, length: 1 }],
            },
            Class::Barrier,
        ),
        (
            Command::MovePlaylist {
                revision: base.clone(),
                from: 0,
                to: 1,
            },
            Class::Barrier,
        ),
        (
            Command::RemovePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                index: 0,
            },
            Class::Barrier,
        ),
        (
            Command::LinkPlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
            },
            Class::Barrier,
        ),
        (
            Command::AddTracks {
                playlist: "spotify:playlist:a".to_owned(),
                revision: base.clone(),
                from_index: 0,
                tracks: vec![TRACK.to_owned()],
            },
            Class::Barrier,
        ),
        (
            Command::RenamePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                name: "A".to_owned(),
            },
            Class::Barrier,
        ),
        (
            Command::CreatePlaylist {
                name: "A".to_owned(),
                revision: base,
                tracks: Vec::new(),
            },
            Class::Barrier,
        ),
        (Command::WatchDevices, Class::Barrier),
        (Command::RefreshDevices, Class::Barrier),
        (Command::Authorize, Class::Barrier),
        (Command::ReloadLibrary, Class::Barrier),
    ];
    for (command, class) in table {
        assert_eq!(classify(&command), class, "{command:?}");
        // The classes and the claims name the same commands, so no command is
        // routed to a body that will not take it.
        assert_eq!(
            Read::claim(command.clone()).is_some(),
            class == Class::Read,
            "{command:?}"
        );
        assert_eq!(
            Immediate::claim(command.clone()).is_some(),
            class == Class::Instant,
            "{command:?}"
        );
    }
}

#[test]
fn independent_reads_overlap() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let updates = run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        let hold = hold_reads(&shared);
        let reads = [
            Command::LoadFollowedShows,
            Command::LoadFollowedArtists,
            Command::LoadSavedAlbums,
            Command::LoadLikedTracks,
        ];
        let asked = reads.len();
        for command in reads {
            ui_end.commands.send(command).expect("read");
        }
        until("every read in flight at once", || {
            in_flight(&shared) == asked
        });
        hold.add_permits(asked);
        (0..4)
            .map(|_| {
                ui_end
                    .updates
                    .recv_timeout(Duration::from_secs(2))
                    .expect("answer")
            })
            .collect::<Vec<_>>()
    });

    let mut kinds: Vec<&str> = updates
        .iter()
        .map(|update| match update {
            Update::FollowedShows(_) => "shows",
            Update::FollowedArtists(_) => "artists",
            Update::SavedAlbums(_) => "albums",
            Update::LikedTracks(_) => "liked",
            other => panic!("unexpected answer: {other:?}"),
        })
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["albums", "artists", "liked", "shows"]);
}

#[test]
fn a_fetch_between_reads_does_not_serialize_them() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("signed out"),
            Update::NeedsAuthorization
        );
        let hold = hold_reads(&shared);
        for command in [
            Command::LoadFollowedShows,
            Command::Fetch { url: url.clone() },
            Command::LoadFollowedArtists,
            Command::Report(None),
            Command::Volume { percent: 35 },
            Command::LoadLikedTracks,
        ] {
            ui_end.commands.send(command).expect("command");
        }
        until("three reads in flight", || in_flight(&shared) == 3);
        // The fetch answers while all three reads are still held: neither it
        // nor the level waits for them, and they do not wait for it.
        assert_eq!(
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("cover reply"),
            Update::CoverFailed { url }
        );
        assert_eq!(
            in_flight(&shared),
            3,
            "the cover reply arrives while every read is still held"
        );
        assert_eq!(
            shared.last_volume.load(Ordering::SeqCst),
            35,
            "the level is applied at once"
        );
        hold.add_permits(3);
        let mut kinds: Vec<&str> = (0..3)
            .map(|_| {
                let update = ui_end
                    .updates
                    .recv_timeout(Duration::from_secs(2))
                    .expect("read answer");
                match &update {
                    Update::FollowedShowsFailed(_) => "shows",
                    Update::FollowedArtistsFailed(_) => "artists",
                    Update::LikedTracksFailed(_) => "liked",
                    other => panic!("unexpected answer: {other:?}"),
                }
            })
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["artists", "liked", "shows"]);
    });
}

#[test]
fn pause_and_seek_apply_while_a_read_is_held() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        let hold = hold_reads(&shared);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("read");
        until("the read in flight", || in_flight(&shared) == 1);
        ui_end.commands.send(Command::Pause).expect("pause");
        ui_end
            .commands
            .send(Command::Seek {
                position_ms: 41_000,
            })
            .expect("seek");
        until("the pause", || shared.pauses.load(Ordering::SeqCst) == 1);
        until("the seek", || shared.seeks.load(Ordering::SeqCst) == 1);
        assert_eq!(shared.last_seek.load(Ordering::SeqCst), 41_000);
        assert_eq!(in_flight(&shared), 1, "the read is still held");
        hold.add_permits(1);
        assert_eq!(
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("answer"),
            Update::FollowedShows(Vec::new())
        );
    });
}

#[test]
fn an_instant_behind_a_barrier_waits_for_it() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        // A report needs a watch to have somewhere to go.
        watch_now(&ui_end, &shared);
        let hold = hold_reads(&shared);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("read");
        until("the read in flight", || in_flight(&shared) == 1);
        ui_end
            .commands
            .send(Command::Play {
                uri: TRACK.to_owned(),
            })
            .expect("play");
        ui_end.commands.send(Command::Pause).expect("pause");
        ui_end
            .commands
            .send(Command::Report(Some(report(TRACK))))
            .expect("report");
        expect_no_update(&ui_end);
        assert_eq!(
            shared.plays.load(Ordering::SeqCst),
            0,
            "the play waits for the read in flight"
        );
        assert_eq!(
            shared.pauses.load(Ordering::SeqCst),
            0,
            "the pause behind the play waits for it"
        );
        assert!(
            !calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Report(_))),
            "the report behind the play waits for it"
        );
        hold.add_permits(1);
        until("the play", || shared.plays.load(Ordering::SeqCst) == 1);
        until("the pause", || shared.pauses.load(Ordering::SeqCst) == 1);
        until("the report", || {
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Report(_)))
        });
        let log = calls(&shared);
        let play = log
            .iter()
            .position(|call| matches!(call, Call::Play(_)))
            .expect("the play landed");
        let reported = log
            .iter()
            .position(|call| matches!(call, Call::Report(_)))
            .expect("the report landed");
        assert!(
            play < reported,
            "the instant behind the barrier lands after it: {log:?}"
        );
        let paused = log
            .iter()
            .position(|call| matches!(call, Call::Pause))
            .expect("the pause landed");
        assert!(
            play < paused && paused < reported,
            "the pause lands after the play and before the report queued behind it: {log:?}"
        );
    });
}

/// Runs the worker on the schedule whose link poll is short enough for a test
/// to notice a loop that stopped watching.
fn start_paced<H: SessionHost, S>(
    runtime: &Runtime,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) -> (UiEnd, WakeSlot, Worker)
where
    S: TokenSource + Send + Sync + 'static,
{
    start_with(
        runtime.handle(),
        host,
        store,
        source,
        WITHDRAW_TIMEOUT,
        Pacing::FAST,
    )
}

#[test]
fn an_idle_batch_hands_the_loop_back() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start_paced(&runtime, host, store, source);

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        // One read, with nothing behind it: no barrier ever ends this batch.
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("read");
        loop {
            if matches!(
                ui_end.updates.recv().expect("update"),
                Update::FollowedShows(_)
            ) {
                break;
            }
        }
        // The link poll is the only source left that can notice the drop.
        watched.down.store(true, Ordering::SeqCst);
        until("the link loss to retire the dead session", || {
            calls(&watched)
                .iter()
                .any(|call| matches!(call, Call::Retire))
        });
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve once the batch handed the loop back");
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_read_arriving_inside_the_grace_joins_the_batch() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start_paced(&runtime, host, store, source);

    let window = move || {
        let _ = expect_library(&ui_end);
        watch_now(&ui_end, &watched);
        let hold = hold_reads(&watched);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("read");
        until("the first read in flight", || in_flight(&watched) == 1);
        hold.add_permits(1);
        loop {
            if matches!(
                ui_end.updates.recv().expect("update"),
                Update::FollowedShows(_)
            ) {
                break;
            }
        }
        // A request the loop alone can answer, sent while the batch is idle.
        inbound_sender(&watched)
            .send(Inbound {
                id: RequestId(11),
                kind: Kind::Transfer,
                instruction: Instruction::Other(Kind::Transfer),
            })
            .expect("inbound");
        std::thread::sleep(IDLE_GRACE / 5);
        // The next frame's ask, well inside the grace: it joins the batch that
        // is still open, so the request above keeps waiting for a turn.
        ui_end
            .commands
            .send(Command::LoadFollowedArtists)
            .expect("read");
        until("the second read in flight", || in_flight(&watched) == 1);
        assert!(
            refusals(&watched).is_empty(),
            "the request waits while the read that joined the batch is in flight"
        );
        hold.add_permits(1);
        loop {
            if matches!(
                ui_end.updates.recv().expect("update"),
                Update::FollowedArtists(_)
            ) {
                break;
            }
        }
        // Nothing is left in flight, so the batch hands the loop back and the
        // request is answered.
        until("the request to be refused", || {
            !refusals(&watched).is_empty()
        });
    };
    let teardown = move || worker.finish();

    run_ordered(runtime, window, teardown);
}

#[test]
fn the_cap_bounds_the_reads_in_flight() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        let hold = hold_reads(&shared);
        let sent = MAX_CONCURRENT_READS + 2;
        for _ in 0..sent {
            ui_end
                .commands
                .send(Command::LoadFollowedShows)
                .expect("read");
        }
        until("the cap", || in_flight(&shared) == MAX_CONCURRENT_READS);
        expect_no_update(&ui_end);
        assert_eq!(
            in_flight(&shared),
            MAX_CONCURRENT_READS,
            "a read past the cap is not admitted while the cap is taken"
        );
        assert_eq!(
            shared.followed_lists.load(Ordering::SeqCst),
            MAX_CONCURRENT_READS,
            "only the reads inside the cap have been asked for"
        );
        hold.add_permits(sent);
        for _ in 0..sent {
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("every read queued behind the cap still answers");
        }
        assert_eq!(
            shared.reads_peak.load(Ordering::SeqCst),
            MAX_CONCURRENT_READS,
            "no more than the cap ever ran at once"
        );
    });
}

#[test]
fn a_write_is_a_barrier() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        let hold = hold_reads(&shared);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("read");
        until("the read in flight", || in_flight(&shared) == 1);
        ui_end
            .commands
            .send(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            })
            .expect("follow");
        expect_no_update(&ui_end);
        assert_eq!(
            shared.follow_writes.load(Ordering::SeqCst),
            0,
            "the write waits for the read in flight"
        );
        hold.add_permits(1);
        let updates = expect_updates(&ui_end, 2);
        assert_eq!(
            updates,
            vec![
                Update::FollowedShows(Vec::new()),
                Update::Followed {
                    artist: ARTIST.to_owned(),
                    followed: true,
                },
            ],
            "the read answers first, then the write"
        );
    });
}

#[test]
fn a_read_after_a_write_waits_for_it() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        let hold = hold_reads(&shared);
        ui_end
            .commands
            .send(Command::LoadFollowedShows)
            .expect("read");
        ui_end
            .commands
            .send(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            })
            .expect("follow");
        ui_end
            .commands
            .send(Command::LoadFollowedArtists)
            .expect("read");
        until("the first read in flight", || in_flight(&shared) == 1);
        expect_no_update(&ui_end);
        assert_eq!(
            shared.followed_artist_lists.load(Ordering::SeqCst),
            0,
            "the read behind the write is not admitted while the write is queued"
        );
        hold.add_permits(1);
        assert_eq!(
            expect_updates(&ui_end, 2),
            vec![
                Update::FollowedShows(Vec::new()),
                Update::Followed {
                    artist: ARTIST.to_owned(),
                    followed: true,
                },
            ],
            "the write answers before the read behind it starts"
        );
        until("the second read", || {
            shared.followed_artist_lists.load(Ordering::SeqCst) == 1
        });
        hold.add_permits(1);
        assert_eq!(
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("answer"),
            Update::FollowedArtists(Vec::new())
        );
    });
}

#[test]
fn a_stop_cancels_every_read_in_flight() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let watched = Arc::clone(&shared);
    hold_reads(&shared);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        for _ in 0..3 {
            ui_end
                .commands
                .send(Command::LoadFollowedShows)
                .expect("read");
        }
        until("three reads in flight", || in_flight(&watched) == 3);
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop cancels the reads in flight");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown runs even though three reads never answered"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn commands_deferred_by_a_rebuild_still_run_in_order() {
    let store: Arc<dyn CredentialStore> = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(ScriptedSource::sequencer(vec![
        good("access"),
        good("fresh"),
    ]));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
    let shared = Arc::clone(&host.shared);
    let probe = Arc::clone(&source);
    let patient = Pacing {
        attempt: Duration::from_secs(5),
        ..Pacing::FAST
    };

    let runtime = runtime();
    let (ui_end, slot, worker) = start_with(
        runtime.handle(),
        host,
        store,
        source,
        WITHDRAW_TIMEOUT,
        patient,
    );

    let window = move || {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        let (release, started) = probe.hold_next();
        shared.down.store(true, Ordering::SeqCst);
        started.recv().expect("the rebuild refresh started");
        for command in [
            Command::LoadLikedTracks,
            Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            },
            Command::LoadFollowedArtists,
        ] {
            ui_end.commands.send(command).expect("command");
        }
        expect_no_update(&ui_end);
        release.send(()).expect("release the refresh");
        let updates: Vec<Update> = (0..3)
            .map(|_| {
                ui_end
                    .updates
                    .recv_timeout(Duration::from_secs(2))
                    .expect("answer")
            })
            .collect();
        updates
    };
    let teardown = move || worker.finish();

    let updates = run_ordered(runtime, window, teardown);

    assert_eq!(
        updates,
        vec![
            Update::LikedTracks(Vec::new()),
            Update::Followed {
                artist: ARTIST.to_owned(),
                followed: true,
            },
            Update::FollowedArtists(Vec::new()),
        ],
        "the commands the attempt set aside run in the order they were sent"
    );
}

fn good(value: &str) -> Result<TokenSet, SourceError> {
    Ok(token_set(value, None))
}
