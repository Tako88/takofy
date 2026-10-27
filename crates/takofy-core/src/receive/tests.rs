use std::time::Duration;

use librespot::core::dealer::protocol::{
    AddToQueueCommand, LoggingParams, PauseCommand, PlayCommand, PlayOptions, Request,
    SetQueueCommand, SkipTo, TransferCommand, TransferOptions,
};
use librespot::protocol::context::Context;
use librespot::protocol::context_page::ContextPage;
use librespot::protocol::context_track::ContextTrack;
use librespot::protocol::player::{PlayOrigin, ProvidedTrack};
use librespot::protocol::session::Session;
use librespot::protocol::transfer_state::TransferState;
use protobuf::MessageField;
use takofy_model::Order;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::*;
use crate::instruction::Refusal;
use crate::testing::fed;
use crate::watch::Timing;

/// The summary reaches the log only if the binary names this exact target,
/// so the name is derived from the module rather than typed twice.
#[test]
fn the_log_target_is_the_module_that_logs() {
    assert_eq!(
        module_path!().strip_suffix("::tests"),
        Some(RECEIVE_LOG_TARGET),
        "the summary's target must be the module that logs it"
    );
}

/// Longer than any test takes, so only the loop itself can answer a request.
const PATIENT: Duration = Duration::from_secs(30);

/// Long enough that a loop that must answer has answered.
const ARRIVAL_WAIT: Duration = Duration::from_secs(1);

fn logging() -> LoggingParams {
    LoggingParams {
        interaction_ids: None,
        device_identifier: None,
        command_initiated_time: None,
        page_instance_ids: None,
        command_id: None,
    }
}

fn request(message_id: u32, command: Command) -> Request {
    Request {
        message_id,
        sent_by_device_id: "phone".to_owned(),
        command,
    }
}

fn pause() -> Command {
    Command::Pause(PauseCommand {
        logging_params: logging(),
    })
}

fn registry() -> Arc<Registry> {
    Arc::new(Registry::new(Timing {
        answer: PATIENT,
        ..Timing::default()
    }))
}

fn start(
    registry: Arc<Registry>,
) -> (
    UnboundedSender<RequestReply>,
    UnboundedReceiver<Inbound>,
    JoinHandle<()>,
) {
    let (requests_tx, requests_rx) = unbounded_channel();
    let (inbound_tx, inbound_rx) = unbounded_channel();
    let task = tokio::spawn(serve(fed(requests_rx), inbound_tx, registry));
    (requests_tx, inbound_rx, task)
}

async fn recv(rx: &mut UnboundedReceiver<Inbound>) -> Inbound {
    tokio::time::timeout(ARRIVAL_WAIT, rx.recv())
        .await
        .expect("the loop must hand the request over")
        .expect("the loop must still be alive")
}

#[tokio::test]
async fn a_request_becomes_an_inbound_with_a_fresh_id_and_its_kind() {
    let registry = registry();
    let (requests, mut inbound, _task) = start(registry);
    let (reply_tx, _reply_rx) = unbounded_channel();

    requests
        .send((request(11, pause()), reply_tx))
        .expect("the stream takes the request");

    let received = recv(&mut inbound).await;
    assert_eq!(received.kind, Kind::Pause);
    assert_eq!(received.instruction, Instruction::Order(Order::Pause));
    assert_eq!(received.id, RequestId(0), "the first request is id zero");
}

#[tokio::test]
async fn a_transfer_without_data_arrives_refused() {
    let registry = registry();
    let (requests, mut inbound, _task) = start(registry);
    let (reply_tx, _reply_rx) = unbounded_channel();
    let bare = Command::Transfer(TransferCommand {
        data: None,
        options: TransferOptions::default(),
        from_device_identifier: String::new(),
        logging_params: logging(),
    });

    requests
        .send((request(1, bare), reply_tx))
        .expect("the stream takes the request");

    let received = recv(&mut inbound).await;
    assert_eq!(received.kind, Kind::Transfer);
    assert_eq!(received.instruction, Instruction::Refused(Refusal::NoData));
}

#[tokio::test]
async fn ids_are_never_reused() {
    let registry = registry();
    let (requests, mut inbound, _task) = start(registry);

    let mut seen = Vec::new();
    for message_id in 0..3 {
        let (reply_tx, _reply_rx) = unbounded_channel();
        requests
            .send((request(message_id, pause()), reply_tx))
            .expect("the stream takes the request");
        seen.push(recv(&mut inbound).await.id);
    }

    assert_eq!(
        seen,
        vec![RequestId(0), RequestId(1), RequestId(2)],
        "every request gets its own id"
    );
}

