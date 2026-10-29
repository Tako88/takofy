use super::super::*;
use super::test_support::*;
use takofy_model::TrackEntry;

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
