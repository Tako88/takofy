//! The transport's state and the position it draws.
//!
//! The position is a pure function of an anchor and an elapsed time, so the
//! interpolation is testable without a clock and the app owns the clock.

use std::time::Duration;

use takofy_model::{CoverUrl, TrackEntry};

use crate::message::{Command, Failure, TransportUpdate, Update};
use crate::signin::failure_message;
use crate::volume::Volume;

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
    /// The track's artists, as one line. Held for the same reason as the title:
    /// the list the track came from may no longer be on screen.
    pub artists: String,
    /// The track's album cover, as it was known when the track started. The
    /// panel and the playbar look it up each frame, so a cover that arrives
    /// later still reaches them.
    pub cover: Option<CoverUrl>,
}

/// The transport's anchor and the track it is about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transport {
    pub position_ms: u32,
    pub state: TransportState,
    pub now_playing: Option<NowPlaying>,
    /// The loaded track's length, once the player has reported it.
    pub duration_ms: Option<u32>,
    /// Where a drag on the seek bar has left the position. It outlives the
    /// release, so the bar holds the pointer's answer instead of falling back to
    /// where the track still is, and is dropped when the player confirms.
    pub scrub: Option<u32>,
    /// Whether the pointer is still down. Ending a drag is a one-shot transition,
    /// so asking twice about the same release asks once.
    pub dragging: bool,
    /// The player's level and its lid.
    pub volume: Volume,
}

/// The position and state a redraw interpolates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub position_ms: u32,
    pub state: TransportState,
}

impl Transport {
    /// The level the player has not been told, as the command that tells it.
    ///
    /// Taken rather than read, so a frame sends one command per change instead of
    /// one per redraw of an unchanged bar.
    pub fn take_volume(&mut self) -> Option<Command> {
        self.volume
            .take()
            .map(|percent| Command::Volume { percent })
    }

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
                self.scrub = None;
            }
            TransportUpdate::Unavailable => {
                self.state = TransportState::Unavailable;
                self.scrub = None;
            }
            TransportUpdate::Duration { duration_ms } => self.duration_ms = Some(*duration_ms),
            TransportUpdate::Seeked { position_ms } => {
                self.position_ms = *position_ms;
                self.scrub = None;
            }
            TransportUpdate::Failed(failure) => {
                self.state = TransportState::Failed(*failure);
                self.scrub = None;
            }
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
    pub fn play(
        &mut self,
        uri: &str,
        title: &str,
        artists: &str,
        cover: Option<CoverUrl>,
    ) -> Command {
        self.now_playing = Some(NowPlaying {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: artists.to_owned(),
            cover,
        });
        self.position_ms = 0;
        self.duration_ms = None;
        self.scrub = None;
        self.state = TransportState::Loading;
        Command::Play {
            uri: uri.to_owned(),
        }
    }

    /// Settles on a track the engine loaded for a hand-over.
    ///
    /// The engine owns that load, so no `Play` is asked for here: naming the
    /// track and the state it starts in is the whole of it. A playing landing is
    /// `Loading`, because it has been accepted and has not started; the length
    /// is unknown until the player reports it.
    pub fn landed(&mut self, entry: &TrackEntry, position_ms: u32, playing: bool) {
        self.now_playing = Some(NowPlaying {
            uri: entry.uri.clone(),
            title: entry.title.clone(),
            artists: entry.artists.clone(),
            cover: entry.cover.clone(),
        });
        self.position_ms = position_ms;
        self.duration_ms = None;
        self.scrub = None;
        self.state = if playing {
            TransportState::Loading
        } else {
            TransportState::Paused
        };
    }

    /// How the transport holds `uri`: whether it is the loaded track, and
    /// whether a pause would act on it.
    ///
    /// One decision, because two callers draw the same row: the search menu's
    /// glyph promises what Enter will do (R10/R11), and the shell performs it.
    /// A glyph that promised a pause the action turns into a restart is what
    /// this rules out.
    pub fn hold(&self, uri: &str) -> Hold {
        let loaded = self.now_playing.as_ref().is_some_and(|now| now.uri == uri);
        Hold {
            loaded,
            held: loaded
                && matches!(
                    self.state,
                    TransportState::Playing | TransportState::Loading | TransportState::Paused
                ),
        }
    }

    /// Whether the loaded track can be jumped in: playing or paused, with a
    /// known non-zero length.
    pub fn seekable(&self) -> bool {
        matches!(self.state, TransportState::Playing | TransportState::Paused)
            && self.length_known()
    }

    /// The command that jumps to a fraction of the track, if it is seekable.
    pub fn seek_to_fraction(&self, fraction: f32) -> Option<Command> {
        self.seek_target(fraction)
            .map(|position_ms| Command::Seek { position_ms })
    }

    /// Follows the pointer across the bar. Nothing is asked for while the drag
    /// runs: the player is told once, when the button comes back up.
    pub fn scrub_to(&mut self, fraction: f32) {
        self.scrub = self.seek_target(fraction);
        self.dragging = true;
    }

    /// Ends a drag, asking for the position it landed on. The preview stays until
    /// the player reports the seek, so a release does not snap the bar back.
    pub fn scrub_end(&mut self) -> Option<Command> {
        if !self.dragging {
            return None;
        }
        self.dragging = false;
        self.scrub.map(|position_ms| Command::Seek { position_ms })
    }

    /// The clamped position a fraction lands on, or `None` when the track cannot
    /// be jumped in.
    fn seek_target(&self, fraction: f32) -> Option<u32> {
        if !self.seekable() {
            return None;
        }
        self.position_at_fraction(fraction)
    }

    /// The clamped position a fraction lands on, asking only that the length is
    /// known: a holder's transport carries its track while it is still loading,
    /// and the seek the bar offers it is enabled on that alone.
    pub(crate) fn length_target(&self, fraction: f32) -> Option<u32> {
        if !self.length_known() {
            return None;
        }
        self.position_at_fraction(fraction)
    }

    /// The clamped position `delta_ms` after `position_ms`, asking only that the
    /// length is known.
    pub(crate) fn length_by(&self, position_ms: u32, delta_ms: i64) -> Option<u32> {
        if !self.length_known() {
            return None;
        }
        self.position_by(position_ms, delta_ms)
    }

    /// The command that jumps `delta_ms` from the anchor, clamped to the track.
    pub fn seek_by(&self, delta_ms: i64) -> Option<Command> {
        if !self.seekable() {
            return None;
        }
        self.position_by(self.position_ms, delta_ms)
            .map(|position_ms| Command::Seek { position_ms })
    }

    /// Whether a known non-zero length makes a seek possible at all.
    ///
    /// The bar's slider, its arrow keys and a holder's steer all ask here, so
    /// the three cannot disagree about which transports can be jumped in.
    pub(crate) fn length_known(&self) -> bool {
        self.duration_ms.is_some_and(|duration| duration > 0)
    }

    fn position_at_fraction(&self, fraction: f32) -> Option<u32> {
        let duration = self.duration_ms?;
        let target = (fraction.clamp(0.0, 1.0) * duration as f32) as u32;
        Some(target.min(duration.saturating_sub(1)))
    }

    fn position_by(&self, position_ms: u32, delta_ms: i64) -> Option<u32> {
        let max = i64::from(self.duration_ms?.saturating_sub(1));
        let target = i64::from(position_ms).saturating_add(delta_ms);
        Some(target.clamp(0, max) as u32)
    }

    /// The position to draw after `elapsed`, never past a known duration.
    pub fn shown_position(&self, elapsed: Duration) -> u32 {
        if let Some(scrub) = self.scrub {
            return scrub;
        }
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

/// How the transport holds one track.
///
/// [`Transport::hold`] is the only place that decides it, so a row's glyph and
/// the action it stands for cannot disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hold {
    /// The transport's own track is this one.
    pub loaded: bool,
    /// Pausing acts on it: it is playing, loading, or paused. `false` with
    /// [`Self::loaded`] means the transport is on the track but a move would
    /// restart it instead.
    pub held: bool,
}

