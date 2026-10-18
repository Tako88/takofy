//! The transport's state and the position it draws.
//!
//! The position is a pure function of an anchor and an elapsed time, so the
//! interpolation is testable without a clock and the app owns the clock.

use std::time::Duration;

use crate::message::{Command, Failure, TransportUpdate, Update};
use crate::signin::failure_message;

/// How often the bar is redrawn while a track plays. A repaint rate this
/// interpolation depends on, so it is not a design token.
pub const REDRAW: Duration = Duration::from_millis(100);

/// Where the transport is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportState {
    /// Nothing has been played.
    #[default]
    Idle,
    /// A track is being loaded.
    Loading,
    /// Playback is running.
    Playing,
    /// Playback is paused.
    Paused,
    /// The track could not be played.
    Unavailable,
    /// Playback ended or was stopped.
    Stopped,
    /// The command could not be carried out.
    Failed(Failure),
}

impl TransportState {
    /// Operator-facing text for the bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Loading => "Loading",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Unavailable => "Unavailable",
            Self::Stopped => "Stopped",
            Self::Failed(failure) => failure_message(*failure),
        }
    }
}

/// What the bar names. Kept here rather than looked up when the bar draws,
/// because the list the track came from may no longer be on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub uri: String,
    pub title: String,
}

/// The transport's anchor and the track it is about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transport {
    pub position_ms: u32,
    pub state: TransportState,
    pub now_playing: Option<NowPlaying>,
}

/// The position and state a redraw interpolates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub position_ms: u32,
    pub state: TransportState,
}

impl Transport {
    /// Folds one update in, reporting whether the anchor moved so the caller
    /// can restamp its clock.
    pub fn apply(&mut self, update: &Update) -> bool {
        let Update::Transport(update) = update else {
            return false;
        };
        let before = self.anchor();
        match update {
            TransportUpdate::Loading => self.state = TransportState::Loading,
            TransportUpdate::Position {
                position_ms,
                paused,
            } => {
                self.position_ms = *position_ms;
                self.state = if *paused {
                    TransportState::Paused
                } else {
                    TransportState::Playing
                };
            }
            TransportUpdate::Ended | TransportUpdate::Stopped => {
                self.state = TransportState::Stopped;
                self.position_ms = 0;
            }
            TransportUpdate::Unavailable => self.state = TransportState::Unavailable,
            TransportUpdate::Failed(failure) => self.state = TransportState::Failed(*failure),
        }
        self.anchor() != before
    }

    /// What the next redraw interpolates from.
    pub fn anchor(&self) -> Anchor {
        Anchor {
            position_ms: self.position_ms,
            state: self.state,
        }
    }

    /// Names the track and returns the command that plays it.
    pub fn play(&mut self, uri: &str, title: &str) -> Command {
        self.now_playing = Some(NowPlaying {
            uri: uri.to_owned(),
            title: title.to_owned(),
        });
        self.position_ms = 0;
        self.state = TransportState::Loading;
        Command::Play {
            uri: uri.to_owned(),
        }
    }
}

/// The position after `elapsed`, or the anchor's own when not playing.
pub fn position_at(anchor: &Anchor, elapsed: Duration) -> u32 {
    if anchor.state != TransportState::Playing {
        return anchor.position_ms;
    }
    let elapsed_ms = u128::from(anchor.position_ms).saturating_add(elapsed.as_millis());
    elapsed_ms.min(u128::from(u32::MAX)) as u32
}

