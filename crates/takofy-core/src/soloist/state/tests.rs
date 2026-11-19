use super::*;
use crate::soloist::frames::{Options, Repeat, Snapshot};
use crate::soloist::position::Anchor as FrameAnchor;
use tokio::time::Instant;

fn expecting(generation: u64, uri: &str) -> Expect {
    Expect {
        generation,
        uri: uri.to_owned(),
        seen_attached: false,
    }
}

fn snapshot(status: Status, uri: &str, duration_ms: Option<u32>, position_ms: u32) -> Snapshot {
    Snapshot {
        status,
        item_uri: Some(uri.to_owned()),
        duration_ms,
        position: Some(FrameAnchor {
            position_ms,
            timestamp_ms: 1,
            speed: 1.0,
        }),
        volume: Some(50),
        is_active: Some(true),
        options: Some(Options {
            shuffle: false,
            repeat: Repeat::Off,
        }),
    }
}

#[test]
fn auth_state_false_is_unpaired_in_the_state() {
    let mut fold = Fold::default();
    let mut expect = Expect::default();
    let events = fold.apply(
        &Frame::AuthState {
            logged_in: false,
            is_active: false,
            device_name: "T".to_owned(),
        },
        Instant::now(),
        &mut expect,
    );
    assert!(events.is_empty());
    assert!(fold.state.auth_known);
    assert!(!fold.state.logged_in);
    assert_eq!(fold.state.pairing(), Some(SoloistFault::Unpaired));
}

#[test]
fn an_accepted_play_loads_before_attach() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    let events = fold.apply(
        &Frame::CommandResult {
            command: "play".to_owned(),
        },
        now,
        &mut expect,
    );
    assert_eq!(events, vec![PlaybackEvent::Loading]);
    let again = fold.apply(
        &Frame::CommandResult {
            command: "play".to_owned(),
        },
        now,
        &mut expect,
    );
    assert!(again.is_empty());
}

#[test]
fn buffering_is_loading_once() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    let first = fold.apply(
        &Frame::PlaybackChanged {
            status: Status::Buffering,
        },
        now,
        &mut expect,
    );
    assert_eq!(first, vec![PlaybackEvent::Loading]);
    let second = fold.apply(
        &Frame::PlaybackChanged {
            status: Status::Buffering,
        },
        now,
        &mut expect,
    );
    assert!(second.is_empty());
}

#[test]
fn a_full_state_reports_duration_then_playing() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    let events = fold.apply(
        &Frame::PlaybackState(snapshot(
            Status::Playing,
            "spotify:track:a",
            Some(210_000),
            45_000,
        )),
        now,
        &mut expect,
    );
    assert_eq!(
        events,
        vec![
            PlaybackEvent::Duration {
                duration_ms: 210_000
            },
            PlaybackEvent::Playing {
                position_ms: 45_000
            },
        ]
    );
    assert!(expect.seen_attached);
    assert_eq!(fold.state.duration_ms, Some(210_000));
    assert_eq!(fold.state.status, Status::Playing);
}

#[test]
fn a_load_started_while_playing_reports_playing_again() {
    let mut fold = Fold::default();
    let now = Instant::now();
    let mut first = expecting(1, "spotify:track:a");
    fold.apply(
        &Frame::PlaybackState(snapshot(
            Status::Playing,
            "spotify:track:a",
            Some(210_000),
            0,
        )),
        now,
        &mut first,
    );
    let mut next = expecting(2, "spotify:track:b");
    let events = fold.apply(
        &Frame::PlaybackChanged {
            status: Status::Playing,
        },
        now,
        &mut next,
    );
    assert!(matches!(
        events.as_slice(),
        [
            PlaybackEvent::Duration {
                duration_ms: 210_000
            },
            PlaybackEvent::Playing { .. }
        ]
    ));
}

#[test]
fn a_repeated_status_emits_nothing() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    let frame = Frame::PlaybackState(snapshot(Status::Playing, "spotify:track:a", Some(1000), 0));
    fold.apply(&frame, now, &mut expect);
    assert!(fold.apply(&frame, now, &mut expect).is_empty());
}

#[test]
fn an_idle_status_is_not_ended_here() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    fold.apply(
        &Frame::PlaybackState(snapshot(Status::Playing, "spotify:track:a", Some(1000), 0)),
        now,
        &mut expect,
    );
    let events = fold.apply(
        &Frame::PlaybackChanged {
            status: Status::Idle,
        },
        now,
        &mut expect,
    );
    assert!(events.is_empty());
}

#[test]
fn duration_is_reported_once_per_value_and_generation() {
    let mut fold = Fold::default();
    let now = Instant::now();
    let mut expect = expecting(1, "spotify:track:a");
    let frame = Frame::TrackChanged {
        uri: "spotify:track:a".to_owned(),
        duration_ms: Some(1000),
    };
    assert_eq!(
        fold.apply(&frame, now, &mut expect),
        vec![PlaybackEvent::Duration { duration_ms: 1000 }]
    );
    assert!(fold.apply(&frame, now, &mut expect).is_empty());

    let changed = Frame::TrackChanged {
        uri: "spotify:track:a".to_owned(),
        duration_ms: Some(2000),
    };
    assert_eq!(
        fold.apply(&changed, now, &mut expect),
        vec![PlaybackEvent::Duration { duration_ms: 2000 }]
    );

    let mut next = expecting(2, "spotify:track:b");
    assert_eq!(
        fold.apply(&changed, now, &mut next),
        vec![PlaybackEvent::Duration { duration_ms: 2000 }]
    );
}

#[test]
fn a_jump_in_a_position_sync_is_a_seek() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    let first = Frame::PositionSync(FrameAnchor {
        position_ms: 1_000,
        timestamp_ms: 1,
        speed: 1.0,
    });
    assert!(fold.apply(&first, now, &mut expect).is_empty());
    let second = Frame::PositionSync(FrameAnchor {
        position_ms: 5_000,
        timestamp_ms: 2,
        speed: 1.0,
    });
    assert_eq!(
        fold.apply(&second, now, &mut expect),
        vec![PlaybackEvent::Seeked { position_ms: 5_000 }]
    );
}

#[test]
fn a_small_position_correction_is_not_a_seek() {
    let mut fold = Fold::default();
    let mut expect = expecting(1, "spotify:track:a");
    let now = Instant::now();
    let first = Frame::PositionSync(FrameAnchor {
        position_ms: 1_000,
        timestamp_ms: 1,
        speed: 1.0,
    });
    fold.apply(&first, now, &mut expect);
    let second = Frame::PositionSync(FrameAnchor {
        position_ms: 1_400,
        timestamp_ms: 2,
        speed: 1.0,
    });
    assert!(fold.apply(&second, now, &mut expect).is_empty());
}

#[test]
fn options_and_volume_are_queryable() {
    let mut fold = Fold::default();
    let mut expect = Expect::default();
    let now = Instant::now();
    fold.apply(
        &Frame::OptionsChanged {
            options: Options {
                shuffle: true,
                repeat: Repeat::Track,
            },
        },
        now,
        &mut expect,
    );
    fold.apply(&Frame::VolumeChanged { volume: 12 }, now, &mut expect);
    assert!(fold.state.options.shuffle);
    assert_eq!(fold.state.options.repeat, Repeat::Track);
    assert_eq!(fold.state.volume, Some(12));
}
