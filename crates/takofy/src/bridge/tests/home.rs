use super::super::*;
use super::announce::{SHOW, ShowScript, followed_artist, show_detail};
use super::reads::{ArtistScript, artist_detail};
use super::test_support::*;
use crate::session::ArtistPage;
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
fn an_artist_read_failure_is_failure_related() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.artist_script.lock().expect("lock") = ArtistScript::Fail;

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
        "the card read never ran, yet the sheet names the likeness failure"
    );
}

#[test]
fn related_answers_with_the_cards() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    let mut page = ArtistPage {
        artist: artist_detail(ARTIST),
        followed: Some(false),
    };
    page.artist.related = vec![SECOND.to_owned()];
    *shared.artist_script.lock().expect("lock") = ArtistScript::Ready(page);

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
            cards: vec![followed_artist(SECOND)],
        })
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

#[test]
fn show_answers() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

    let ask = HomeAsk::Show {
        show: SHOW.to_owned(),
    };
    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end.commands.send(Command::LoadHome(ask)).expect("home");
        ui_end.updates.recv().expect("show")
    });

    assert_eq!(update, Update::Home(HomeAnswer::Show(show_detail(SHOW))));
}

#[test]
fn show_failure() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
    let shared = Arc::clone(&host.shared);
    *shared.show_script.lock().expect("lock") = ShowScript::Fail;

    let ask = HomeAsk::Show {
        show: SHOW.to_owned(),
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
            ask: HomeAsk::Show {
                show: SHOW.to_owned(),
            },
            failure: Failure::Load,
        }
    );
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
