//! The Soloist sound: one task folding frames and rows into play events.
//!
//! The task owns the fold, the end-of-load judge and the command sequence. It
//! selects over the daemon's frames, the trait calls and a slow tick. It logs
//! fault kinds only: never a frame, a track URI or a key.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval};

use super::ending::{Ending, Verdict};
use super::frames::{Command, Frame};
use super::sequence::{Load, Output, Sequence};
use super::state::{Expect, Fold};
use super::{SOLOIST_LOG_TARGET, SoloistFault};
use crate::playback::{PlaybackError, PlaybackEvent, Sound, playable_track};

/// How often the task advances timers no frame reports.
pub const TICK: Duration = Duration::from_millis(250);

/// The channels a WebSocket adapter fulfils.
#[derive(Debug)]
pub struct Connection {
    pub outgoing: UnboundedSender<String>,
    pub incoming: UnboundedReceiver<String>,
}

impl Connection {
    pub fn new(outgoing: UnboundedSender<String>, incoming: UnboundedReceiver<String>) -> Self {
        Self { outgoing, incoming }
    }
}

/// A running Soloist sound.
pub struct SoloistSound {
    requests: UnboundedSender<Request>,
    task: JoinHandle<()>,
}

#[derive(Debug)]
enum Request {
    Load {
        uri: String,
        position_ms: u32,
        play: bool,
    },
    Seek(u32),
    Pause,
    Resume,
    Stop,
    SetVolume(u16),
}

impl SoloistSound {
    /// Starts the task and hands back the event stream beside it.
    pub fn start(connection: Connection, volume: u16) -> (Self, UnboundedReceiver<PlaybackEvent>) {
        let (events, incoming_events) = unbounded_channel();
        let (requests, incoming_requests) = unbounded_channel();
        let task = tokio::spawn(run(connection, volume.min(100), incoming_requests, events));
        (Self { requests, task }, incoming_events)
    }
}

