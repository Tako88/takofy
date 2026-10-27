use librespot::protocol::context::Context;
use librespot::protocol::context_page::ContextPage;
use librespot::protocol::context_player_options::ContextPlayerOptions;
use librespot::protocol::context_track::ContextTrack;
use librespot::protocol::playback::Playback;
use librespot::protocol::queue::Queue;
use librespot::protocol::session::Session;
use protobuf::MessageField;
use takofy_model::Status;

use super::*;

/// The wire's ids are 22 base62 characters, so a fixture has to be one.
const TRACK_1: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";
const TRACK_2: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";
const TRACK_3: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ3";
const TRACK_7: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ7";
const TRACK_9: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ9";
const QUEUE_1: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQa";
const QUEUE_2: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQb";
const EPISODE: &str = "spotify:episode:5CfCWKI5pZ28U0uOJ0ZZAK";
const ARTIST: &str = "spotify:artist:0OdUWJ0sBjDrqHygGUXeCF";
const ALBUM: &str = "spotify:album:1DFixLWuPkv3KT3TnV35m3";
const SHOW: &str = "spotify:show:5CfCWKI5pZ28U0uOJ0ZZAK";
const PLAYLIST: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";
const COLLECTION: &str = "spotify:user:someone:collection";
const STATION: &str = "spotify:station:artist:0OdUWJ0sBjDrqHygGUXeCF";
const SEARCH: &str = "spotify:search:radiohead";
const LOCAL: &str = "spotify:local:artist:album:title:200";

/// The context and current track a measured transfer carried.
fn spec() -> Spec {
    Spec {
        context: Some(PLAYLIST),
        pages: vec![vec![TRACK_1, TRACK_2, TRACK_3]],
        queue: Vec::new(),
        playing_queue: false,
        current: Some(TRACK_2),
        associated: None,
        paused: true,
        position: Some(0),
        timestamp: Some(0),
        shuffle: false,
        repeat_context: false,
        repeat_track: false,
    }
}

#[derive(Debug, Clone)]
struct Spec {
    context: Option<&'static str>,
    pages: Vec<Vec<&'static str>>,
    queue: Vec<&'static str>,
    playing_queue: bool,
    current: Option<&'static str>,
    associated: Option<&'static str>,
    paused: bool,
    position: Option<i32>,
    timestamp: Option<i64>,
    shuffle: bool,
    repeat_context: bool,
    repeat_track: bool,
}

fn track(uri: &str) -> ContextTrack {
    ContextTrack {
        uri: Some(uri.to_owned()),
        ..Default::default()
    }
}

