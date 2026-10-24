//! The request loop: the dealer's commands become one `Inbound` each, and every
//! one is registered so something can answer it exactly once.
//!
//! A request that reaches here can be a transfer carrying a whole context, so
//! what leaves this module in a log is counts and kinds. No command's payload is
//! ever printed.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use librespot::core::dealer::manager::{BoxedStream, Reply, RequestReply};
use librespot::core::dealer::protocol::Command;
use takofy_model::RequestId;
use tokio::sync::mpsc::UnboundedSender;

use crate::instruction::{Kind, kind_of, summarise};
use crate::registry::{Ack, Registry, Responder, Verdict};

/// The log target of this module, so the binary can let its one summary line
/// through the app's `Warn` floor without raising it for everything.
pub const RECEIVE_LOG_TARGET: &str = "takofy_core::receive";

/// One request the window has to deal with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inbound {
    pub id: RequestId,
    pub kind: Kind,
}

/// Reads the dealer's requests until its stream ends.
///
/// Registration comes before the hand-off, so the window can answer from the
/// moment it sees the request. A window that is gone is answered at once rather
/// than left to the budget.
pub(crate) async fn serve(
    mut requests: BoxedStream<RequestReply>,
    tx: UnboundedSender<Inbound>,
    registry: Arc<Registry>,
) {
    let mut next = 0u64;
    while let Some((request, reply)) = requests.next().await {
        let id = RequestId(next);
        next = next.wrapping_add(1);
        log::info!("{}", line(&request.command));
        registry.insert(
            id,
            Ack {
                message_id: request.message_id,
                sender: request.sent_by_device_id.clone(),
            },
            Box::new(DealerResponder(reply)),
        );
        if tx
            .send(Inbound {
                id,
                kind: kind_of(&request.command),
            })
            .is_err()
        {
            registry.refuse(id);
        }
    }
}

/// The whole of what a request's log line carries, so the seam that logs and
/// the test that guards it are one function.
fn line(command: &Command) -> String {
    format!("connect request: {}", describe(command))
}

/// A request in the words a log may carry.
fn describe(command: &Command) -> String {
    match command {
        Command::Transfer(transfer) => match transfer.data.as_ref() {
            Some(state) => format!("transfer, {}", summarise(state, now_ms())),
            None => "transfer, no data".to_owned(),
        },
        other => kind_of(other).word().to_owned(),
    }
}

/// The dealer's own reply channel, which it treats as a failure when dropped.
struct DealerResponder(UnboundedSender<Reply>);

impl Responder for DealerResponder {
    fn answer(&mut self, verdict: Verdict) {
        let reply = match verdict {
            Verdict::Success => Reply::Success,
            Verdict::Failure => Reply::Failure,
        };
        // A closed channel means the dealer already gave up on this request.
        let _ = self.0.send(reply);
    }
}

/// Milliseconds since the epoch, or zero when the clock is before it.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use librespot::core::dealer::protocol::{
        AddToQueueCommand, LoggingParams, PauseCommand, Request, TransferCommand, TransferOptions,
    };
    use librespot::protocol::context::Context;
    use librespot::protocol::context_page::ContextPage;
    use librespot::protocol::context_track::ContextTrack;
    use librespot::protocol::player::ProvidedTrack;
    use librespot::protocol::session::Session;
    use librespot::protocol::transfer_state::TransferState;
    use protobuf::MessageField;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
    use tokio::task::JoinHandle;

    use super::*;
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
        assert_eq!(received.id, RequestId(0), "the first request is id zero");
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

        let shown = line(&transfer);
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
        let shown = line(&add);
        assert_eq!(shown, "connect request: add_to_queue");
        assert!(!shown.contains("spotify:"), "{shown}");

        let bare = Command::Transfer(TransferCommand {
            data: None,
            options: TransferOptions::default(),
            from_device_identifier: String::new(),
            logging_params: logging(),
        });
        assert_eq!(line(&bare), "connect request: transfer, no data");
    }
}
