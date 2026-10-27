use super::super::*;
use super::test_support::*;

use eframe::App;
use std::sync::mpsc::channel;
use takofy_model::{Device, DeviceKind, Landing, RemotePlayback, RemoteState, Status};
use takofy_ui::message::{DeviceUpdate, Failure, TransportUpdate};

/// A window whose updates the test can feed, so `logic` drains them.
fn live_window() -> (
    TakofyApp,
    tokio::sync::mpsc::UnboundedReceiver<Command>,
    std::sync::mpsc::Sender<Update>,
) {
    let (mut app, received) = test_window();
    let (sender, updates) = channel();
    app.ui_end.updates = updates;
    (app, received, sender)
}

fn landing(rows: &[&str], index: usize) -> Landing {
    Landing {
        rows: rows.iter().map(|uri| (*uri).to_owned()).collect(),
        index,
        position_ms: 0,
        playing: true,
        context: Some("spotify:playlist:p".to_owned()),
        shuffle: Some(false),
        repeat_context: Some(false),
        repeat_track: Some(false),
        epoch: 1,
    }
}

#[test]
fn logic_sends_a_report_after_a_landing_drains() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");

    logic_frame(&mut app);
    let commands = take_commands(&mut received);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report))
                if report.status == Status::Loading
                    && report
                        .track
                        .as_ref()
                        .map(|track| track.uri.as_str())
                        == Some("spotify:track:a")
        )),
        "a landing is published as it drains: {commands:?}"
    );
}

#[test]
fn logic_sends_a_report_again_on_the_first_position_event() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");
    logic_frame(&mut app);
    let _ = take_commands(&mut received);

    updates
        .send(Update::Transport(TransportUpdate::Position {
            position_ms: 1_000,
            paused: false,
        }))
        .expect("position");
    logic_frame(&mut app);
    let commands = take_commands(&mut received);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report)) if report.status == Status::Playing
        )),
        "the first position turns the loading report into playing: {commands:?}"
    );
}

#[test]
fn ui_sends_a_report_when_a_click_changes_the_track() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");
    logic_frame(&mut app);
    let _ = take_commands(&mut received);

    // The effect a click on the second row has, applied before the draw.
    let _ = app.queue.jump(1, &mut app.transport);
    let ctx = window_context();
    let _ = ui_frame(&mut app, &ctx);

    let commands = take_commands(&mut received);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report))
                if report
                    .track
                    .as_ref()
                    .map(|track| track.uri.as_str())
                    == Some("spotify:track:b")
        )),
        "the draw's track change is published: {commands:?}"
    );
}

#[test]
fn an_answered_orders_effect_is_published_in_the_same_frame() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");
    logic_frame(&mut app);
    let _ = take_commands(&mut received);

    let id = RequestId(9);
    updates
        .send(Update::Remote(RemoteUpdate::Order {
            id,
            order: Order::Next(None),
            expires: Instant::now() + Duration::from_secs(5),
        }))
        .expect("order");
    logic_frame(&mut app);

    let commands = take_commands(&mut received);
    assert!(
        commands.contains(&Command::Answer { id, ok: true }),
        "the order is answered: {commands:?}"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report))
                if report
                    .track
                    .as_ref()
                    .map(|track| track.uri.as_str())
                    == Some("spotify:track:b")
        )),
        "the order's move is published in the frame it was applied: {commands:?}"
    );
}

fn watched(watches: &[Command]) -> usize {
    watches
        .iter()
        .filter(|command| matches!(command, Command::WatchDevices))
        .count()
}

#[test]
fn logic_sends_watch_devices_once_after_the_playlists_land() {
    let (mut app, mut received) = test_window();
    logic_frame(&mut app);
    assert_eq!(
        watched(&take_commands(&mut received)),
        0,
        "a watch was asked for before the account could be named"
    );

    app.library.playlists = Load::Ready(Vec::new());
    logic_frame(&mut app);
    assert_eq!(
        watched(&take_commands(&mut received)),
        1,
        "the playlists landing is what arms the watch"
    );

    logic_frame(&mut app);
    assert_eq!(
        watched(&take_commands(&mut received)),
        0,
        "a second frame asked the one-shot dealer to start again"
    );
}

#[test]
fn a_roster_drains_into_the_devices() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let now = Instant::now();
    tx.send(Update::Devices(DeviceUpdate::Roster {
        devices: vec![Device {
            id: "a".to_owned(),
            name: "Phone".to_owned(),
            kind: DeviceKind::Phone,
        }],
        holder: Some("a".to_owned()),
        held_here: false,
    }))
    .expect("send");

    let commands = drain(
        &rx,
        &mut state,
        &mut library,
        &mut transport,
        &mut queue,
        &mut devices,
        &mut Art::default(),
        &mut Playclock::at(now),
    );

    assert!(commands.is_empty());
    assert_eq!(devices.caption(now), "Phone");
    assert_eq!(
        devices.rows(&Transport::default(), false).len(),
        1,
        "the roster did not reach the card"
    );
}

