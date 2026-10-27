//! The summaries a log line reads: counts and kinds, never a uri.

use librespot::core::dealer::protocol::{PlayCommand, PlayOptions, SetQueueCommand};
use librespot::protocol::context::Context;
use librespot::protocol::playback::Playback;
use librespot::protocol::player::PlayOrigin;
use librespot::protocol::queue::Queue;
use librespot::protocol::session::Session;
use librespot::protocol::transfer_state::TransferState;
use protobuf::MessageField;

use super::super::*;
use super::test_support::*;

#[test]
fn a_queue_summary_counts_and_never_names() {
    let summary = summarise_queue(&SetQueueCommand {
        next_tracks: vec![provided(TRACK_1), provided(TRACK_2)],
        prev_tracks: vec![provided(TRACK_3)],
        queue_revision: "rev-7".to_owned(),
        logging_params: logging(),
    });
    let shown = summary.to_string();

    assert_eq!(summary.prev, 1);
    assert_eq!(summary.next, 2);
    assert!(shown.contains("prev 1 rows"), "{shown}");
    assert!(shown.contains("next 2 rows"), "{shown}");
    assert!(!shown.contains("spotify:"), "{shown}");
}

#[test]
fn a_play_summary_names_presence_only() {
    let summary = summarise_play(&PlayCommand {
        context: Context {
            uri: Some(PLAYLIST.to_owned()),
            pages: vec![page(&[TRACK_1])],
            ..Default::default()
        },
        play_origin: PlayOrigin::default(),
        options: PlayOptions {
            seek_to: Some(42_000),
            ..skip_to(Some(TRACK_1), Some(0), Some("uid-1"))
        },
        logging_params: logging(),
    });
    let shown = summary.to_string();

    assert_eq!(summary.context, ContextKind::Playlist);
    assert_eq!((summary.pages, summary.tracks), (1, 1));
    assert!(summary.uri && summary.index && summary.uid);
    assert!(summary.seek && !summary.paused);
    assert!(shown.contains("skip uri named"), "{shown}");
    assert!(shown.contains("paused absent"), "{shown}");
    assert!(!shown.contains("spotify:"), "{shown}");
}

#[test]
fn a_summary_counts_and_never_names() {
    let state = TransferState {
        current_session: MessageField::some(Session {
            context: MessageField::some(Context {
                uri: Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned()),
                pages: vec![
                    page(&["spotify:track:1", "spotify:track:2"]),
                    page(&["spotify:track:3"]),
                ],
                ..Default::default()
            }),
            ..Default::default()
        }),
        playback: MessageField::some(Playback {
            timestamp: Some(1_000),
            is_paused: Some(true),
            ..Default::default()
        }),
        queue: MessageField::some(Queue {
            tracks: vec![track("spotify:track:4")],
            is_playing_queue: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };

    let summary = summarise(&state, 2_500);
    let shown = summary.to_string();

    assert_eq!(summary.context, ContextKind::Playlist);
    assert_eq!(summary.pages, 2);
    assert_eq!(summary.tracks, 3);
    assert_eq!(summary.queue, 1);
    assert!(summary.paused);
    assert!(shown.contains("playlist"), "{shown}");
    assert!(shown.contains("pages 2"), "{shown}");
    assert!(shown.contains("tracks 3"), "{shown}");
    assert!(shown.contains("queue 1"), "{shown}");
    assert!(!shown.contains("spotify:"), "{shown}");
}

#[test]
fn the_summary_reports_skew() {
    let state = TransferState {
        playback: MessageField::some(Playback {
            timestamp: Some(10_000),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(summarise(&state, 13_750).skew_ms, Some(3_750));
    assert_eq!(summarise(&state, 8_000).skew_ms, Some(-2_000));
    assert_eq!(summarise(&TransferState::default(), 13_750).skew_ms, None);
}
