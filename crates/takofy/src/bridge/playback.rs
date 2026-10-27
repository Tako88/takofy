//! Local playback: starting the player, loading a track, and draining its events.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use takofy_core::playback::PlaybackEvent;
use takofy_ui::message::{Command, TransportUpdate, Update};
use tokio::sync::mpsc::UnboundedReceiver;

use super::covers::Drains;
use super::errors::{ends_playability, playback_failure};
use super::wake::Notifier;
use crate::playhead::{Playhead, Verdict};
use crate::session::{SessionError, SessionHost};

/// Starts the player if needed and hands the one drain thread its events.
pub(super) fn play<H: SessionHost>(host: &mut H, notifier: &Notifier, drains: &Drains, uri: &str) {
    drains.playhead.loaded(uri, 0, true, Instant::now());
    match host.play(uri) {
        Ok(()) => {
            // The device can be picked from now on, so the account is told so
            // before the player's first event arrives.
            host.set_playable(true);
            ensure_drain(host, notifier, drains);
        }
        Err(error) => {
            drains.playhead.clear();
            if ends_playability(&error) {
                host.set_playable(false);
            }
            notifier.send(Update::Transport(TransportUpdate::Failed(
                playback_failure(&error),
            )));
        }
    }
}

/// Loads one track, recording it for the playhead and settling a paused load's
/// transport, which librespot may not announce.
pub(super) fn start_load<H: SessionHost>(
    host: &mut H,
    playhead: &Playhead,
    notifier: &Notifier,
    uri: &str,
    position_ms: u32,
    play: bool,
) -> Result<(), SessionError> {
    playhead.loaded(uri, position_ms, play, Instant::now());
    host.load(uri, position_ms, play)?;
    if !play {
        notifier.send(Update::Transport(TransportUpdate::Position {
            position_ms,
            paused: true,
        }));
    }
    Ok(())
}

/// Hands the player's events to the one drain thread, if a player was started
/// since the last call.
///
/// A transfer normally follows a local play, so this is usually a no-op. A
/// stale picker row can still send one to a window that never played, and then
/// preparing the player would leave its events with nobody to read them.
pub(super) fn ensure_drain<H: SessionHost>(host: &mut H, notifier: &Notifier, drains: &Drains) {
    if let Some(events) = host.take_events() {
        spawn_drain(notifier.clone(), events, drains);
    }
}

/// Whether a command has a say in where a rebuild resumes, or signs in again,
/// so it is handled at once instead of after the attempt.
pub(super) fn interrupts_attempt(command: &Command) -> bool {
    matches!(
        command,
        Command::Play { .. }
            | Command::Pause
            | Command::Resume
            | Command::Seek { .. }
            | Command::Stop
            | Command::Authorize
    )
}

/// Drains playback events onto the update channel for as long as the player
/// lives. A plain thread needs no runtime, so `blocking_recv` cannot delay the
/// runtime's drop.
///
/// A failure the playhead reads as the link is withheld, and its generation is
/// signalled instead: the command task rebuilds, and the reload decides what to
/// tell the window.
pub(super) fn spawn_drain(
    notifier: Notifier,
    mut events: UnboundedReceiver<PlaybackEvent>,
    drains: &Drains,
) {
    let retired = Arc::clone(&drains.retired);
    let playhead = drains.playhead.clone();
    let signal = drains.signal.clone();
    let generation = playhead.generation();
    std::thread::spawn(move || {
        while let Some(event) = events.blocking_recv() {
            if retired.load(Ordering::SeqCst) {
                return;
            }
            let now = Instant::now();
            if event == PlaybackEvent::Unavailable
                && playhead.judge_unavailable(generation, now) != Verdict::Forward
            {
                let _ = signal.send(generation);
                continue;
            }
            playhead.observe(generation, &event, now);
            notifier.send(transport_of(&event));
        }
    });
}

/// Turns a playback event into the window's vocabulary. Total over the event
/// set, so a new event is a build failure here.
pub(crate) fn transport_of(event: &PlaybackEvent) -> Update {
    Update::Transport(match event {
        PlaybackEvent::Loading => TransportUpdate::Loading,
        PlaybackEvent::Playing { position_ms } => TransportUpdate::Position {
            position_ms: *position_ms,
            paused: false,
        },
        PlaybackEvent::Paused { position_ms } => TransportUpdate::Position {
            position_ms: *position_ms,
            paused: true,
        },
        PlaybackEvent::Ended => TransportUpdate::Ended,
        PlaybackEvent::Unavailable => TransportUpdate::Unavailable,
        PlaybackEvent::Stopped => TransportUpdate::Stopped,
        PlaybackEvent::Duration { duration_ms } => TransportUpdate::Duration {
            duration_ms: *duration_ms,
        },
        PlaybackEvent::Seeked { position_ms } => TransportUpdate::Seeked {
            position_ms: *position_ms,
        },
    })
}
