use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use takofy_core::soloist::Connection;
use tokio::io::DuplexStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

use super::{CONNECT_BUDGET, bridge};

/// One relayed connection and the far end of the cable.
async fn pair() -> (Connection, WebSocketStream<DuplexStream>) {
    let (client, server) = tokio::io::duplex(8 * 1024);
    let accepted = tokio::spawn(async move {
        tokio_tungstenite::accept_async(server)
            .await
            .expect("the far end accepts")
    });
    let (socket, _) = tokio_tungstenite::client_async("ws://127.0.0.1:9/", client)
        .await
        .expect("the handshake completes");
    let far = accepted.await.expect("the far end finished");
    (bridge(socket), far)
}

async fn sent(
    far: &mut WebSocketStream<DuplexStream>,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
    tokio::time::timeout(Duration::from_secs(2), far.next())
        .await
        .expect("an answer arrives")
}

#[tokio::test]
async fn a_command_reaches_the_daemon_as_text() {
    let (connection, mut far) = pair().await;
    connection
        .outgoing
        .send("{\"type\":\"command\"}".to_owned())
        .expect("the relay takes it");

    let Some(Ok(Message::Text(text))) = sent(&mut far).await else {
        panic!("a text frame crosses the socket");
    };
    assert_eq!(text.as_str(), "{\"type\":\"command\"}");
}

#[tokio::test]
async fn a_frame_reaches_the_sound_as_text() {
    let (mut connection, mut far) = pair().await;
    far.send(Message::Text("{\"type\":\"playback_state\"}".into()))
        .await
        .expect("the far end writes");

    let text = tokio::time::timeout(Duration::from_secs(2), connection.incoming.recv())
        .await
        .expect("a frame arrives")
        .expect("the relay holds it");
    assert_eq!(text, "{\"type\":\"playback_state\"}");
}

#[tokio::test]
async fn a_daemon_that_closes_closes_the_sound() {
    let (mut connection, mut far) = pair().await;
    far.close(None).await.expect("the far end closes");

    let answer = tokio::time::timeout(Duration::from_secs(2), connection.incoming.recv())
        .await
        .expect("the close arrives");
    assert!(answer.is_none(), "a closed socket ends the frame stream");
}

#[tokio::test]
async fn a_sound_that_closes_closes_the_daemon() {
    let (connection, mut far) = pair().await;
    // The sound gives up its sender, which is the end of the session.
    drop(connection.outgoing);

    let answer = tokio::time::timeout(Duration::from_secs(2), far.next())
        .await
        .expect("the close arrives");
    assert!(
        matches!(answer, Some(Ok(Message::Close(_))) | None),
        "the daemon is told the sound is gone: {answer:?}"
    );
}

#[test]
fn one_connect_attempt_is_bounded() {
    assert_eq!(CONNECT_BUDGET, Duration::from_secs(2));
}
