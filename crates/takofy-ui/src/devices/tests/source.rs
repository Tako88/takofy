use super::super::*;
use super::test_support::*;

#[test]
fn the_source_rule_picks_the_remote_only_while_the_local_transport_is_silent() {
    let now = base();
    struct Case {
        holder: Option<&'static str>,
        playback: bool,
        local: TransportState,
        source: Source,
    }
    let cases = [
        Case {
            holder: Some("a"),
            playback: true,
            local: TransportState::Playing,
            source: Source::Local,
        },
        Case {
            holder: Some("a"),
            playback: true,
            local: TransportState::Loading,
            source: Source::Local,
        },
        Case {
            holder: Some("a"),
            playback: true,
            local: TransportState::Paused,
            source: Source::Remote,
        },
        Case {
            holder: Some("a"),
            playback: true,
            local: TransportState::Idle,
            source: Source::Remote,
        },
        Case {
            holder: Some("a"),
            playback: true,
            local: TransportState::Stopped,
            source: Source::Remote,
        },
        Case {
            holder: Some("a"),
            playback: true,
            local: TransportState::Failed(Failure::Other),
            source: Source::Remote,
        },
        Case {
            holder: Some("a"),
            playback: false,
            local: TransportState::Idle,
            source: Source::Local,
        },
        Case {
            holder: None,
            playback: true,
            local: TransportState::Idle,
            source: Source::Local,
        },
        Case {
            holder: None,
            playback: false,
            local: TransportState::Idle,
            source: Source::Local,
        },
    ];
    for case in cases {
        let mut devices = Devices::default();
        devices.apply(&roster(vec![phone()], case.holder), now);
        if case.playback {
            devices.apply(
                &playing(Some(playback(
                    RemoteState::Playing,
                    "spotify:track:a",
                    Some(10_000),
                ))),
                now,
            );
        }
        let local = Transport {
            state: case.local,
            ..Transport::default()
        };
        assert_eq!(
            devices.source(&local),
            case.source,
            "holder {:?}, playback {}, local {:?}",
            case.holder,
            case.playback,
            case.local
        );
    }
}

#[test]
fn holding_here_makes_the_source_local_even_when_the_local_transport_is_paused() {
    let now = base();
    let local = Transport {
        state: TransportState::Paused,
        ..Transport::default()
    };

    let mut devices = watching(now);
    devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000),
        ))),
        now,
    );
    assert_eq!(
        devices.source(&local),
        Source::Remote,
        "a still window that does not hold the sound shows the holder"
    );

    assert!(devices.apply(&roster_of(vec![phone(), laptop()], Some("a"), true), now));
    assert_eq!(
        devices.source(&local),
        Source::Local,
        "a landed transfer that settled paused is still ours"
    );
}

#[test]
fn reading_the_source_twice_without_an_update_answers_the_same() {
    let now = base();
    let devices = remote(now);
    let local = Transport::default();
    assert_eq!(devices.source(&local), devices.source(&local));
    assert_eq!(devices.source(&local), Source::Remote);
}
