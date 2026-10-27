use std::time::Duration;

use super::*;

fn transport_update(update: TransportUpdate) -> Update {
    Update::Transport(update)
}

#[test]
fn a_position_while_playing_advances_with_the_elapsed_time() {
    let anchor = Anchor {
        position_ms: 1_000,
        state: TransportState::Playing,
    };
    assert_eq!(position_at(&anchor, Duration::from_millis(500)), 1_500);
}

#[test]
fn a_position_while_paused_does_not_advance() {
    let anchor = Anchor {
        position_ms: 1_000,
        state: TransportState::Paused,
    };
    assert_eq!(position_at(&anchor, Duration::from_secs(10)), 1_000);
}

#[test]
fn a_position_while_idle_does_not_advance() {
    let anchor = Anchor {
        position_ms: 0,
        state: TransportState::Idle,
    };
    assert_eq!(position_at(&anchor, Duration::from_secs(10)), 0);
}

#[test]
fn a_position_saturates_rather_than_wrapping() {
    let anchor = Anchor {
        position_ms: u32::MAX - 5,
        state: TransportState::Playing,
    };
    assert_eq!(position_at(&anchor, Duration::from_secs(10)), u32::MAX);
}

#[test]
fn loading_maps_to_the_loading_state() {
    let mut transport = Transport::default();
    assert!(transport.apply(&transport_update(TransportUpdate::Loading)));
    assert_eq!(transport.state, TransportState::Loading);
}

#[test]
fn a_position_while_running_maps_to_playing() {
    let mut transport = Transport::default();
    assert!(
        transport.apply(&transport_update(TransportUpdate::Position {
            position_ms: 42,
            paused: false,
        }))
    );
    assert_eq!(transport.state, TransportState::Playing);
    assert_eq!(transport.position_ms, 42);
}

#[test]
fn a_position_while_paused_maps_to_paused() {
    let mut transport = Transport::default();
    assert!(
        transport.apply(&transport_update(TransportUpdate::Position {
            position_ms: 42,
            paused: true,
        }))
    );
    assert_eq!(transport.state, TransportState::Paused);
    assert_eq!(transport.position_ms, 42);
}

#[test]
fn ended_maps_to_stopped() {
    let mut transport = Transport::default();
    assert!(transport.apply(&transport_update(TransportUpdate::Ended)));
    assert_eq!(transport.state, TransportState::Stopped);
}

#[test]
fn unavailable_maps_to_unavailable() {
    let mut transport = Transport::default();
    assert!(transport.apply(&transport_update(TransportUpdate::Unavailable)));
    assert_eq!(transport.state, TransportState::Unavailable);
}

#[test]
fn a_stop_request_maps_to_stopped() {
    let mut transport = Transport::default();
    assert!(transport.apply(&transport_update(TransportUpdate::Stopped)));
    assert_eq!(transport.state, TransportState::Stopped);
}

/// A rebuild holds the track where the drop caught it: the position is
/// frozen, a held seek is dropped, and the track stays named.
#[test]
fn reconnecting_freezes_the_position_and_keeps_the_track() {
    let mut transport = Transport::default();
    transport.play(NowPlaying::named("spotify:track:a", "A", "B"));
    transport.apply(&transport_update(TransportUpdate::Duration {
        duration_ms: 100_000,
    }));
    transport.apply(&transport_update(TransportUpdate::Position {
        position_ms: 10_000,
        paused: false,
    }));
    transport.scrub_to(0.5);

    assert!(transport.apply(&transport_update(TransportUpdate::Reconnecting)));
    assert_eq!(transport.state, TransportState::Reconnecting);
    assert_eq!(transport.position_ms, 10_000);
    assert_eq!(
        transport.scrub, None,
        "a held seek is not where the track is"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("spotify:track:a")
    );
    assert_eq!(
        transport.shown_position(Duration::from_secs(10)),
        10_000,
        "a rebuild holds the position it caught"
    );
}

#[test]
fn reconnecting_names_itself() {
    assert_eq!(TransportState::Reconnecting.label(), "Reconnecting");
}

#[test]
fn a_failed_command_maps_to_failed() {
    let mut transport = Transport::default();
    assert!(transport.apply(&transport_update(TransportUpdate::Failed(
        Failure::NoAudioDevice,
    ))));
    assert_eq!(
        transport.state,
        TransportState::Failed(Failure::NoAudioDevice)
    );
}