/// Formats a position as minutes and seconds.
pub fn format_position(position_ms: u32) -> String {
    let seconds = position_ms / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn transport_update(update: TransportUpdate) -> Update {
        Update::Transport(update)
    }

    #[test]
    fn a_position_while_playing_advances_with_the_elapsed_time() {
        let anchor = Anchor {
            position_ms: 1_000,
            state: TransportState::Playing,
        };
        assert_eq!(position_at(&anchor, Duration::from_millis(500)), 1_500);
    }

    #[test]
    fn a_position_while_paused_does_not_advance() {
        let anchor = Anchor {
            position_ms: 1_000,
            state: TransportState::Paused,
        };
        assert_eq!(position_at(&anchor, Duration::from_secs(10)), 1_000);
    }

    #[test]
    fn a_position_while_idle_does_not_advance() {
        let anchor = Anchor {
            position_ms: 0,
            state: TransportState::Idle,
        };
        assert_eq!(position_at(&anchor, Duration::from_secs(10)), 0);
    }

    #[test]
    fn a_position_saturates_rather_than_wrapping() {
        let anchor = Anchor {
            position_ms: u32::MAX - 5,
            state: TransportState::Playing,
        };
        assert_eq!(position_at(&anchor, Duration::from_secs(10)), u32::MAX);
    }

    #[test]
    fn loading_maps_to_the_loading_state() {
        let mut transport = Transport::default();
        assert!(transport.apply(&transport_update(TransportUpdate::Loading)));
        assert_eq!(transport.state, TransportState::Loading);
    }

    #[test]
    fn a_position_while_running_maps_to_playing() {
        let mut transport = Transport::default();
        assert!(
            transport.apply(&transport_update(TransportUpdate::Position {
                position_ms: 42,
                paused: false,
            }))
        );
        assert_eq!(transport.state, TransportState::Playing);
        assert_eq!(transport.position_ms, 42);
    }

    #[test]
    fn a_position_while_paused_maps_to_paused() {
        let mut transport = Transport::default();
        assert!(
            transport.apply(&transport_update(TransportUpdate::Position {
                position_ms: 42,
                paused: true,
            }))
        );
        assert_eq!(transport.state, TransportState::Paused);
        assert_eq!(transport.position_ms, 42);
    }

    #[test]
    fn ended_maps_to_stopped() {
        let mut transport = Transport::default();
        assert!(transport.apply(&transport_update(TransportUpdate::Ended)));
        assert_eq!(transport.state, TransportState::Stopped);
    }

    #[test]
    fn unavailable_maps_to_unavailable() {
        let mut transport = Transport::default();
        assert!(transport.apply(&transport_update(TransportUpdate::Unavailable)));
        assert_eq!(transport.state, TransportState::Unavailable);
    }

    #[test]
    fn a_stop_request_maps_to_stopped() {
        let mut transport = Transport::default();
        assert!(transport.apply(&transport_update(TransportUpdate::Stopped)));
        assert_eq!(transport.state, TransportState::Stopped);
    }

    #[test]
    fn a_failed_command_maps_to_failed() {
        let mut transport = Transport::default();
        assert!(transport.apply(&transport_update(TransportUpdate::Failed(
            Failure::NoAudioDevice,
        ))));
        assert_eq!(
            transport.state,
            TransportState::Failed(Failure::NoAudioDevice)
        );
    }

    #[test]
    fn a_position_reanchors_once_per_change() {
        let mut transport = Transport::default();
        let update = transport_update(TransportUpdate::Position {
            position_ms: 100,
            paused: false,
        });
        assert!(transport.apply(&update), "the first position is a change");
        assert!(!transport.apply(&update), "an identical position is not");
    }

    #[test]
    fn a_non_transport_update_is_ignored() {
        let mut transport = Transport::default();
        assert!(!transport.apply(&Update::SignedIn));
        assert_eq!(transport.state, TransportState::Idle);
    }

    #[test]
    fn play_names_the_track_and_asks_for_it() {
        let mut transport = Transport::default();
        assert_eq!(
            transport.play("spotify:track:x", "A Title"),
            Command::Play {
                uri: "spotify:track:x".to_owned()
            }
        );
        assert_eq!(
            transport.now_playing,
            Some(NowPlaying {
                uri: "spotify:track:x".to_owned(),
                title: "A Title".to_owned(),
            })
        );
        assert_eq!(transport.state, TransportState::Loading);
    }

    #[test]
    fn every_state_has_a_label() {
        for state in [
            TransportState::Loading,
            TransportState::Playing,
            TransportState::Paused,
            TransportState::Unavailable,
            TransportState::Stopped,
            TransportState::Failed(Failure::NoAudioDevice),
        ] {
            assert!(!state.label().trim().is_empty(), "{state:?} has no label");
        }
        assert!(TransportState::Idle.label().is_empty());
    }

    #[test]
    fn a_position_formats_as_minutes_and_seconds() {
        assert_eq!(format_position(0), "0:00");
        assert_eq!(format_position(5_000), "0:05");
        assert_eq!(format_position(65_000), "1:05");
        assert_eq!(format_position(3_600_000), "60:00");
    }

    #[test]
    fn the_redraw_interval_is_non_zero() {
        assert!(REDRAW > Duration::ZERO);
    }
}
