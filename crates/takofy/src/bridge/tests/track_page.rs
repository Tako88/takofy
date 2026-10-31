use super::super::*;
use super::test_support::*;
use crate::session::TrackPage;
use std::sync::atomic::Ordering;
use takofy_model::{CoverSet, PlaylistAccess};

#[test]
fn a_failed_track_load_is_announced_and_the_task_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.tracks_script.lock().expect("lock") = TracksScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::TracksFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.tracks.load(Ordering::SeqCst), 1);
    assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
}

/// Drives one scripted track page through the live `load_tracks` path and
/// returns the `Update::Tracks` the window receives. This is the end-to-end
/// guard the direct-`Update` tests cannot give: every hop from the host's
/// page to the update is real.
fn run_track_page(page: TrackPage) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.tracks_script.lock().expect("lock") = TracksScript::Ready(page);

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("tracks")
    })
}

#[test]
fn a_track_page_cover_reaches_the_update() {
    let cover =
        CoverSet::single(CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover"));
    let update = run_track_page(TrackPage {
        tracks: vec![track("t")],
        cover: Some(cover.clone()),
        access: None,
    });
    assert_eq!(
        update,
        Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(cover),
            access: None,
        }
    );
}

#[test]
fn a_track_page_without_a_cover_reaches_the_update_as_none() {
    let update = run_track_page(TrackPage {
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    assert_eq!(
        update,
        Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        }
    );
}

#[test]
fn a_track_page_access_reaches_the_update() {
    let access = PlaylistAccess {
        revision: revision(1),
        editable: true,
    };
    let update = run_track_page(TrackPage {
        tracks: vec![track("t")],
        cover: None,
        access: Some(access.clone()),
    });
    assert_eq!(
        update,
        Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: Some(access),
        }
    );
}