#[tokio::test]
async fn a_request_remembers_its_message_id_and_sender() {
    let registry = registry();
    let (requests, mut inbound, _task) = start(Arc::clone(&registry));
    let (reply_tx, _reply_rx) = unbounded_channel();

    requests
        .send((request(41, pause()), reply_tx))
        .expect("the stream takes the request");

    let received = recv(&mut inbound).await;
    let ack = registry
        .ack_of(received.id)
        .expect("a handed-over request is registered");
    assert_eq!(ack.message_id, 41);
    assert_eq!(ack.sender, "phone");
}

#[tokio::test]
async fn a_request_the_window_cannot_take_is_answered_failure_at_once() {
    let registry = registry();
    let (requests, inbound, _task) = start(registry);
    drop(inbound);
    let (reply_tx, mut reply_rx) = unbounded_channel();

    requests
        .send((request(1, pause()), reply_tx))
        .expect("the stream takes the request");

    let reply = tokio::time::timeout(ARRIVAL_WAIT, reply_rx.recv())
        .await
        .expect("a request nobody can take is answered without waiting")
        .expect("the dealer's reply channel is open");
    assert!(matches!(reply, Reply::Failure));
}

#[tokio::test]
async fn the_loop_ends_with_its_stream() {
    let registry = registry();
    let (requests, _inbound, task) = start(registry);

    drop(requests);

    tokio::time::timeout(ARRIVAL_WAIT, task)
        .await
        .expect("the loop ends when the dealer stops sending")
        .expect("the loop must not panic");
}

#[tokio::test]
async fn a_logged_request_carries_kinds_and_counts_but_no_uri() {
    let state = TransferState {
        current_session: MessageField::some(Session {
            context: MessageField::some(Context {
                uri: Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned()),
                pages: vec![ContextPage {
                    tracks: vec![ContextTrack {
                        uri: Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let transfer = Command::Transfer(TransferCommand {
        data: Some(state),
        options: TransferOptions::default(),
        from_device_identifier: String::new(),
        logging_params: logging(),
    });

    let shown = line(&transfer, &Instruction::Other(Kind::Transfer));
    assert!(shown.starts_with("connect request: "), "{shown}");
    assert!(shown.contains("transfer"), "{shown}");
    assert!(shown.contains("tracks 1"), "{shown}");
    assert!(!shown.contains("spotify:"), "{shown}");

    let add = Command::AddToQueue(AddToQueueCommand {
        track: ProvidedTrack {
            uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
            ..Default::default()
        },
        logging_params: logging(),
    });
    let shown = line(&add, &Instruction::Other(Kind::AddToQueue));
    assert_eq!(shown, "connect request: add_to_queue");
    assert!(!shown.contains("spotify:"), "{shown}");

    let queue = Command::SetQueue(SetQueueCommand {
        next_tracks: vec![ProvidedTrack {
            uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
            ..Default::default()
        }],
        prev_tracks: Vec::new(),
        queue_revision: "rev-7".to_owned(),
        logging_params: logging(),
    });
    let shown = line(
        &queue,
        &Instruction::Order(Order::SetQueue {
            prev: Vec::new(),
            next: vec!["a uri".to_owned()],
            revision: "rev-7".to_owned(),
        }),
    );
    assert!(shown.contains("set_queue"), "{shown}");
    assert!(shown.contains("prev 0 rows"), "{shown}");
    assert!(shown.contains("next 1 rows"), "{shown}");
    assert!(!shown.contains("spotify:"), "{shown}");
    assert!(!shown.contains("rev-7"), "{shown}");

    let play = Command::Play(Box::new(PlayCommand {
        context: Context {
            uri: Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned()),
            pages: vec![ContextPage {
                tracks: vec![ContextTrack {
                    uri: Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        play_origin: PlayOrigin::default(),
        options: PlayOptions {
            skip_to: Some(SkipTo {
                track_uid: Some("a uid".to_owned()),
                ..Default::default()
            }),
            player_options_override: None,
            license: None,
            seek_to: None,
            always_play_something: None,
            audio_stream: None,
            initially_paused: None,
            prefetch_level: None,
            system_initiated: None,
        },
        logging_params: logging(),
    }));
    let shown = line(&play, &Instruction::Other(Kind::Play));
    assert!(shown.contains("play, context playlist"), "{shown}");
    assert!(
        shown.contains("skip uri absent, index absent, uid named"),
        "{shown}"
    );
    assert!(!shown.contains("spotify:"), "{shown}");

    let bare = Command::Transfer(TransferCommand {
        data: None,
        options: TransferOptions::default(),
        from_device_identifier: String::new(),
        logging_params: logging(),
    });
    assert_eq!(
        line(&bare, &Instruction::Refused(Refusal::NoData)),
        "connect request: transfer, no data, refused no data"
    );
}
