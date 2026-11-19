use super::*;
use crate::soloist::frames::{Options, Repeat};
use std::time::Duration;
use tokio::time::Instant;

fn load(position_ms: u32, play: bool) -> Load {
    Load {
        uri: "spotify:track:a".to_owned(),
        position_ms,
        play,
    }
}

/// Drives the prelude of the first load and returns the play command.
fn first_play(sequence: &mut Sequence, now: Instant) -> Vec<Command> {
    let mut commands = Vec::new();
    let output = sequence.begin(1, load(0, true), now);
    commands.extend(output.commands);
    for expected in ["activate", "set_shuffle", "set_repeat_track"] {
        let output = sequence.on_result(expected, now);
        commands.extend(output.commands);
    }
    let output = sequence.on_result("set_repeat_context", now);
    commands.extend(output.commands);
    commands
}

#[test]
fn the_first_load_activates_then_normalises_then_plays() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    let commands = first_play(&mut sequence, now);
    assert_eq!(
        commands,
        vec![
            Command::Activate,
            Command::SetShuffle { enabled: false },
            Command::SetRepeatTrack { enabled: false },
            Command::SetRepeatContext { enabled: false },
            Command::Play {
                uri: "spotify:track:a".to_owned()
            },
        ]
    );
    let output = sequence.on_result("play", now);
    assert!(output.commands.is_empty());
}

#[test]
fn a_later_load_plays_without_reactivating() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    let output = sequence.begin(2, load(0, true), now);
    assert_eq!(
        output.commands,
        vec![Command::Play {
            uri: "spotify:track:a".to_owned()
        }]
    );
}

#[test]
fn a_seek_waits_for_attach_then_runs_before_the_pause() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    // A non-zero position asks for a seek.
    let output = sequence.begin(2, load(60_000, false), now);
    assert_eq!(
        output.commands,
        vec![Command::Play {
            uri: "spotify:track:a".to_owned()
        }]
    );
    let output = sequence.on_result("play", now);
    assert!(output.commands.is_empty(), "seek must wait for attach");

    let attach = Frame::TrackChanged {
        uri: "spotify:track:a".to_owned(),
        duration_ms: Some(210_000),
    };
    let output = sequence.on_frame(&attach, now);
    assert_eq!(
        output.commands,
        vec![Command::Seek {
            position_ms: 60_000
        }]
    );

    let output = sequence.on_result("seek", now);
    assert!(output.commands.is_empty());
    let sync = Frame::PositionSync(crate::soloist::position::Anchor {
        position_ms: 60_400,
        timestamp_ms: 1,
        speed: 0.0,
    });
    let output = sequence.on_frame(&sync, now);
    assert_eq!(output.commands, vec![Command::Pause]);
    let output = sequence.on_result("pause", now);
    assert!(output.commands.is_empty());
}

#[test]
fn a_seek_is_sent_without_attach_after_the_wait() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    sequence.begin(2, load(60_000, true), now);
    sequence.on_result("play", now);
    let early = sequence.tick(now + SEEK_WAIT - Duration::from_millis(1));
    assert!(early.commands.is_empty());
    let late = sequence.tick(now + SEEK_WAIT);
    assert_eq!(
        late.commands,
        vec![Command::Seek {
            position_ms: 60_000
        }]
    );
}

#[test]
fn an_unconfirmed_seek_is_resent_once_then_dropped() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    sequence.begin(2, load(60_000, false), now);
    sequence.on_result("play", now);
    sequence.on_frame(
        &Frame::TrackChanged {
            uri: "spotify:track:a".to_owned(),
            duration_ms: None,
        },
        now,
    );
    sequence.on_result("seek", now);
    let resent = sequence.tick(now + VERIFY_BUDGET);
    assert_eq!(
        resent.commands,
        vec![Command::Seek {
            position_ms: 60_000
        }]
    );
    sequence.on_result("seek", now + VERIFY_BUDGET);
    let dropped = sequence.tick(now + VERIFY_BUDGET + VERIFY_BUDGET);
    assert_eq!(dropped.commands, vec![Command::Pause]);
}

#[test]
fn a_matching_anchor_confirms_the_seek() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    sequence.begin(2, load(60_000, true), now);
    sequence.on_result("play", now);
    sequence.on_frame(
        &Frame::TrackChanged {
            uri: "spotify:track:a".to_owned(),
            duration_ms: None,
        },
        now,
    );
    sequence.on_result("seek", now);
    let sync = Frame::PositionSync(crate::soloist::position::Anchor {
        position_ms: 61_400,
        timestamp_ms: 1,
        speed: 0.0,
    });
    let output = sequence.on_frame(&sync, now);
    assert!(output.commands.is_empty());
    assert!(sequence.tick(now + VERIFY_BUDGET).commands.is_empty());
}