/// The one hold decision: only playing, loading, paused and reconnecting
/// can be paused; every other state is a restart, and a row draws its glyph
/// from this. A rebuild is held rather than restarted: its load is already
/// under way.
#[test]
fn hold_separates_a_pausable_track_from_a_restart() {
    let cases = [
        (TransportState::Idle, false),
        (TransportState::Loading, true),
        (TransportState::Playing, true),
        (TransportState::Paused, true),
        (TransportState::Unavailable, false),
        (TransportState::Reconnecting, true),
        (TransportState::Stopped, false),
        (TransportState::Failed(Failure::Other), false),
    ];
    for (state, held) in cases {
        let mut transport = Transport::default();
        transport.play(NowPlaying::named("spotify:track:a", "A", "B"));
        transport.state = state;
        assert_eq!(
            transport.hold("spotify:track:a"),
            Hold { loaded: true, held },
            "{state:?}"
        );
        // A uri the transport is not on is never held, whatever the state.
        assert_eq!(
            transport.hold("spotify:track:b"),
            Hold {
                loaded: false,
                held: false
            },
            "{state:?}"
        );
    }

    let idle = Transport::default();
    assert_eq!(
        idle.hold("spotify:track:a"),
        Hold::default(),
        "nothing is loaded, so nothing is held"
    );
}

#[test]
fn a_position_reanchors_once_per_change() {
    let mut transport = Transport::default();
    let update = transport_update(TransportUpdate::Position {
        position_ms: 100,
        paused: false,
    });
    assert!(transport.apply(&update), "the first position is a change");
    assert!(!transport.apply(&update), "an identical position is not");
}

#[test]
fn a_non_transport_update_is_ignored() {
    let mut transport = Transport::default();
    assert!(!transport.apply(&Update::SignedIn));
    assert_eq!(transport.state, TransportState::Idle);
}

/// A transport that is playing a track with a length, which is the only state
/// a drag can happen in.
fn seekable_transport() -> Transport {
    let mut transport = Transport::default();
    transport.apply(&transport_update(TransportUpdate::Duration {
        duration_ms: 100_000,
    }));
    transport.apply(&transport_update(TransportUpdate::Position {
        position_ms: 10_000,
        paused: false,
    }));
    transport
}

#[test]
fn a_drag_shows_where_the_pointer_is_not_where_the_track_is() {
    let mut transport = seekable_transport();
    transport.scrub_to(0.75);
    assert_eq!(transport.shown_position(Duration::ZERO), 75_000);
}

#[test]
fn a_drag_asks_for_nothing_until_it_ends() {
    let mut transport = seekable_transport();
    transport.scrub_to(0.25);
    transport.scrub_to(0.5);
    assert_eq!(
        transport.scrub_end(),
        Some(Command::Seek {
            position_ms: 50_000
        }),
        "the release asks for where it was let go",
    );
    assert_eq!(transport.scrub_end(), None, "one drag is one jump");
}

#[test]
fn a_drag_holds_its_place_until_the_player_reports_the_seek() {
    let mut transport = seekable_transport();
    transport.scrub_to(0.9);
    transport.scrub_end();
    assert_eq!(
        transport.shown_position(Duration::from_secs(5)),
        90_000,
        "the bar must not fall back to where the track still is",
    );
    transport.apply(&transport_update(TransportUpdate::Seeked {
        position_ms: 1_000,
    }));
    assert_eq!(
        transport.shown_position(Duration::ZERO),
        1_000,
        "and the player's answer wins once it arrives",
    );
}

#[test]
fn a_drag_on_a_track_with_no_length_does_nothing() {
    let mut transport = Transport::default();
    transport.scrub_to(0.5);
    assert_eq!(transport.shown_position(Duration::ZERO), 0);
    assert_eq!(transport.scrub_end(), None);
}

#[test]
fn a_drag_past_either_end_clamps_to_the_track() {
    let mut transport = seekable_transport();
    transport.scrub_to(1.5);
    assert_eq!(
        transport.scrub_end(),
        Some(Command::Seek {
            position_ms: 99_999
        }),
        "the last millisecond, never the length itself",
    );
    transport.scrub_to(-0.5);
    assert_eq!(
        transport.scrub_end(),
        Some(Command::Seek { position_ms: 0 }),
    );
}

#[test]
fn playing_another_track_drops_a_drag() {
    let mut transport = seekable_transport();
    transport.scrub_to(0.5);
    transport.play(NowPlaying::named("spotify:track:y", "Another", "Someone"));
    assert_eq!(transport.shown_position(Duration::ZERO), 0);
}

