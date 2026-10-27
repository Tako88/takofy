use std::sync::atomic::Ordering;

use librespot::core::SessionConfig;
use librespot::playback::mixer::VolumeGetter;
use takofy_core::connect::Steer;
use takofy_core::playback::Volume;
use takofy_core::{Resolution, TransferPlan};
use takofy_model::{Report, RequestId, Status, TrackEntry};
use tokio::sync::oneshot;

use super::{LiveSession, Retired, SessionError, SessionHost, stop_or};

#[tokio::test]
async fn stop_or_returns_the_work_when_it_finishes_first() {
    let (_tx, mut stop) = oneshot::channel();
    assert_eq!(stop_or(&mut stop, async { 7 }).await, Some(7));
}

#[tokio::test]
async fn stop_or_gives_up_when_the_stop_arrives_first() {
    let (tx, mut stop) = oneshot::channel();
    tx.send(()).expect("stop");
    assert_eq!(
        stop_or(&mut stop, std::future::pending::<u32>()).await,
        None
    );
}

#[test]
fn a_level_set_before_the_player_exists_is_the_level_a_later_handle_reads() {
    let live = LiveSession::new(SessionConfig::default());
    live.set_volume(35);

    // The player is built with a clone of this cell, so what the clone
    // reads is what the player starts at.
    let handle = live.volume.clone();
    assert_eq!(
        handle.attenuation_factor(),
        Volume::at(35).attenuation_factor(),
        "a clone taken later reads the level set before the player existed"
    );

    live.set_volume(60);
    assert_eq!(
        handle.attenuation_factor(),
        Volume::at(60).attenuation_factor(),
        "a later change through the session is seen through the clone"
    );
}

#[tokio::test]
async fn the_device_calls_before_sign_in_report_the_missing_engine() {
    let mut live = LiveSession::new(SessionConfig::default());

    assert!(matches!(
        SessionHost::watch_devices(&mut live).await,
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::refresh_devices(&live).await,
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::steer(&live, "phone", Steer::Pause).await,
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::hand_over(&live, "phone").await,
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::withdraw_devices(&live).await,
        Err(SessionError::NotConnected)
    ));
    assert!(SessionHost::take_devices(&mut live).is_none());
}

#[tokio::test]
async fn a_watch_asked_for_twice_hands_back_the_stream_it_has() {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut live = LiveSession::new(SessionConfig::default());
    live.devices = Some(rx);

    // The stream is the guard: a second request cannot start a second
    // watch, and a window that asks again gets what already exists.
    assert!(SessionHost::watch_devices(&mut live).await.is_ok());
    assert!(SessionHost::take_devices(&mut live).is_some());
    assert!(SessionHost::take_devices(&mut live).is_none());
}

#[tokio::test]
async fn a_second_watch_after_success_returns_ok_without_a_second_start() {
    let (_devices_tx, devices) = tokio::sync::mpsc::unbounded_channel();
    let (_inbound_tx, inbound) = tokio::sync::mpsc::unbounded_channel();
    let mut live = LiveSession::new(SessionConfig::default());
    live.devices = Some(devices);
    live.inbound = Some(inbound);

    // The stored streams are the guard: a second request answers `Ok`
    // without reaching the engine, whose dealer start is already spent.
    assert!(SessionHost::watch_devices(&mut live).await.is_ok());
    assert!(SessionHost::take_devices(&mut live).is_some());
    assert!(SessionHost::take_inbound(&mut live).is_some());
    assert!(
        SessionHost::take_inbound(&mut live).is_none(),
        "the request stream is handed out once"
    );
}

#[tokio::test]
async fn take_inbound_hands_the_stream_out_once() {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut live = LiveSession::new(SessionConfig::default());
    live.inbound = Some(rx);

    assert!(SessionHost::take_inbound(&mut live).is_some());
    assert!(SessionHost::take_inbound(&mut live).is_none());
}

#[test]
fn set_playable_and_refuse_before_sign_in_do_nothing_and_do_not_panic() {
    let live = LiveSession::new(SessionConfig::default());

    // Both are safe before there is an engine: the playability report and
    // the refusal have nowhere to go, and neither panics.
    SessionHost::set_playable(&live, true);
    SessionHost::set_playable(&live, false);
    SessionHost::refuse(&live, RequestId(1));
}

#[test]
fn a_report_before_sign_in_is_ignored() {
    let live = LiveSession::new(SessionConfig::default());
    let report = report();

    // There is no engine to report to yet, so the call has nowhere to go and
    // must not panic.
    SessionHost::report(&live, Some(report));
    SessionHost::report(&live, None);
}

/// The report the window would publish while a track plays.
fn report() -> Report {
    Report {
        status: Status::Playing,
        track: Some(TrackEntry {
            uri: "spotify:track:x".to_owned(),
            title: "x".to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }),
        prev: Vec::new(),
        next: Vec::new(),
        context: None,
        queue_revision: String::new(),
        options: None,
        position_ms: 0,
        duration_ms: None,
    }
}

