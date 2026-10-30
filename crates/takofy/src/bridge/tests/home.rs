use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;

use super::super::*;
use super::announce::{EpisodesScript, SHOW, followed_artist, show_detail};
use super::reads::{PortraitScript, portrait};
use super::test_support::*;
use takofy_model::TrackEntry;
use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, StationSeed};

#[test]
fn a_label_saved_read_answers_with_the_asked_and_named_tracks() {
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
            .send(Command::LabelSaved {
                tracks: vec![TRACK.to_owned()],
            })
            .expect("labels");
        ui_end.updates.recv().expect("labelled")
    });

    assert_eq!(
        update,
        Update::SavedLabelled {
            asked: vec![TRACK.to_owned()],
            named: vec![named],
        }
    );
    assert_eq!(calls(&shared), vec![Call::Label(vec![TRACK.to_owned()])]);
}

#[test]
fn a_failed_label_saved_read_reports_the_failure_with_the_asked_tracks() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.release_script.lock().expect("lock") = ReleaseScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LabelSaved {
                tracks: vec![TRACK.to_owned()],
            })
            .expect("labels");
        ui_end.updates.recv().expect("labelled")
    });

    assert_eq!(
        update,
        Update::SavedLabelFailed {
            asked: vec![TRACK.to_owned()],
            failure: Failure::Load,
        },
        "a failure is an answer the shelf can name"
    );
    assert_eq!(calls(&shared), vec![Call::Label(vec![TRACK.to_owned()])]);
}

#[test]
fn a_neighbour_read_failure_is_failure_related() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.related_script.lock().expect("lock") = RelatedScript::Fail;

    let ask = HomeAsk::Related {
        artist: ARTIST.to_owned(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::HomeFailed {
            ask: HomeAsk::Related {
                artist: ARTIST.to_owned(),
            },
            failure: Failure::Related,
        },
        "a shelf that could not read its neighbours names the failure"
    );
}

#[test]
fn related_answers_with_the_cards() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

    let ask = HomeAsk::Related {
        artist: ARTIST.to_owned(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("related")
    });

    assert_eq!(
        update,
        Update::Home(HomeAnswer::Related {
            artist: ARTIST.to_owned(),
            cards: vec![
                followed_artist(&format!("{ARTIST}:one")),
                followed_artist(&format!("{ARTIST}:two")),
            ],
        }),
        "one station read names the neighbours, with their names"
    );
}

#[test]
fn related_cards_carry_the_faces_one_read_names() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let face = format!("{ARTIST}:one");
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Ready(vec![portrait(&face)]);

    let ask = HomeAsk::Related {
        artist: ARTIST.to_owned(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("related")
    });

    let Update::Home(HomeAnswer::Related { cards, .. }) = update else {
        panic!("expected the related answer, got {update:?}");
    };
    assert_eq!(cards.len(), 2);
    assert!(
        cards[0].portrait.is_some(),
        "the card draws the face the portrait read named"
    );
    assert!(
        cards[1].portrait.is_none(),
        "an artist the read did not name keeps its hole"
    );
}

#[test]
fn related_failure_is_failure_related() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.related_script.lock().expect("lock") = RelatedScript::Fail;

    let ask = HomeAsk::Related {
        artist: ARTIST.to_owned(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::HomeFailed {
            ask: HomeAsk::Related {
                artist: ARTIST.to_owned(),
            },
            failure: Failure::Related,
        }
    );
}

/// A second show, so a pooled read names more than one.
const SECOND_SHOW: &str = "spotify:show:b";

/// A second artist, so a batched faces read names more than one.
const SECOND_ARTIST: &str = "spotify:artist:b";

#[test]
fn an_episodes_ask_answers_every_show_in_one_read() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let details = vec![show_detail(SHOW), show_detail(SECOND_SHOW)];
    *shared.episodes_script.lock().expect("lock") = EpisodesScript::Ready(details.clone());

    let shows = vec![SHOW.to_owned(), SECOND_SHOW.to_owned()];
    let ask = HomeAsk::Episodes {
        shows: shows.clone(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("episodes")
    });

    assert_eq!(update, Update::Home(HomeAnswer::Episodes(details)));
    assert_eq!(
        shared.episode_reads.load(Ordering::SeqCst),
        1,
        "one read names every show the shelf draws"
    );
    assert_eq!(
        shared.episode_asked.lock().expect("lock").clone(),
        vec![shows],
        "the one read names the shows it was asked for"
    );
}

