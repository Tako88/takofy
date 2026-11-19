use takofy_core::soloist::Connection;
use takofy_core::soloist::frames::Command;
use tokio::sync::mpsc::unbounded_channel;

use super::auth_of;

/// A connection whose far end the test plays, and the commands it hears.
fn daemon() -> (
    Connection,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    tokio::sync::mpsc::UnboundedSender<String>,
) {
    let (outgoing, heard) = unbounded_channel();
    let (frames, incoming) = unbounded_channel();
    (Connection::new(outgoing, incoming), heard, frames)
}

#[tokio::test(start_paused = true)]
async fn the_daemon_is_asked_who_it_is() {
    let (mut connection, mut heard, _frames) = daemon();
    assert_eq!(auth_of(&mut connection).await, None);

    assert_eq!(
        heard.recv().await,
        Some(Command::GetAuthState.encode()),
        "the question carries no track URI"
    );
}

#[tokio::test]
async fn an_auth_state_answers_who_the_daemon_is() {
    let (mut connection, mut heard, frames) = daemon();
    tokio::spawn(async move {
        let _ = heard.recv().await;
        frames
            .send(
                r#"{"type":"auth_state","logged_in":true,"device_name":"Takofy Soloist"}"#
                    .to_owned(),
            )
            .expect("the answer is sent");
    });

    assert_eq!(
        auth_of(&mut connection).await,
        Some((true, "Takofy Soloist".to_owned()))
    );
}

#[tokio::test]
async fn a_frame_that_is_not_the_answer_is_passed_over() {
    let (mut connection, mut heard, frames) = daemon();
    tokio::spawn(async move {
        let _ = heard.recv().await;
        let _ = frames.send(r#"{"type":"volume_changed","volume":40}"#.to_owned());
        let _ = frames.send(r#"{"type":"auth_state","logged_in":false}"#.to_owned());
    });

    assert_eq!(auth_of(&mut connection).await, Some((false, String::new())));
}

#[tokio::test(start_paused = true)]
async fn a_daemon_that_says_nothing_is_not_trusted() {
    let (mut connection, _heard, _frames) = daemon();

    assert_eq!(auth_of(&mut connection).await, None);
}

#[tokio::test(start_paused = true)]
async fn a_socket_that_closes_before_answering_is_not_trusted() {
    let (mut connection, mut heard, frames) = daemon();
    tokio::spawn(async move {
        let _ = heard.recv().await;
        drop(frames);
    });

    assert_eq!(auth_of(&mut connection).await, None);
}

#[test]
fn a_state_with_a_sound_in_it_answers_without_a_fault() {
    use super::daemon::State;
    assert!(State::Ready.playable());
    assert!(State::Adopted.playable());
    assert!(!State::NoSocket.playable());
}