#[test]
fn playability_and_the_last_report_are_held_for_a_rebuilt_session() {
    let live = LiveSession::new(SessionConfig::default());

    // A fresh engine starts blank, so what the window said before the drop
    // has to be held here to be told again.
    SessionHost::set_playable(&live, true);
    SessionHost::report(&live, Some(report()));
    assert!(live.playable.load(Ordering::SeqCst));
    assert_eq!(
        *live.report.lock().expect("lock"),
        Some(report()),
        "the newest report is held, not only forwarded"
    );

    SessionHost::set_playable(&live, false);
    SessionHost::report(&live, None);
    assert!(!live.playable.load(Ordering::SeqCst));
    assert_eq!(*live.report.lock().expect("lock"), None);
}

#[test]
fn a_lost_link_is_named_for_the_log_without_the_reason() {
    assert_eq!(SessionError::LinkLost.kind(), "link lost");
}

#[test]
fn a_session_with_no_engine_is_not_down() {
    let live = LiveSession::new(SessionConfig::default());
    assert!(
        !SessionHost::is_down(&live),
        "a window that never connected has no link to lose"
    );
}

#[tokio::test]
async fn a_load_before_sign_in_reports_the_missing_engine_not_a_lost_link() {
    let mut live = LiveSession::new(SessionConfig::default());
    assert!(matches!(
        SessionHost::load(&mut live, "spotify:track:x", 0, true),
        Err(SessionError::NotConnected)
    ));
}

#[tokio::test]
async fn retiring_an_empty_session_has_nothing_to_join() {
    let mut live = LiveSession::new(SessionConfig::default());
    assert_eq!(SessionHost::retire(&mut live).await, Retired::Joined);
    assert!(!SessionHost::is_down(&live));
}

#[tokio::test]
async fn retiring_drops_the_streams_the_old_session_held() {
    let (_devices_tx, devices) = tokio::sync::mpsc::unbounded_channel();
    let (_inbound_tx, inbound) = tokio::sync::mpsc::unbounded_channel();
    let mut live = LiveSession::new(SessionConfig::default());
    live.devices = Some(devices);
    live.inbound = Some(inbound);

    assert_eq!(SessionHost::retire(&mut live).await, Retired::Joined);

    assert!(
        SessionHost::take_devices(&mut live).is_none(),
        "a retired drain is handed nothing more"
    );
    assert!(SessionHost::take_inbound(&mut live).is_none());
}

#[test]
fn play_before_sign_in_fails_and_keeps_the_level() {
    let mut live = LiveSession::new(SessionConfig::default());
    live.set_volume(35);

    let error = SessionHost::play(&mut live, "spotify:track:x")
        .expect_err("a play with no engine must fail");
    assert!(
        matches!(error, SessionError::NotConnected),
        "a play before sign-in reports the missing engine"
    );
    assert_eq!(
        live.volume.attenuation_factor(),
        Volume::at(35).attenuation_factor(),
        "a failed play leaves the level alone"
    );
}

/// A plan can only be built from the wire's own shape, so a test outside
/// core decodes one the way the request loop does.
fn plan() -> TransferPlan {
    use librespot::core::dealer::protocol::{
        Command, LoggingParams, Request, TransferCommand, TransferOptions,
    };
    use librespot::protocol::context_track::ContextTrack;
    use librespot::protocol::playback::Playback as WirePlayback;
    use librespot::protocol::transfer_state::TransferState;
    use takofy_core::instruction::{Instruction, instruction_of};

    let track = ContextTrack {
        uri: Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned()),
        ..Default::default()
    };
    let playback = WirePlayback {
        current_track: Some(track).into(),
        ..Default::default()
    };
    let data = TransferState {
        playback: Some(playback).into(),
        ..Default::default()
    };
    let request = Request {
        message_id: 1,
        sent_by_device_id: "phone".to_owned(),
        command: Command::Transfer(TransferCommand {
            data: Some(data),
            options: TransferOptions::default(),
            from_device_identifier: String::new(),
            logging_params: LoggingParams {
                interaction_ids: None,
                device_identifier: None,
                command_initiated_time: None,
                page_instance_ids: None,
                command_id: None,
            },
        }),
    };
    match instruction_of(&request) {
        Instruction::Transfer(plan) => plan,
        other => panic!("a transfer naming a track must decode to a plan: {other:?}"),
    }
}

#[tokio::test]
async fn each_new_host_call_before_sign_in_returns_not_connected() {
    let mut live = LiveSession::new(SessionConfig::default());
    let plan = plan();

    assert!(matches!(
        SessionHost::prepare_player(&mut live),
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::resolve_context(&live, "spotify:playlist:abc").await,
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::load(&mut live, "spotify:track:x", 5_000, false),
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::accept(&live, RequestId(1), &plan, Resolution::Resolved(Vec::new())).await,
        Err(SessionError::NotConnected)
    ));
    assert!(matches!(
        SessionHost::release_sound(&live).await,
        Err(SessionError::NotConnected)
    ));
    assert!(
        !SessionHost::holding(&live),
        "a window with no session holds nothing"
    );
}