/// A window already watching an account whose phone plays in this window's
/// own place: the remote transport is the one on screen.
fn remote_window(now: Instant) -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (mut app, received) = test_window();
    app.state = SignIn::SignedIn;
    app.library.playlists = Load::Ready(Vec::new());
    assert_eq!(app.devices.watch_request(true), Some(Command::WatchDevices));
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Roster {
            devices: vec![Device {
                id: "a".to_owned(),
                name: "Phone".to_owned(),
                kind: DeviceKind::Phone,
            }],
            holder: Some("a".to_owned()),
            held_here: false,
        }),
        now,
    );
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Playing(Some(RemotePlayback {
            track: track("spotify:track:a"),
            state: RemoteState::Playing,
            position_ms: 1_000,
            duration_ms: Some(60_000),
            steerable: true,
        }))),
        now,
    );
    (app, received)
}

/// The delay one `logic` pass asked for. The pass is the one a delayed
/// repaint is timed from, so the ask is read from that same output.
fn logic_delay(app: &mut TakofyApp) -> Duration {
    let ctx = window_context();
    // egui's own first passes ask for a frame; the app's ask is what is left.
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let mut frame = eframe::Frame::_new_kittest();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.logic(ui.ctx(), &mut frame);
    });
    output.textures_delta.clear();
    root_delay(&output)
}

#[test]
fn an_idle_window_is_woken_at_the_watch_patience_and_then_rests() {
    let (mut app, _received) = idle_window();

    let delay = logic_delay(&mut app);
    assert!(
        delay > REDRAW,
        "an idle window was redrawn at the transport cadence: {delay:?}"
    );
    assert!(
        delay <= takofy_ui::devices::WATCH_PATIENCE,
        "the watch's own patience is a wake this window must get: {delay:?}"
    );

    // A watch that has answered has no clock left, so the next pass asks
    // for nothing without a remote holding the sound.
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Failed(Failure::Devices)),
        Instant::now(),
    );
    assert_eq!(
        logic_delay(&mut app),
        Duration::MAX,
        "a settled idle window still woke for the watch"
    );
}

#[test]
fn a_flash_wakes_the_window_and_its_end_lets_it_rest() {
    let (mut app, _received) = idle_window();
    let now = Instant::now();
    // The watch has already settled, so the notice alone is asking.
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Failed(Failure::Devices)),
        now,
    );
    app.devices
        .apply(&Update::Devices(DeviceUpdate::SteerFailed), now);

    let delay = logic_delay(&mut app);
    assert!(
        delay <= takofy_ui::devices::FLASH_FOR,
        "the notice's own deadline is not the wake: {delay:?}"
    );

    assert!(
        app.devices.expire(now + takofy_ui::devices::FLASH_FOR),
        "the notice did not expire"
    );
    assert_eq!(
        logic_delay(&mut app),
        Duration::MAX,
        "the expired notice still woke the window"
    );
}

#[test]
fn logic_sends_the_refresh_a_card_open_asked_for() {
    let now = Instant::now();
    let (mut app, mut received) = remote_window(now);
    app.devices.list_opened();

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::RefreshDevices))
            .count(),
        1,
        "the open's refresh was not the logic pass's to send: {sent:?}"
    );

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::RefreshDevices))
            .count(),
        0,
        "the open's refresh was sent twice: {sent:?}"
    );
}

#[test]
fn logic_sends_the_pick_a_taken_frame_left_behind() {
    let now = Instant::now();
    let (mut app, mut received) = remote_window(now);
    // A press whose own frame already held a command keeps its pick.
    assert_eq!(app.devices.pick("a", false), None);

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::Transfer { .. }))
            .count(),
        1,
        "the delayed pick was not the logic pass's to send: {sent:?}"
    );

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        !sent
            .iter()
            .any(|command| matches!(command, Command::Transfer { .. })),
        "the delayed pick was sent twice: {sent:?}"
    );
}

#[test]
fn while_the_remote_plays_the_window_keeps_the_redraw_cadence() {
    let now = Instant::now();
    let (mut app, _received) = remote_window(now);

    let delay = logic_delay(&mut app);

    assert!(
        delay <= REDRAW,
        "the remote position would stand still: {delay:?}"
    );
}
