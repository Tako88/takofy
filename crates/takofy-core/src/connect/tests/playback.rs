use protobuf::MessageField;
use takofy_model::RemoteState;

use super::super::*;
use super::test_support::*;

fn playback_at(position: i64, timestamp: i64, server: i64, duration: i64) -> RemotePlayback {
    let mut player = player("spotify:track:1", true, false, false);
    player.position_as_of_timestamp = position;
    player.timestamp = timestamp;
    player.duration = duration;
    let mut cluster = cluster_with_player(player);
    cluster.server_timestamp_ms = server;
    view_of(&cluster, "own")
        .playback
        .expect("a view of a playing cluster has playback")
}

#[test]
fn playback_needs_a_track_uri() {
    let mut no_track = player("spotify:track:1", true, false, false);
    no_track.track = MessageField::none();
    assert!(
        view_of(&cluster_with_player(no_track), "own")
            .playback
            .is_none()
    );

    assert!(
        view_of(&cluster_with_player(player("", true, false, false)), "own")
            .playback
            .is_none()
    );
}

#[test]
fn the_measured_triples_decide_the_state() {
    let cases = [
        (true, false, false, RemoteState::Playing),
        (true, true, false, RemoteState::Paused),
        (true, true, true, RemoteState::Paused),
        (true, false, true, RemoteState::Loading),
        (false, false, false, RemoteState::Paused),
    ];

    for (playing, paused, buffering, expected) in cases {
        let cluster = cluster_with_player(player("spotify:track:1", playing, paused, buffering));
        let playback = view_of(&cluster, "own")
            .playback
            .expect("a track is loaded");
        assert_eq!(
            playback.state, expected,
            "playing={playing} paused={paused} buffering={buffering}"
        );
    }
}

#[test]
fn position_advances_on_the_server_clock() {
    assert_eq!(playback_at(10_000, 1_000, 2_500, 0).position_ms, 11_500);
}

#[test]
fn the_bare_position_field_is_ignored() {
    let mut player = player("spotify:track:1", true, false, false);
    player.position = 99_999;
    player.position_as_of_timestamp = 0;
    player.timestamp = 0;

    let playback = view_of(&cluster_with_player(player), "own")
        .playback
        .expect("a track is loaded");

    assert_eq!(playback.position_ms, 0);
}

#[test]
fn a_negative_position_reads_as_zero() {
    assert_eq!(playback_at(-5_000, 0, 0, 0).position_ms, 0);
}

#[test]
fn elapsed_needs_both_server_instants() {
    assert_eq!(playback_at(1_000, 0, 5_000, 0).position_ms, 1_000);
    assert_eq!(playback_at(1_000, -10, 5_000, 0).position_ms, 1_000);
    assert_eq!(playback_at(1_000, 1_000, 0, 0).position_ms, 1_000);
    assert_eq!(playback_at(1_000, 1_000, -10, 0).position_ms, 1_000);
}

#[test]
fn elapsed_is_zero_when_the_server_is_behind_the_player() {
    assert_eq!(playback_at(1_000, 5_000, 4_000, 0).position_ms, 1_000);
}

#[test]
fn a_three_minute_old_anchor_advances_three_minutes() {
    let playback = playback_at(0, 1_000, 1_000 + 180_000, 0);

    assert_eq!(playback.position_ms, 180_000);
}

#[test]
fn a_missing_duration_still_has_a_generous_ceiling() {
    let playback = playback_at(0, 1_000, 1_000 + MAX_EXTRAPOLATION_MS * 10, 0);

    assert_eq!(playback.position_ms, MAX_EXTRAPOLATION_MS as u32);
}

#[test]
fn a_known_duration_wins_over_the_ceiling() {
    let playback = playback_at(0, 1_000, 1_000 + MAX_EXTRAPOLATION_MS * 10, 240_000);

    assert_eq!(playback.position_ms, 240_000);
    assert_eq!(playback.duration_ms, Some(240_000));

    let long = playback_at(0, 1_000, 1_000 + MAX_EXTRAPOLATION_MS * 10, 72_000_000);
    assert_eq!(long.position_ms, 72_000_000);
}

#[test]
fn position_saturates_at_the_u32_ceiling() {
    assert_eq!(
        playback_at(i64::from(u32::MAX), 0, 0, 0).position_ms,
        u32::MAX
    );
    assert_eq!(
        playback_at(i64::from(u32::MAX) - 10, 1_000, 5_000, 0).position_ms,
        u32::MAX
    );
}

#[test]
fn a_duration_must_be_positive() {
    assert_eq!(playback_at(0, 0, 0, 0).duration_ms, None);
    assert_eq!(playback_at(0, 0, 0, -1).duration_ms, None);
    assert_eq!(playback_at(0, 0, 0, 30_000).duration_ms, Some(30_000));
}

#[test]
fn position_never_passes_the_duration() {
    assert_eq!(playback_at(50_000, 0, 0, 30_000).position_ms, 30_000);
    assert_eq!(
        playback_at(10_000, 1_000, 11_000, 15_000).position_ms,
        15_000
    );
    assert_eq!(
        playback_at(10_000, 1_000, 11_000, 15_000).duration_ms,
        Some(15_000)
    );
}