fn entry(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: "A Title".to_owned(),
        artists: "A Band".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

#[test]
fn play_names_the_track_and_asks_for_it() {
    let mut transport = Transport::default();
    assert_eq!(
        transport.play(NowPlaying::named("spotify:track:x", "A Title", "A Band")),
        Command::Play {
            uri: "spotify:track:x".to_owned()
        }
    );
    assert_eq!(
        transport.now_playing,
        Some(NowPlaying::named("spotify:track:x", "A Title", "A Band"))
    );
    assert_eq!(transport.state, TransportState::Loading);
}

#[test]
fn play_names_the_album_and_first_artist_uris() {
    let mut transport = Transport::default();
    let mut entry = entry("spotify:track:x");
    entry.album_uri = Some("spotify:album:a".to_owned());
    entry.artist_uris = vec![
        "spotify:artist:first".to_owned(),
        "spotify:artist:second".to_owned(),
    ];
    transport.play(NowPlaying::from(&entry));

    let now = transport.now_playing.expect("a named track");
    assert_eq!(now.album_uri.as_deref(), Some("spotify:album:a"));
    assert_eq!(
        now.artist_uri.as_deref(),
        Some("spotify:artist:first"),
        "a joined credit is one link, and it is the first artist's"
    );
}

#[test]
fn play_without_uris_names_none() {
    let mut transport = Transport::default();
    transport.play(NowPlaying::named("spotify:track:x", "A Title", "A Band"));

    let now = transport.now_playing.expect("a named track");
    assert_eq!(now.album_uri, None);
    assert_eq!(now.artist_uri, None);
}

#[test]
fn landed_carries_the_uris() {
    let mut transport = Transport::default();
    let mut entry = entry("spotify:track:x");
    entry.album_uri = Some("spotify:album:a".to_owned());
    entry.artist_uris = vec!["spotify:artist:first".to_owned()];

    transport.landed(&entry, 1_000, false);

    let now = transport.now_playing.expect("the landed track is named");
    assert_eq!(now.album_uri.as_deref(), Some("spotify:album:a"));
    assert_eq!(now.artist_uri.as_deref(), Some("spotify:artist:first"));
}

#[test]
fn play_replaces_the_artist_with_the_new_tracks() {
    let mut transport = Transport::default();
    transport.play(NowPlaying::named("spotify:track:x", "A Title", "A Band"));
    transport.play(NowPlaying::named(
        "spotify:track:y",
        "Another",
        "Another Band",
    ));

    let now = transport
        .now_playing
        .as_ref()
        .expect("the second track is named");
    assert_eq!(now.artists, "Another Band");
    assert_eq!(now.title, "Another");

    // An entry the metadata gave no artist for is one with an empty line, not
    // the previous track's artist left standing.
    transport.play(NowPlaying::named("spotify:track:z", "Third", ""));
    let now = transport
        .now_playing
        .as_ref()
        .expect("the third track is named");
    assert!(now.artists.is_empty());
}

#[test]
fn duration_sets_the_length_and_play_clears_it() {
    let mut transport = Transport::default();
    assert!(
        !transport.apply(&transport_update(TransportUpdate::Duration {
            duration_ms: 222_000,
        })),
        "a length does not move the anchor"
    );
    assert_eq!(transport.duration_ms, Some(222_000));

    transport.play(NowPlaying::named("spotify:track:x", "A Title", "A Band"));
    assert_eq!(transport.duration_ms, None);
}

#[test]
fn seeked_moves_the_position_and_keeps_the_state() {
    let mut transport = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };
    assert!(transport.apply(&transport_update(TransportUpdate::Seeked {
        position_ms: 41_000,
    })));
    assert_eq!(transport.position_ms, 41_000);
    assert_eq!(transport.state, TransportState::Playing);
}

#[test]
fn seeked_does_not_reanchor_when_the_position_is_unchanged() {
    let mut transport = Transport {
        state: TransportState::Paused,
        position_ms: 41_000,
        ..Transport::default()
    };
    assert!(!transport.apply(&transport_update(TransportUpdate::Seeked {
        position_ms: 41_000,
    })));
}

#[test]
fn seekable_is_playing_or_paused_with_a_non_zero_length() {
    for (state, duration) in [
        (TransportState::Playing, Some(1)),
        (TransportState::Paused, Some(1)),
        (TransportState::Loading, Some(1)),
        (TransportState::Stopped, Some(1)),
        (TransportState::Playing, None),
        (TransportState::Playing, Some(0)),
    ] {
        let transport = Transport {
            state,
            duration_ms: duration,
            ..Transport::default()
        };
        let expected = matches!(state, TransportState::Playing | TransportState::Paused)
            && duration == Some(1);
        assert_eq!(transport.seekable(), expected, "{state:?} {duration:?}");
    }
}

