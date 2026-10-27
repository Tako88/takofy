use librespot::protocol::player::Restrictions;
use protobuf::MessageField;

use super::super::*;
use super::test_support::*;

#[test]
fn steerable_follows_the_remote_control_reasons() {
    let mut unrestricted = player("spotify:track:1", true, false, false);
    unrestricted.restrictions = MessageField::none();
    assert!(
        view_of(&cluster_with_player(unrestricted), "own")
            .playback
            .expect("a track is loaded")
            .steerable
    );

    let mut allowed = player("spotify:track:1", true, false, false);
    allowed.restrictions = MessageField::some(Restrictions::default());
    assert!(
        view_of(&cluster_with_player(allowed), "own")
            .playback
            .expect("a track is loaded")
            .steerable
    );

    let mut refused = player("spotify:track:1", true, false, false);
    refused.restrictions = MessageField::some(Restrictions {
        disallow_remote_control_reasons: vec!["not-controllable".to_owned()],
        ..Default::default()
    });
    assert!(
        !view_of(&cluster_with_player(refused), "own")
            .playback
            .expect("a track is loaded")
            .steerable
    );
}

#[test]
fn steer_bodies_are_pinned() {
    let cases = [
        (Steer::Pause, r#"{"command":{"endpoint":"pause"}}"#),
        (Steer::Resume, r#"{"command":{"endpoint":"resume"}}"#),
        (Steer::SkipNext, r#"{"command":{"endpoint":"skip_next"}}"#),
        (Steer::SkipPrev, r#"{"command":{"endpoint":"skip_prev"}}"#),
        (
            Steer::SeekTo(60_000),
            r#"{"command":{"endpoint":"seek_to","value":60000,"position":60000}}"#,
        ),
        (
            Steer::SeekTo(0),
            r#"{"command":{"endpoint":"seek_to","value":0,"position":0}}"#,
        ),
    ];

    for (steer, expected) in cases {
        assert_eq!(steer_body(&steer), expected);
    }
}

#[test]
fn repeatable_is_true_only_for_absolute_commands() {
    assert!(repeatable(&Steer::Pause));
    assert!(repeatable(&Steer::Resume));
    assert!(repeatable(&Steer::SeekTo(60_000)));
    assert!(!repeatable(&Steer::SkipNext));
    assert!(!repeatable(&Steer::SkipPrev));
}

#[test]
fn every_body_carries_the_command_envelope() {
    let steers = [
        Steer::Pause,
        Steer::Resume,
        Steer::SkipNext,
        Steer::SkipPrev,
        Steer::SeekTo(0),
        Steer::SeekTo(60_000),
    ];

    for steer in steers {
        let body = steer_body(&steer);
        assert!(body.starts_with(r#"{"command":{"#), "{body}");
        assert!(body.ends_with("}}"), "{body}");
        assert!(!body.starts_with(r#"{"endpoint""#), "{body}");
    }
}