/// The measured hand-over: the phone's pages carry the row uids and the
/// session names the row it was on, with no current track anywhere.
fn uid_state(rows: &[(&str, &str)], current_uid: Option<&str>, queue: &[&str]) -> TransferState {
    TransferState {
        playback: MessageField::some(Playback {
            is_paused: Some(false),
            position_as_of_timestamp: Some(29_763),
            timestamp: Some(0),
            ..Default::default()
        }),
        current_session: MessageField::some(Session {
            context: MessageField::some(Context {
                uri: Some(ALBUM.to_owned()),
                pages: vec![ContextPage {
                    tracks: rows
                        .iter()
                        .map(|(uid, uri)| ContextTrack {
                            uid: Some((*uid).to_owned()),
                            uri: Some((*uri).to_owned()),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            current_uid: current_uid.map(str::to_owned),
            ..Default::default()
        }),
        queue: MessageField::some(Queue {
            tracks: queue.iter().map(|uri| track(uri)).collect(),
            is_playing_queue: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A row the phone sends with only its raw id, the way a real hand-over does.
fn id_only_track(gid: u8) -> ContextTrack {
    ContextTrack {
        gid: Some(vec![gid; 16]),
        uid: Some(format!("uid-{gid}")),
        ..Default::default()
    }
}

#[test]
fn a_row_that_carries_only_its_id_still_names_the_track() {
    let mut state = uid_state(&[("uid-1", TRACK_1)], Some("uid-1"), &[]);
    if let Some(playback) = state.playback.as_mut() {
        playback.current_track = MessageField::some(id_only_track(7));
    }
    if let Some(page) = state
        .current_session
        .as_mut()
        .and_then(|session| session.context.as_mut())
        .and_then(|context| context.pages.first_mut())
    {
        page.tracks = vec![id_only_track(7)];
    }

    let plan = plan(&state).expect("the id names a track");
    assert_eq!(plan.current, "spotify:track:0dg9MKoV2L6aKsP3wSpXy7");
    assert_eq!(plan.pages, vec![plan.current.clone()]);
}

#[test]
fn an_id_only_row_is_found_by_its_uid_too() {
    let mut state = uid_state(&[("uid-1", TRACK_1)], Some("uid-9"), &[]);
    if let Some(page) = state
        .current_session
        .as_mut()
        .and_then(|session| session.context.as_mut())
        .and_then(|context| context.pages.first_mut())
    {
        page.tracks = vec![id_only_track(9)];
    }

    let plan = plan(&state).expect("the uid finds the id-only row");
    assert_eq!(plan.current, "spotify:track:0h33IXETCYyvxaudoyxnq9");
}

#[test]
fn the_current_uid_names_the_row_when_the_current_track_is_missing() {
    let plan = plan(&uid_state(
        &[("uid-1", TRACK_1), ("uid-2", TRACK_2)],
        Some("uid-2"),
        &[],
    ))
    .expect("the uid names a playable row");

    assert_eq!(plan.current, TRACK_2);
    assert_eq!(plan.source, ContextSource::Pages);
    assert_eq!(plan.position_ms, Some(29_763));
}

#[test]
fn a_uid_that_matches_no_row_still_refuses() {
    assert_eq!(
        plan(&uid_state(&[("uid-1", TRACK_1)], Some("uid-9"), &[])),
        Err(Refusal::NoTrack)
    );
    assert_eq!(
        plan(&uid_state(&[("uid-1", TRACK_1)], None, &[])),
        Err(Refusal::NoTrack),
        "a payload with neither a track nor a uid has no row"
    );
}

#[test]
fn a_named_current_track_still_wins_over_the_uid() {
    let mut state = state(Spec {
        current: Some(TRACK_2),
        ..spec()
    });
    if let Some(session) = state.current_session.as_mut() {
        session.current_uid = Some("uid-3".to_owned());
    }

    assert_eq!(
        plan(&state).expect("the track names the row").current,
        TRACK_2
    );
}

#[test]
fn the_queue_head_still_wins_when_the_queue_is_playing() {
    let mut state = uid_state(&[("uid-1", TRACK_1)], Some("uid-1"), &[TRACK_3]);
    if let Some(queue) = state.queue.as_mut() {
        queue.is_playing_queue = Some(true);
    }

    assert_eq!(
        plan(&state).expect("the queue names the row").current,
        TRACK_3
    );
}

fn state(spec: Spec) -> TransferState {
    TransferState {
        options: MessageField::some(ContextPlayerOptions {
            shuffling_context: Some(spec.shuffle),
            repeating_context: Some(spec.repeat_context),
            repeating_track: Some(spec.repeat_track),
            ..Default::default()
        }),
        playback: MessageField::some(Playback {
            current_track: spec.current.map(track).into(),
            associated_current_track: spec.associated.map(track).into(),
            is_paused: Some(spec.paused),
            position_as_of_timestamp: spec.position,
            timestamp: spec.timestamp,
            ..Default::default()
        }),
        current_session: MessageField::some(Session {
            context: MessageField::some(Context {
                uri: spec.context.map(str::to_owned),
                pages: spec
                    .pages
                    .iter()
                    .map(|uris| ContextPage {
                        tracks: uris.iter().map(|uri| track(uri)).collect(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        queue: MessageField::some(Queue {
            tracks: spec.queue.iter().map(|uri| track(uri)).collect(),
            is_playing_queue: Some(spec.playing_queue),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn plan_of(spec: Spec) -> TransferPlan {
    plan(&state(spec)).expect("the fixture is playable")
}

/// A paused playlist plan whose current track is `current`.
fn paused_plan(current: &'static str) -> TransferPlan {
    plan_of(Spec {
        pages: Vec::new(),
        current: Some(current),
        position: Some(1_000),
        ..spec()
    })
}

fn uris(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|uri| (*uri).to_owned()).collect()
}

#[test]
fn playlist_album_show_and_collection_are_resolved() {
    for uri in [PLAYLIST, ALBUM, SHOW, COLLECTION] {
        let plan = plan_of(Spec {
            context: Some(uri),
            pages: Vec::new(),
            ..spec()
        });

        assert_eq!(
            plan.source,
            ContextSource::Resolve {
                uri: uri.to_owned()
            },
            "{uri}"
        );
        assert_eq!(plan.context.as_deref(), Some(uri), "{uri}");
    }
}

#[test]
fn the_phones_pages_are_used_before_any_read() {
    let pages: Vec<&str> = (0..10).map(|_| TRACK_1).collect();
    let plan = plan_of(Spec {
        context: Some(ALBUM),
        pages: vec![pages],
        current: Some(TRACK_1),
        ..spec()
    });

    assert_eq!(plan.source, ContextSource::Pages);
    assert_eq!(plan.pages.len(), 10, "the measured transfer carried ten");
    assert_eq!(plan.resolve_uri(), None, "the pages cost no read");
    assert_eq!(plan.context.as_deref(), Some(ALBUM));
}

#[test]
fn an_artist_context_is_refused_even_with_a_playable_track() {
    let refusal = plan(&state(Spec {
        context: Some(ARTIST),
        current: Some(TRACK_1),
        ..spec()
    }));

    assert_eq!(
        refusal,
        Err(Refusal::UnsupportedContext(ContextKind::Artist))
    );
}

#[test]
fn a_station_a_search_and_other_kinds_are_refused() {
    let cases = [
        (STATION, ContextKind::Station),
        (SEARCH, ContextKind::Search),
        (TRACK_1, ContextKind::Other),
        ("https://example.test/list", ContextKind::Other),
    ];

    for (uri, kind) in cases {
        assert_eq!(
            plan(&state(Spec {
                context: Some(uri),
                ..spec()
            })),
            Err(Refusal::UnsupportedContext(kind)),
            "{uri}"
        );
    }
}

#[test]
fn a_local_file_context_and_a_local_current_track_are_refused() {
    assert_eq!(
        plan(&state(Spec {
            context: Some(LOCAL),
            ..spec()
        })),
        Err(Refusal::UnsupportedContext(ContextKind::Local))
    );
    assert_eq!(
        plan(&state(Spec {
            current: Some(LOCAL),
            ..spec()
        })),
        Err(Refusal::NotPlayable)
    );
}

#[test]
fn no_context_dash_and_empty_use_pages_when_there_are_any() {
    for context in [None, Some(""), Some("-")] {
        let plan = plan_of(Spec {
            context,
            pages: vec![vec![TRACK_1, TRACK_2]],
            current: Some(TRACK_1),
            ..spec()
        });

        assert_eq!(plan.source, ContextSource::Pages, "{context:?}");
        assert_eq!(plan.context, None, "{context:?}");
    }
}

#[test]
fn pages_that_carry_no_tracks_fall_through_to_the_read() {
    let plan = plan_of(Spec {
        context: Some(ALBUM),
        pages: vec![Vec::new()],
        ..spec()
    });

    assert_eq!(
        plan.source,
        ContextSource::Resolve {
            uri: ALBUM.to_owned()
        }
    );
}

#[test]
fn no_context_and_no_pages_lands_on_the_one_track() {
    let plan = plan_of(Spec {
        context: None,
        pages: Vec::new(),
        ..spec()
    });

    assert_eq!(plan.source, ContextSource::SingleTrack);
    assert_eq!(plan.current, TRACK_2);
    assert_eq!(plan.context, None);
}

#[test]
fn no_track_is_refused() {
    assert_eq!(
        plan(&state(Spec {
            current: None,
            ..spec()
        })),
        Err(Refusal::NoTrack)
    );
    assert_eq!(
        plan(&state(Spec {
            current: Some(""),
            ..spec()
        })),
        Err(Refusal::NoTrack)
    );
}

#[test]
fn a_non_track_non_episode_current_is_refused() {
    assert_eq!(
        plan(&state(Spec {
            current: Some(ARTIST),
            ..spec()
        })),
        Err(Refusal::NotPlayable)
    );

    let episode = plan(&state(Spec {
        current: Some(EPISODE),
        ..spec()
    }))
    .expect("an episode is playable");
    assert_eq!(episode.current, EPISODE);
}

#[test]
fn every_refusal_has_its_own_word() {
    let words = [
        Refusal::NoData,
        Refusal::UnsupportedContext(ContextKind::Artist),
        Refusal::NoTrack,
        Refusal::NotPlayable,
    ]
    .map(|refusal| refusal.word());
    let mut distinct = words.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), words.len(), "{words:?}");
}

#[test]
fn an_unsupported_refusal_names_the_context_that_could_not_be_adopted() {
    assert_eq!(
        Refusal::UnsupportedContext(ContextKind::Artist).word(),
        "artist",
        "the kind is what says why"
    );
    assert_eq!(
        Refusal::UnsupportedContext(ContextKind::Station).word(),
        "station"
    );
}

#[test]
fn no_data_is_refused() {
    assert_eq!(plan(&TransferState::default()), Err(Refusal::NoData));
}

#[test]
fn playing_the_queue_takes_the_queue_head_as_current() {
    let plan = plan_of(Spec {
        playing_queue: true,
        queue: vec![QUEUE_1, QUEUE_2],
        current: Some(TRACK_9),
        ..spec()
    });

    assert!(plan.from_queue);
    assert_eq!(plan.current, QUEUE_1);
    assert_eq!(plan.queue, uris(&[QUEUE_1, QUEUE_2]));
}

#[test]
fn the_flags_and_options_are_carried() {
    let plan = plan_of(Spec {
        paused: true,
        position: Some(42_000),
        timestamp: Some(1_700_000_000_000),
        shuffle: true,
        repeat_context: true,
        repeat_track: true,
        ..spec()
    });

    assert!(plan.paused);
    assert_eq!(plan.position_ms, Some(42_000));
    assert_eq!(plan.timestamp_ms, Some(1_700_000_000_000));
    assert_eq!(plan.shuffle, Some(true));
    assert_eq!(plan.repeat_context, Some(true));
    assert_eq!(plan.repeat_track, Some(true));
}

#[test]
fn lands_on_the_current_track_in_the_resolved_list() {
    let landing = land(
        &paused_plan(TRACK_2),
        Resolution::Resolved(uris(&[TRACK_1, TRACK_2, TRACK_3])),
        1_000,
    );

    assert_eq!(landing.rows, uris(&[TRACK_1, TRACK_2, TRACK_3]));
    assert_eq!(landing.index, 1);
    assert_eq!(landing.position_ms, 1_000);
    assert!(!landing.playing, "the fixture is paused");
    assert_eq!(landing.epoch, 0, "activation stamps the epoch");
    assert_eq!(landing.context.as_deref(), Some(PLAYLIST));

    let report = landing.report();
    assert_eq!(
        report.track.as_ref().map(|track| track.uri.as_str()),
        Some(TRACK_2)
    );
    assert_eq!(report.prev.len(), 1);
    assert_eq!(report.next.len(), 1);
}

#[test]
fn a_repeated_track_lands_on_its_first_row() {
    let landing = land(
        &paused_plan(TRACK_2),
        Resolution::Resolved(uris(&[TRACK_2, TRACK_1, TRACK_2])),
        1_000,
    );

    assert_eq!(landing.index, 0);
    assert_eq!(landing.rows.len(), 3, "the later copy stays a row");
}

#[test]
fn a_current_track_not_in_the_resolved_list_falls_back_to_the_one_track() {
    let landing = land(
        &paused_plan(TRACK_7),
        Resolution::Resolved(uris(&[TRACK_1, TRACK_2])),
        1_000,
    );

    assert_eq!(landing.rows, uris(&[TRACK_7]));
    assert_eq!(landing.index, 0);
}

#[test]
fn a_failed_resolution_falls_back_to_the_one_track() {
    let landing = land(&paused_plan(TRACK_7), Resolution::Failed, 1_000);

    assert_eq!(landing.rows, uris(&[TRACK_7]));
    assert_eq!(landing.index, 0);
}

#[test]
fn a_failed_read_keeps_the_phones_pages() {
    let plan = plan_of(Spec {
        pages: vec![vec![TRACK_1, TRACK_2, TRACK_3]],
        ..spec()
    });
    let landing = land(&plan, Resolution::Failed, 1_000);

    assert_eq!(landing.rows, uris(&[TRACK_1, TRACK_2, TRACK_3]));
    assert_eq!(landing.index, 1);
}

#[test]
fn queue_tracks_follow_the_current_row() {
    let plan = plan_of(Spec {
        queue: vec![QUEUE_1, QUEUE_2],
        current: Some(TRACK_2),
        ..spec()
    });
    let landing = land(
        &plan,
        Resolution::Resolved(uris(&[TRACK_1, TRACK_2, TRACK_3])),
        1_000,
    );

    assert_eq!(
        landing.rows,
        uris(&[TRACK_1, TRACK_2, QUEUE_1, QUEUE_2, TRACK_3])
    );
    assert_eq!(landing.index, 1);
}

#[test]
fn playing_the_queue_resumes_the_context_after_the_associated_track() {
    let plan = plan_of(Spec {
        playing_queue: true,
        queue: vec![QUEUE_1, QUEUE_2],
        associated: Some(TRACK_2),
        ..spec()
    });
    let landing = land(
        &plan,
        Resolution::Resolved(uris(&[TRACK_1, TRACK_2, TRACK_3])),
        1_000,
    );

    assert_eq!(landing.rows, uris(&[QUEUE_1, QUEUE_2, TRACK_3]));
    assert_eq!(landing.index, 0);
}

#[test]
fn playing_the_queue_without_an_associated_track_has_only_the_queue() {
    let plan = plan_of(Spec {
        playing_queue: true,
        queue: vec![QUEUE_1, QUEUE_2],
        associated: None,
        ..spec()
    });
    let landing = land(
        &plan,
        Resolution::Resolved(uris(&[TRACK_1, TRACK_2])),
        1_000,
    );

    assert_eq!(landing.rows, uris(&[QUEUE_1, QUEUE_2]));
    assert_eq!(landing.index, 0);
}

#[test]
fn the_landing_report_is_loading_when_playing_and_paused_otherwise() {
    let landing = land(&paused_plan(TRACK_7), Resolution::Failed, 0);

    assert_eq!(landing.report().status, Status::Paused);
    assert_eq!(
        landing
            .report()
            .track
            .as_ref()
            .map(|track| track.uri.as_str()),
        Some(TRACK_7)
    );
    assert!(landing.report().prev.is_empty());
    assert!(landing.report().next.is_empty());

    let mut played = landing;
    played.playing = true;
    assert_eq!(
        played.report().status,
        Status::Loading,
        "an accepted track has not started yet"
    );
}

#[test]
fn a_playing_transfer_adds_the_elapsed_time() {
    assert_eq!(
        landing_position(Some(10_000), Some(1_000), 4_500, false),
        13_500
    );
}

#[test]
fn a_playing_transfer_at_zero_still_adds_elapsed_time() {
    assert_eq!(landing_position(Some(0), Some(1_000), 3_000, false), 2_000);
}

#[test]
fn a_paused_transfer_keeps_its_position() {
    assert_eq!(
        landing_position(Some(10_000), Some(1_000), 9_000, true),
        10_000
    );
}

#[test]
fn a_snapshot_aged_eight_seconds_still_adds_its_elapsed() {
    // The measured case: the phone pushes on an event, so its snapshot was
    // already 8.4 s old when the transfer arrived, and 11.6 s old when it
    // was retried. Both are ordinary elapsed music.
    assert_eq!(
        landing_position(Some(30_000), Some(1_000), 9_405, false),
        38_405
    );
    assert_eq!(
        landing_position(Some(30_000), Some(1_000), 12_555, false),
        41_555
    );
}

#[test]
fn an_age_beyond_the_state_bound_adds_nothing() {
    let base = Some(30_000);
    let timestamp = Some(1_000);

    assert_eq!(
        landing_position(base, timestamp, 1_000 + STATE_AGE_MAX, false),
        30_000 + STATE_AGE_MAX as u32,
        "the bound itself is still elapsed music"
    );
    assert_eq!(
        landing_position(base, timestamp, 1_001 + STATE_AGE_MAX, false),
        30_000
    );
}

#[test]
fn negative_elapsed_adds_nothing() {
    assert_eq!(
        landing_position(Some(10_000), Some(2_500), 1_000, false),
        10_000
    );
}

#[test]
fn a_missing_position_lands_at_zero() {
    assert_eq!(landing_position(None, Some(1_000), 9_000, false), 0);
    assert_eq!(landing_position(None, None, 9_000, true), 0);
}

#[test]
fn the_position_saturates_at_u32_max() {
    assert_eq!(
        landing_position(Some(u32::MAX - 1), Some(1_000), 30_000, false),
        u32::MAX
    );
}