#[test]
fn an_episodes_failure_is_failure_load() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.episodes_script.lock().expect("lock") = EpisodesScript::Fail;

    let shows = vec![SHOW.to_owned()];
    let ask = HomeAsk::Episodes {
        shows: shows.clone(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::HomeFailed {
            ask: HomeAsk::Episodes { shows },
            failure: Failure::Load,
        }
    );
}

#[test]
fn a_stop_during_an_episodes_read_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.episodes_script.lock().expect("lock") = EpisodesScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.episode_started.lock().expect("lock") = Some(started_tx);

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
            .send(Command::LoadHome(HomeAsk::Episodes {
                shows: vec![SHOW.to_owned()],
            }))
            .expect("home");
        started_rx.recv().expect("episodes started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the episodes read");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the episodes read"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn labels_answers_even_when_every_track_falls_back() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let asked = vec![TRACK.to_owned(), SECOND.to_owned()];

    // The default release read names nothing, so every row falls back to its
    // URI and the answer is still complete.
    let ask = HomeAsk::Labels {
        tracks: asked.clone(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("labels")
    });

    assert_eq!(
        update,
        Update::Home(HomeAnswer::Labelled {
            asked: asked.clone(),
            named: Vec::new(),
        })
    );
    assert_eq!(calls(&shared), vec![Call::Label(asked)]);
}

#[test]
fn labels_failure_is_failure_load() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.release_script.lock().expect("lock") = ReleaseScript::Fail;

    let tracks = vec![TRACK.to_owned()];
    let ask = HomeAsk::Labels {
        tracks: tracks.clone(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::HomeFailed {
            ask: HomeAsk::Labels { tracks },
            failure: Failure::Load,
        }
    );
}

#[test]
fn portraits_are_one_batched_read() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let artists = vec![ARTIST.to_owned(), SECOND_ARTIST.to_owned()];
    let found = vec![portrait(ARTIST), portrait(SECOND_ARTIST)];
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Ready(found.clone());

    let ask = HomeAsk::Portraits {
        artists: artists.clone(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("portraits")
    });

    assert_eq!(
        update,
        Update::Home(HomeAnswer::Portraits {
            asked: artists.clone(),
            found,
        }),
        "the answer names the artists it was asked about beside the faces it found"
    );
    assert_eq!(
        shared.portrait_reads.load(Ordering::SeqCst),
        1,
        "one read names every face the shelf draws"
    );
    assert_eq!(
        shared.portraited.lock().expect("lock").clone(),
        vec![artists],
        "the one read names the artists asked about"
    );
}

#[test]
fn a_portraits_read_that_answers_nothing_is_failure_load() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    // A refused request and an abandoned one are one answer from here on:
    // `Engine::portraits` has no failure to tell them apart.
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Fail;

    let artists = vec![ARTIST.to_owned()];
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadHome(HomeAsk::Portraits {
                artists: artists.clone(),
            }))
            .expect("home");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::HomeFailed {
            ask: HomeAsk::Portraits { artists },
            failure: Failure::Load,
        }
    );
}

#[test]
fn station_answers_the_canned_station() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

    let seed = StationSeed {
        kind: SeedKind::Artist,
        uri: ARTIST.to_owned(),
    };
    let ask = HomeAsk::Station { seed: seed.clone() };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("station")
    });

    assert_eq!(
        update,
        Update::Home(HomeAnswer::Station(canned_station(&seed)))
    );
}

#[test]
fn station_failure_is_failure_load() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.station_script.lock().expect("lock") = StationScript::Fail;

    let ask = HomeAsk::Station {
        seed: StationSeed {
            kind: SeedKind::Song,
            uri: TRACK.to_owned(),
        },
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadHome(ask.clone()))
            .expect("home");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::HomeFailed {
            ask,
            failure: Failure::Load,
        }
    );
}
