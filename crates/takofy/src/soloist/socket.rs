//! The WebSocket side of the daemon's local socket.
//!
//! Two tasks, one per direction, over one connection: text frames in, text
//! frames out, and either side ending closes both. Frames are relayed as text
//! and never read here.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use takofy_core::soloist::Connection;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::unbounded_channel;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{WebSocketStream, connect_async};

/// How long one connect attempt may take.
pub const CONNECT_BUDGET: Duration = Duration::from_secs(2);

/// Why a connection was not made. Kinds only: no address and no wire text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketError {
    /// The daemon refused or the handshake failed.
    Refused,
    /// It answered nothing inside the budget.
    TimedOut,
}

/// Connects to the daemon's socket, inside one attempt's budget.
pub async fn connect(endpoint: SocketAddr) -> Result<Connection, SocketError> {
    let url = format!("ws://{endpoint}");
    match tokio::time::timeout(CONNECT_BUDGET, connect_async(url)).await {
        Ok(Ok((socket, _))) => Ok(bridge(socket)),
        Ok(Err(_)) => Err(SocketError::Refused),
        Err(_) => Err(SocketError::TimedOut),
    }
}

/// Splits one WebSocket into the channel pair the sound speaks through.
fn bridge<S>(socket: WebSocketStream<S>) -> Connection
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (outgoing, mut requests) = unbounded_channel::<String>();
    let (frames, incoming) = unbounded_channel::<String>();
    let (mut sink, mut stream) = socket.split();

    tokio::spawn(async move {
        while let Some(text) = requests.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        // The sound dropped its sender or the socket refused: either way this
        // direction is over, and closing here is what the far side reads.
        let _ = sink.close().await;
    });

    tokio::spawn(async move {
        while let Some(message) = stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if frames.send(text.as_str().to_owned()).is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                // The protocol here is text: a ping is the socket's own
                // business and a binary frame is not ours to read.
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    });

    Connection::new(outgoing, incoming)
}

#[cfg(test)]
mod tests;
