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

use crate::instruction::{
    Instruction, Kind, instruction_of, kind_of, summarise, summarise_play, summarise_queue,
};
use crate::registry::{Ack, Registry, Responder, Verdict};

/// The log target of this module, so the binary can let its one summary line
/// through the app's `Warn` floor without raising it for everything.
pub const RECEIVE_LOG_TARGET: &str = "takofy_core::receive";

/// One request the window has to deal with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inbound {
    pub id: RequestId,
    pub kind: Kind,
    /// What the request asked for, decoded once so every handler agrees.
    pub instruction: Instruction,
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
        let instruction = instruction_of(&request);
        log::info!("{}", line(&request.command, &instruction));
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
                instruction,
            })
            .is_err()
        {
            registry.refuse(id);
        }
    }
}

/// The whole of what a request's log line carries, so the seam that logs and
/// the test that guards it are one function.
fn line(command: &Command, instruction: &Instruction) -> String {
    let mut text = format!("connect request: {}", describe(command));
    if let Instruction::Refused(refusal) = instruction {
        text.push_str(", refused ");
        text.push_str(refusal.word());
    }
    text
}

/// A request in the words a log may carry.
fn describe(command: &Command) -> String {
    match command {
        Command::Transfer(transfer) => match transfer.data.as_ref() {
            Some(state) => format!("transfer, {}", summarise(state, now_ms())),
            None => "transfer, no data".to_owned(),
        },
        // A phone-side play and a queue window carry payloads whose counts are
        // what a live session is checked against, so they get the same
        // counts-only treatment as a hand-over.
        Command::Play(play) => format!("play, {}", summarise_play(play)),
        Command::SetQueue(queue) => format!("set_queue, {}", summarise_queue(queue)),
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
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests;