/// The position after `elapsed`, or the anchor's own when not playing.
pub fn position_at(anchor: &Anchor, elapsed: Duration) -> u32 {
    if anchor.state != TransportState::Playing {
        return anchor.position_ms;
    }
    let elapsed_ms = u128::from(anchor.position_ms).saturating_add(elapsed.as_millis());
    elapsed_ms.min(u128::from(u32::MAX)) as u32
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

    /// The one hold decision: only playing, loading and paused can be paused;
    /// every other state is a restart, and a row draws its glyph from this.
    #[test]
    fn hold_separates_a_pausable_track_from_a_restart() {
        let cases = [
            (TransportState::Idle, false),
            (TransportState::Loading, true),
            (TransportState::Playing, true),
            (TransportState::Paused, true),
            (TransportState::Unavailable, false),
            (TransportState::Stopped, false),
            (TransportState::Failed(Failure::Other), false),
        ];
        for (state, held) in cases {
            let mut transport = Transport::default();
            transport.play("spotify:track:a", "A", "B", None);
            transport.state = state;
            assert_eq!(
                transport.hold("spotify:track:a"),
                Hold { loaded: true, held },
                "{state:?}"
            );
            // A uri the transport is not on is never held, whatever the state.
            assert_eq!(
                transport.hold("spotify:track:b"),
                Hold {
                    loaded: false,
                    held: false
                },
                "{state:?}"
            );
        }

        let idle = Transport::default();
        assert_eq!(
            idle.hold("spotify:track:a"),
            Hold::default(),
            "nothing is loaded, so nothing is held"
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

    /// A transport that is playing a track with a length, which is the only state
    /// a drag can happen in.
    fn seekable_transport() -> Transport {
        let mut transport = Transport::default();
        transport.apply(&transport_update(TransportUpdate::Duration {
            duration_ms: 100_000,
        }));
        transport.apply(&transport_update(TransportUpdate::Position {
            position_ms: 10_000,
            paused: false,
        }));
        transport
    }

    #[test]
    fn a_drag_shows_where_the_pointer_is_not_where_the_track_is() {
        let mut transport = seekable_transport();
        transport.scrub_to(0.75);
        assert_eq!(transport.shown_position(Duration::ZERO), 75_000);
    }

    #[test]
    fn a_drag_asks_for_nothing_until_it_ends() {
        let mut transport = seekable_transport();
        transport.scrub_to(0.25);
        transport.scrub_to(0.5);
        assert_eq!(
            transport.scrub_end(),
            Some(Command::Seek {
                position_ms: 50_000
            }),
            "the release asks for where it was let go",
        );
        assert_eq!(transport.scrub_end(), None, "one drag is one jump");
    }

    #[test]
    fn a_drag_holds_its_place_until_the_player_reports_the_seek() {
        let mut transport = seekable_transport();
        transport.scrub_to(0.9);
        transport.scrub_end();
        assert_eq!(
            transport.shown_position(Duration::from_secs(5)),
            90_000,
            "the bar must not fall back to where the track still is",
        );
        transport.apply(&transport_update(TransportUpdate::Seeked {
            position_ms: 1_000,
        }));
        assert_eq!(
            transport.shown_position(Duration::ZERO),
            1_000,
            "and the player's answer wins once it arrives",
        );
    }

    #[test]
    fn a_drag_on_a_track_with_no_length_does_nothing() {
        let mut transport = Transport::default();
        transport.scrub_to(0.5);
        assert_eq!(transport.shown_position(Duration::ZERO), 0);
        assert_eq!(transport.scrub_end(), None);
    }

    #[test]
    fn a_drag_past_either_end_clamps_to_the_track() {
        let mut transport = seekable_transport();
        transport.scrub_to(1.5);
        assert_eq!(
            transport.scrub_end(),
            Some(Command::Seek {
                position_ms: 99_999
            }),
            "the last millisecond, never the length itself",
        );
        transport.scrub_to(-0.5);
        assert_eq!(
            transport.scrub_end(),
            Some(Command::Seek { position_ms: 0 }),
        );
    }

    #[test]
    fn playing_another_track_drops_a_drag() {
        let mut transport = seekable_transport();
        transport.scrub_to(0.5);
        transport.play("spotify:track:y", "Another", "Someone", None);
        assert_eq!(transport.shown_position(Duration::ZERO), 0);
    }

    #[test]
    fn play_names_the_track_and_asks_for_it() {
        let mut transport = Transport::default();
        assert_eq!(
            transport.play("spotify:track:x", "A Title", "A Band", None),
            Command::Play {
                uri: "spotify:track:x".to_owned()
            }
        );
        assert_eq!(
            transport.now_playing,
            Some(NowPlaying {
                uri: "spotify:track:x".to_owned(),
                title: "A Title".to_owned(),
                artists: "A Band".to_owned(),
                cover: None,
            })
        );
        assert_eq!(transport.state, TransportState::Loading);
    }

    #[test]
    fn play_replaces_the_artist_with_the_new_tracks() {
        let mut transport = Transport::default();
        transport.play("spotify:track:x", "A Title", "A Band", None);
        transport.play("spotify:track:y", "Another", "Another Band", None);

        let now = transport
            .now_playing
            .as_ref()
            .expect("the second track is named");
        assert_eq!(now.artists, "Another Band");
        assert_eq!(now.title, "Another");

        // An entry the metadata gave no artist for is one with an empty line, not
        // the previous track's artist left standing.
        transport.play("spotify:track:z", "Third", "", None);
        let now = transport
            .now_playing
            .as_ref()
            .expect("the third track is named");
        assert!(now.artists.is_empty());
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

        transport.play("spotify:track:x", "A Title", "A Band", None);
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
    fn a_restored_level_is_one_command_on_the_first_take() {
        let mut transport = Transport {
            volume: Volume::restored(35, false),
            ..Transport::default()
        };
        assert_eq!(
            transport.take_volume(),
            Some(Command::Volume { percent: 35 }),
            "a restored level must reach the player, not stay a slider position"
        );
        assert_eq!(transport.take_volume(), None, "one change is one command");
    }

    #[test]
    fn a_restored_mute_takes_volume_to_zero() {
        let mut transport = Transport {
            volume: Volume::restored(35, true),
            ..Transport::default()
        };
        assert_eq!(
            transport.take_volume(),
            Some(Command::Volume { percent: 0 }),
            "a restored mute owes the level that is heard, which is silence"
        );
    }

    #[test]
    fn a_default_transport_owes_no_volume_command() {
        let mut transport = Transport::default();
        assert_eq!(
            transport.take_volume(),
            None,
            "the player already starts at full, so there is nothing to send"
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
    fn the_redraw_interval_is_non_zero() {
        assert!(REDRAW > Duration::ZERO);
    }
}