#[test]
fn a_refused_play_stops_the_sequence() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    let output = sequence.on_error(now);
    assert!(output.commands.is_empty());
    assert!(
        sequence
            .tick(now + Duration::from_secs(60))
            .commands
            .is_empty()
    );
    assert!(sequence.on_result("play", now).commands.is_empty());
}

#[test]
fn a_refused_seek_still_pauses() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    sequence.begin(2, load(60_000, false), now);
    sequence.on_result("play", now);
    sequence.on_frame(
        &Frame::TrackChanged {
            uri: "spotify:track:a".to_owned(),
            duration_ms: None,
        },
        now,
    );
    let output = sequence.on_error(now);
    assert_eq!(output.commands, vec![Command::Pause]);
}

#[test]
fn a_refused_pause_is_retried_once_then_reported() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    sequence.begin(2, load(0, false), now);
    let output = sequence.on_result("play", now);
    assert_eq!(output.commands, vec![Command::Pause]);
    let retry = sequence.on_error(now);
    assert_eq!(retry.commands, vec![Command::Pause]);
    let given_up = sequence.on_error(now);
    assert!(given_up.commands.is_empty());
    assert!(given_up.pause_refused);
}

#[test]
fn a_new_load_supersedes_an_unfinished_one() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    sequence.begin(1, load(60_000, true), now);
    let output = sequence.begin(2, load(0, true), now);
    assert_eq!(output.commands, vec![Command::Activate]);
    assert!(sequence.on_result("play", now).commands.is_empty());
    let next = sequence.on_result("activate", now);
    assert_eq!(next.commands, vec![Command::SetShuffle { enabled: false }]);
}

#[test]
fn a_superseded_plays_error_is_not_charged_to_the_new_load() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    let output = sequence.begin(2, load(60_000, true), now);
    assert_eq!(
        output.commands,
        vec![Command::Play {
            uri: "spotify:track:a".to_owned()
        }]
    );

    let stale = sequence.on_error(now);
    assert!(stale.commands.is_empty());
    assert!(stale.superseded);

    // The new load's own play still moves it on, so its seek follows attach.
    assert!(!sequence.on_result("play", now).superseded);
    let attach = Frame::TrackChanged {
        uri: "spotify:track:a".to_owned(),
        duration_ms: Some(210_000),
    };
    assert_eq!(
        sequence.on_frame(&attach, now).commands,
        vec![Command::Seek {
            position_ms: 60_000
        }]
    );
}

#[test]
fn an_answer_claim_lapses_after_the_command_budget() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.begin(2, load(0, true), now);
    assert!(!sequence.on_error(now + COMMAND_BUDGET).superseded);
}

#[test]
fn a_stopped_sequence_owes_the_next_load_one_answer() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.abort(now);
    let output = sequence.begin(2, load(0, true), now);
    assert_eq!(
        output.commands,
        vec![Command::Play {
            uri: "spotify:track:a".to_owned()
        }]
    );
    assert!(sequence.on_result("play", now).superseded);
    assert!(!sequence.on_result("play", now).superseded);
}

#[test]
fn another_device_taking_the_sound_stops_the_sequence() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    let output = sequence.on_frame(
        &Frame::DeviceChanged {
            is_active: false,
            device_name: Some("phone".to_owned()),
        },
        now,
    );
    assert!(output.stopped);
    assert!(output.commands.is_empty());

    // The command the device loss dropped still owes the next load one answer,
    // and the activate it starts with is not the one swallowed.
    let next = sequence.begin(2, load(0, true), now);
    assert_eq!(next.commands, vec![Command::Activate]);
    assert!(sequence.on_result("play", now).superseded);
    assert_eq!(
        sequence.on_result("activate", now).commands,
        vec![Command::Play {
            uri: "spotify:track:a".to_owned()
        }]
    );
}

#[test]
fn drift_in_soloist_options_normalises_again_on_the_next_load() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    first_play(&mut sequence, now);
    sequence.on_result("play", now);
    sequence.on_frame(
        &Frame::OptionsChanged {
            options: Options {
                shuffle: true,
                repeat: Repeat::Off,
            },
        },
        now,
    );
    let output = sequence.begin(2, load(0, true), now);
    assert_eq!(
        output.commands,
        vec![Command::SetShuffle { enabled: false }]
    );
}

#[test]
fn a_mismatched_result_is_ignored() {
    let now = Instant::now();
    let mut sequence = Sequence::default();
    sequence.begin(1, load(0, true), now);
    assert!(sequence.on_result("pause", now).commands.is_empty());
    let next = sequence.on_result("activate", now);
    assert_eq!(next.commands, vec![Command::SetShuffle { enabled: false }]);
}
