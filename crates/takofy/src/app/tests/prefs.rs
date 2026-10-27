use super::super::*;
use super::test_support::*;

use crate::preferences::PrefsWriter;
use eframe::App;
use std::sync::{Arc, Mutex};
use takofy_ui::prefs::QUIET;
use takofy_ui::repeat::Repeat;

type PrefsLog = Arc<Mutex<Vec<Prefs>>>;

/// A sink that keeps every value it is handed.
fn recording() -> (
    PrefsLog,
    impl Fn(&Prefs) -> std::io::Result<()> + Send + 'static,
) {
    let log: PrefsLog = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |prefs: &Prefs| {
            log.lock().expect("the sink log").push(prefs.clone());
            Ok(())
        }
    };
    (log, sink)
}

/// A sync whose file already holds `written`, saving through `sink`.
fn synced(
    written: Prefs,
    sink: impl Fn(&Prefs) -> std::io::Result<()> + Send + 'static,
) -> PrefsSync {
    PrefsSync::for_test(
        written,
        PrefsWriter::with_sink(sink, Duration::from_secs(2)),
    )
}

fn found(text: &str) -> Loaded {
    Loaded::Found(Prefs::parse(text).expect("valid prefs"))
}

/// The volume commands the frames since the last call sent.
fn volume_frames(received: &mut tokio::sync::mpsc::UnboundedReceiver<Command>) -> Vec<u16> {
    let mut levels = Vec::new();
    while let Ok(command) = received.try_recv() {
        if let Command::Volume { percent } = command {
            levels.push(percent);
        }
    }
    levels
}

#[test]
fn restored_state_builds_the_transport_and_queue() {
    let (transport, queue) =
        TakofyApp::restored_state(&found("volume=40\nmute=false\nshuffle=true\nrepeat=all\n"));
    assert_eq!(
        transport.volume.level(),
        40,
        "the saved level seeds the bar"
    );
    assert!(!transport.volume.muted(), "the file was not muted");
    assert!(queue.shuffle(), "the saved shuffle seeds the queue");
    assert_eq!(queue.repeat(), Repeat::All, "the saved repeat seeds it too");
}

#[test]
fn a_restored_volume_is_sent_once_on_the_first_frame() {
    let (mut app, mut received) = test_window_with(found("volume=40\n"), PrefsSync::off());
    let ctx = window_context();
    let _ = ui_frame(&mut app, &ctx);
    assert_eq!(
        volume_frames(&mut received),
        vec![40],
        "a remembered level reaches the player on the first frame"
    );
    let _ = ui_frame(&mut app, &ctx);
    assert!(
        volume_frames(&mut received).is_empty(),
        "one change is one command"
    );
}

#[test]
fn a_restored_mute_sends_zero() {
    let (mut app, mut received) =
        test_window_with(found("volume=70\nmute=true\n"), PrefsSync::off());
    let ctx = window_context();
    let _ = ui_frame(&mut app, &ctx);
    assert_eq!(
        volume_frames(&mut received),
        vec![0],
        "a remembered mute silences the player"
    );
    assert_eq!(
        app.transport.volume.level(),
        70,
        "the bar keeps the level behind the lid"
    );
}

#[test]
fn default_prefs_send_no_volume_command() {
    for loaded in [
        Loaded::Absent,
        Loaded::Refused("line 3: bad value for `repeat`".to_owned()),
        Loaded::Unreadable,
    ] {
        let (mut app, mut received) = test_window_with(loaded, PrefsSync::off());
        let ctx = window_context();
        let _ = ui_frame(&mut app, &ctx);
        assert!(
            volume_frames(&mut received).is_empty(),
            "nothing is owed when the file named no level"
        );
    }
}

#[test]
fn a_changed_volume_asks_for_a_wake_within_quiet() {
    let (mut app, _received) = idle_window();
    let ctx = window_context();
    let mut idle = Duration::ZERO;
    for _ in 0..10 {
        idle = root_delay(&ui_frame(&mut app, &ctx));
        if idle == Duration::MAX {
            break;
        }
    }
    assert_eq!(idle, Duration::MAX, "an idle window must not be woken");

    assert!(app.transport.volume.set_fraction(0.4));
    let delay = root_delay(&ui_frame(&mut app, &ctx));
    assert!(
        delay < Duration::MAX,
        "a pending value must wake the window, at {delay:?}"
    );
    assert!(
        delay <= QUIET,
        "the write is due within the quiet window: {delay:?}"
    );
}

#[test]
fn on_exit_flushes_an_unsettled_change() {
    let (log, sink) = recording();
    let written = Prefs::parse("volume=40\n").expect("valid prefs");
    let (mut app, _received) =
        test_window_with(Loaded::Found(written.clone()), synced(written, sink));
    assert!(app.transport.volume.set_fraction(0.6));
    let changed = Prefs::of(&app.transport.volume, &app.queue);

    App::on_exit(&mut app, None);
    drop(app);

    assert_eq!(
        *log.lock().expect("the sink log"),
        vec![changed],
        "close writes the value no frame had time to settle"
    );
}

#[test]
fn on_exit_after_a_settled_write_sends_nothing_more() {
    let (log, sink) = recording();
    let written = Prefs::parse("volume=40\n").expect("valid prefs");
    let (mut app, _received) =
        test_window_with(Loaded::Found(written.clone()), synced(written, sink));
    assert!(app.transport.volume.set_fraction(0.6));
    let changed = Prefs::of(&app.transport.volume, &app.queue);
    let now = Instant::now();
    assert_eq!(
        app.prefs.observe(now, &changed),
        Some(QUIET),
        "the change is armed, not written"
    );
    assert_eq!(
        app.prefs.observe(now + QUIET, &changed),
        None,
        "quiet has passed, so the value is written"
    );

    App::on_exit(&mut app, None);
    drop(app);

    assert_eq!(
        *log.lock().expect("the sink log"),
        vec![changed],
        "a settled value is not written again"
    );
}

#[test]
fn an_untouched_session_never_writes() {
    let (log, sink) = recording();
    let written = Prefs::parse("volume=40\nshuffle=true\nrepeat=all\n").expect("valid prefs");
    let (mut app, mut received) =
        test_window_with(Loaded::Found(written.clone()), synced(written, sink));
    let ctx = window_context();
    for _ in 0..3 {
        let _ = ui_frame(&mut app, &ctx);
    }
    assert_eq!(
        volume_frames(&mut received),
        vec![40],
        "the restored level went out once, so the frames did run"
    );

    App::on_exit(&mut app, None);
    drop(app);

    assert!(
        log.lock().expect("the sink log").is_empty(),
        "nothing changed, so nothing is written"
    );
}
