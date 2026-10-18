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

/// How far into a track Previous still steps back rather than restarting it.
pub const RESTART_AFTER_MS: u32 = 4_000;

/// One arrow press moves the position this far.
pub const SEEK_STEP_MS: u32 = 5_000;

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
    /// The loaded track's length, once the player has reported it.
    pub duration_ms: Option<u32>,
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
            TransportUpdate::Duration { duration_ms } => self.duration_ms = Some(*duration_ms),
            TransportUpdate::Seeked { position_ms } => self.position_ms = *position_ms,
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
        self.duration_ms = None;
        self.state = TransportState::Loading;
        Command::Play {
            uri: uri.to_owned(),
        }
    }

    /// Whether the loaded track can be jumped in: playing or paused, with a
    /// known non-zero length.
    pub fn seekable(&self) -> bool {
        matches!(self.state, TransportState::Playing | TransportState::Paused)
            && self.duration_ms.is_some_and(|duration| duration > 0)
    }

    /// The command that jumps to a fraction of the track, if it is seekable.
    pub fn seek_to_fraction(&self, fraction: f32) -> Option<Command> {
        if !self.seekable() {
            return None;
        }
        let duration = self.duration_ms?;
        let target = (fraction.clamp(0.0, 1.0) * duration as f32) as u32;
        Some(Command::Seek {
            position_ms: target.min(duration.saturating_sub(1)),
        })
    }

    /// The command that jumps `delta_ms` from the anchor, clamped to the track.
    pub fn seek_by(&self, delta_ms: i64) -> Option<Command> {
        if !self.seekable() {
            return None;
        }
        let max = i64::from(self.duration_ms?.saturating_sub(1));
        let target = i64::from(self.position_ms).saturating_add(delta_ms);
        Some(Command::Seek {
            position_ms: target.clamp(0, max) as u32,
        })
    }

    /// The position to draw after `elapsed`, never past a known duration.
    pub fn shown_position(&self, elapsed: Duration) -> u32 {
        let position = position_at(&self.anchor(), elapsed);
        match self.duration_ms {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    /// The filled fraction at `position_ms`, or `None` with no length to
    /// measure it against.
    pub fn fraction(&self, position_ms: u32) -> Option<f32> {
        let duration = self.duration_ms?;
        (duration > 0).then(|| (position_ms as f32 / duration as f32).clamp(0.0, 1.0))
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

/// Formats the time left, in the reference's `-m:ss` form.
pub fn format_remaining(position_ms: u32, duration_ms: u32) -> String {
    format!(
        "-{}",
        format_position(duration_ms.saturating_sub(position_ms))
    )
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
    fn duration_sets_the_length_and_play_clears_it() {
        let mut transport = Transport::default();
        assert!(
            !transport.apply(&transport_update(TransportUpdate::Duration {
                duration_ms: 222_000,
            })),
            "a length does not move the anchor"
        );
        assert_eq!(transport.duration_ms, Some(222_000));

        transport.play("spotify:track:x", "A Title");
        assert_eq!(transport.duration_ms, None);
    }

    #[test]
    fn seeked_moves_the_position_and_keeps_the_state() {
        let mut transport = Transport {
            state: TransportState::Playing,
            ..Transport::default()
        };
        assert!(transport.apply(&transport_update(TransportUpdate::Seeked {
            position_ms: 41_000,
        })));
        assert_eq!(transport.position_ms, 41_000);
        assert_eq!(transport.state, TransportState::Playing);
    }

    #[test]
    fn seeked_does_not_reanchor_when_the_position_is_unchanged() {
        let mut transport = Transport {
            state: TransportState::Paused,
            position_ms: 41_000,
            ..Transport::default()
        };
        assert!(!transport.apply(&transport_update(TransportUpdate::Seeked {
            position_ms: 41_000,
        })));
    }

    #[test]
    fn seekable_is_playing_or_paused_with_a_non_zero_length() {
        for (state, duration) in [
            (TransportState::Playing, Some(1)),
            (TransportState::Paused, Some(1)),
            (TransportState::Loading, Some(1)),
            (TransportState::Stopped, Some(1)),
            (TransportState::Playing, None),
            (TransportState::Playing, Some(0)),
        ] {
            let transport = Transport {
                state,
                duration_ms: duration,
                ..Transport::default()
            };
            let expected = matches!(state, TransportState::Playing | TransportState::Paused)
                && duration == Some(1);
            assert_eq!(transport.seekable(), expected, "{state:?} {duration:?}");
        }
    }

    #[test]
    fn seek_to_fraction_clamps_and_refuses_when_not_seekable() {
        let transport = Transport {
            state: TransportState::Playing,
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert_eq!(
            transport.seek_to_fraction(0.25),
            Some(Command::Seek { position_ms: 2_500 })
        );
        assert_eq!(
            transport.seek_to_fraction(1.5),
            Some(Command::Seek { position_ms: 9_999 }),
            "the last millisecond is the furthest a seek may land"
        );
        assert_eq!(
            transport.seek_to_fraction(-1.0),
            Some(Command::Seek { position_ms: 0 })
        );

        let paused = Transport {
            state: TransportState::Paused,
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert!(paused.seek_to_fraction(0.5).is_some());

        let idle = Transport {
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert_eq!(idle.seek_to_fraction(0.5), None);
        let zero = Transport {
            state: TransportState::Playing,
            duration_ms: Some(0),
            ..Transport::default()
        };
        assert_eq!(zero.seek_to_fraction(0.5), None);
    }

    #[test]
    fn seek_by_saturates_at_both_ends() {
        let transport = Transport {
            state: TransportState::Playing,
            position_ms: 3_000,
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert_eq!(
            transport.seek_by(-100),
            Some(Command::Seek { position_ms: 2_900 })
        );
        assert_eq!(
            transport.seek_by(-5_000),
            Some(Command::Seek { position_ms: 0 }),
            "a seek before the start saturates at zero"
        );
        assert_eq!(
            transport.seek_by(20_000),
            Some(Command::Seek { position_ms: 9_999 }),
            "a seek past the end stops on the last millisecond"
        );
        assert_eq!(Transport::default().seek_by(1), None);
    }

    #[test]
    fn shown_position_is_clamped_to_the_duration() {
        let transport = Transport {
            state: TransportState::Playing,
            position_ms: 9_000,
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert_eq!(transport.shown_position(Duration::from_secs(5)), 10_000);

        let known = Transport {
            state: TransportState::Paused,
            position_ms: 4_000,
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert_eq!(known.shown_position(Duration::from_secs(5)), 4_000);
    }

    #[test]
    fn fraction_is_none_without_a_non_zero_duration() {
        let transport = Transport {
            duration_ms: Some(10_000),
            ..Transport::default()
        };
        assert_eq!(transport.fraction(2_500), Some(0.25));
        assert_eq!(transport.fraction(20_000), Some(1.0));
        assert_eq!(Transport::default().fraction(2_500), None);
        let zero = Transport {
            duration_ms: Some(0),
            ..Transport::default()
        };
        assert_eq!(zero.fraction(0), None);
    }

    #[test]
    fn format_remaining_counts_down_and_saturates() {
        assert_eq!(format_remaining(74_000, 222_000), "-2:28");
        assert_eq!(format_remaining(0, 222_000), "-3:42");
        assert_eq!(format_remaining(222_000, 222_000), "-0:00");
        assert_eq!(format_remaining(300_000, 222_000), "-0:00");
        assert_eq!(
            format_remaining(73_250, 222_000),
            "-2:28",
            "sub-second truncates"
        );
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