async fn run(
    connection: Connection,
    volume: u16,
    mut requests: UnboundedReceiver<Request>,
    events: UnboundedSender<PlaybackEvent>,
) {
    let Connection {
        outgoing,
        mut incoming,
    } = connection;
    let mut fold = Fold::default();
    let mut ending = Ending::default();
    let mut sequence = Sequence::default();
    let mut expect = Expect::default();
    let mut generation = 0_u64;

    let now = Instant::now();
    send_commands(
        &outgoing,
        &[Command::SetVolume {
            volume: volume as u8,
        }],
        &mut ending,
        now,
    );

    let mut ticker = interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(text) = message else {
                    log::info!(target: SOLOIST_LOG_TARGET, "{}", SoloistFault::Lost);
                    break;
                };
                let now = Instant::now();
                let frame = super::frames::decode(&text);
                let mut reported = fold.apply(&frame, now, &mut expect);
                let output = match &frame {
                    Frame::Error { .. } => sequence.on_error(now),
                    Frame::CommandResult { command } => sequence.on_result(command, now),
                    _ => sequence.on_frame(&frame, now),
                };
                // The sequence knows which answers a superseded load still owed;
                // the ending could only guess.
                let verdict = if output.superseded {
                    Verdict::default()
                } else {
                    ending.observe(&frame, now, &fold.state, &expect)
                };
                finish(
                    &events,
                    &mut reported,
                    verdict,
                    output,
                    &outgoing,
                    &mut ending,
                    now,
                );
            }
            request = requests.recv() => {
                let Some(request) = request else {
                    break;
                };
                handle_request(
                    request,
                    &outgoing,
                    &events,
                    &mut ending,
                    &mut sequence,
                    &mut expect,
                    &mut generation,
                );
            }
            _ = ticker.tick() => {
                let now = Instant::now();
                let verdict = ending.tick(now, &fold.state, &expect);
                let output = sequence.tick(now);
                finish(
                    &events,
                    &mut Vec::new(),
                    verdict,
                    output,
                    &outgoing,
                    &mut ending,
                    now,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_request(
    request: Request,
    outgoing: &UnboundedSender<String>,
    events: &UnboundedSender<PlaybackEvent>,
    ending: &mut Ending,
    sequence: &mut Sequence,
    expect: &mut Expect,
    generation: &mut u64,
) {
    match request {
        Request::Load {
            uri,
            position_ms,
            play,
        } => {
            *generation += 1;
            *expect = Expect {
                generation: *generation,
                uri: uri.clone(),
                seen_attached: false,
            };
            let now = Instant::now();
            ending.begin_load(*generation, now);
            let output = sequence.begin(
                *generation,
                Load {
                    uri,
                    position_ms,
                    play,
                },
                now,
            );
            send_commands(outgoing, &output.commands, ending, now);
        }
        Request::Seek(position_ms) => {
            let _ = outgoing.send(Command::Seek { position_ms }.encode());
        }
        Request::Pause => {
            let _ = outgoing.send(Command::Pause.encode());
        }
        Request::Resume => {
            let _ = outgoing.send(Command::Resume.encode());
        }
        Request::Stop => {
            let _ = events.send(PlaybackEvent::Stopped);
            ending.suppress();
            sequence.abort(Instant::now());
            let _ = outgoing.send(Command::Pause.encode());
        }
        Request::SetVolume(percent) => {
            let volume = percent.min(100) as u8;
            let _ = outgoing.send(Command::SetVolume { volume }.encode());
        }
    }
}

fn finish(
    events: &UnboundedSender<PlaybackEvent>,
    reported: &mut Vec<PlaybackEvent>,
    verdict: Verdict,
    output: Output,
    outgoing: &UnboundedSender<String>,
    ending: &mut Ending,
    now: Instant,
) {
    reported.extend(verdict.events);
    if output.stopped {
        ending.suppress();
        reported.push(PlaybackEvent::Stopped);
    }
    if output.pause_refused {
        log::warn!(target: SOLOIST_LOG_TARGET, "{}", SoloistFault::Refused);
    }
    send_commands(outgoing, &output.commands, ending, now);
    if verdict.pause {
        let _ = outgoing.send(Command::Pause.encode());
    }
    for event in reported.drain(..) {
        if events.send(event).is_err() {
            return;
        }
    }
}

fn send_commands(
    outgoing: &UnboundedSender<String>,
    commands: &[Command],
    ending: &mut Ending,
    now: Instant,
) {
    for command in commands {
        if matches!(command, Command::Play { .. }) {
            ending.play_sent(now);
        }
        let _ = outgoing.send(command.encode());
    }
}

impl Sound for SoloistSound {
    fn load(&self, uri: &str, position_ms: u32, play: bool) -> Result<(), PlaybackError> {
        let _ = playable_track(uri)?;
        self.requests
            .send(Request::Load {
                uri: uri.to_owned(),
                position_ms,
                play,
            })
            .map_err(|_| PlaybackError::PlayerClosed)
    }

    fn play(&self, uri: &str) -> Result<(), PlaybackError> {
        self.load(uri, 0, true)
    }

    fn seek(&self, position_ms: u32) {
        let _ = self.requests.send(Request::Seek(position_ms));
    }

    fn pause(&self) {
        let _ = self.requests.send(Request::Pause);
    }

    fn resume(&self) {
        let _ = self.requests.send(Request::Resume);
    }

    fn stop(&self) {
        let _ = self.requests.send(Request::Stop);
    }

    fn set_volume(&self, percent: u16) {
        let _ = self.requests.send(Request::SetVolume(percent));
    }

    fn shutdown(
        self: Box<Self>,
    ) -> Pin<Box<dyn Future<Output = Result<(), PlaybackError>> + Send>> {
        Box::pin(async move {
            self.task.abort();
            let _ = self.task.await;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