#[test]
fn seek_to_fraction_clamps_and_refuses_when_not_seekable() {
    let transport = Transport {
        state: TransportState::Playing,
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert_eq!(
        transport.seek_to_fraction(0.25),
        Some(Command::Seek { position_ms: 2_500 })
    );
    assert_eq!(
        transport.seek_to_fraction(1.5),
        Some(Command::Seek { position_ms: 9_999 }),
        "the last millisecond is the furthest a seek may land"
    );
    assert_eq!(
        transport.seek_to_fraction(-1.0),
        Some(Command::Seek { position_ms: 0 })
    );

    let paused = Transport {
        state: TransportState::Paused,
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert!(paused.seek_to_fraction(0.5).is_some());

    let idle = Transport {
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert_eq!(idle.seek_to_fraction(0.5), None);
    let zero = Transport {
        state: TransportState::Playing,
        duration_ms: Some(0),
        ..Transport::default()
    };
    assert_eq!(zero.seek_to_fraction(0.5), None);
}

#[test]
fn seek_by_saturates_at_both_ends() {
    let transport = Transport {
        state: TransportState::Playing,
        position_ms: 3_000,
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert_eq!(
        transport.seek_by(-100),
        Some(Command::Seek { position_ms: 2_900 })
    );
    assert_eq!(
        transport.seek_by(-5_000),
        Some(Command::Seek { position_ms: 0 }),
        "a seek before the start saturates at zero"
    );
    assert_eq!(
        transport.seek_by(20_000),
        Some(Command::Seek { position_ms: 9_999 }),
        "a seek past the end stops on the last millisecond"
    );
    assert_eq!(Transport::default().seek_by(1), None);
}

#[test]
fn shown_position_is_clamped_to_the_duration() {
    let transport = Transport {
        state: TransportState::Playing,
        position_ms: 9_000,
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert_eq!(transport.shown_position(Duration::from_secs(5)), 10_000);

    let known = Transport {
        state: TransportState::Paused,
        position_ms: 4_000,
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert_eq!(known.shown_position(Duration::from_secs(5)), 4_000);
}

#[test]
fn fraction_is_none_without_a_non_zero_duration() {
    let transport = Transport {
        duration_ms: Some(10_000),
        ..Transport::default()
    };
    assert_eq!(transport.fraction(2_500), Some(0.25));
    assert_eq!(transport.fraction(20_000), Some(1.0));
    assert_eq!(Transport::default().fraction(2_500), None);
    let zero = Transport {
        duration_ms: Some(0),
        ..Transport::default()
    };
    assert_eq!(zero.fraction(0), None);
}

#[test]
fn a_restored_level_is_one_command_on_the_first_take() {
    let mut transport = Transport {
        volume: Volume::restored(35, false),
        ..Transport::default()
    };
    assert_eq!(
        transport.take_volume(),
        Some(Command::Volume { percent: 35 }),
        "a restored level must reach the player, not stay a slider position"
    );
    assert_eq!(transport.take_volume(), None, "one change is one command");
}

#[test]
fn a_restored_mute_takes_volume_to_zero() {
    let mut transport = Transport {
        volume: Volume::restored(35, true),
        ..Transport::default()
    };
    assert_eq!(
        transport.take_volume(),
        Some(Command::Volume { percent: 0 }),
        "a restored mute owes the level that is heard, which is silence"
    );
}

#[test]
fn a_default_transport_owes_no_volume_command() {
    let mut transport = Transport::default();
    assert_eq!(
        transport.take_volume(),
        None,
        "the player already starts at full, so there is nothing to send"
    );
}

#[test]
fn every_state_has_a_label() {
    for state in [
        TransportState::Loading,
        TransportState::Playing,
        TransportState::Paused,
        TransportState::Unavailable,
        TransportState::Stopped,
        TransportState::Failed(Failure::NoAudioDevice),
    ] {
        assert!(!state.label().trim().is_empty(), "{state:?} has no label");
    }
    assert!(TransportState::Idle.label().is_empty());
}

#[test]
fn the_redraw_interval_is_non_zero() {
    assert!(REDRAW > Duration::ZERO);
}
