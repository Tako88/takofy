//! What this window has loaded, and whether a failed load was the link.
//!
//! A silent drop leaves librespot's keep-alive waiting before it notices the
//! socket is gone, so a load pressed inside that window fails on its key
//! request and reads as an unavailable track. The playback drain asks here
//! before such an `Unavailable` goes on to the window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use takofy_core::playback::PlaybackEvent;

/// How long a load may go without starting before a failed one is read as a
/// lost link. librespot's key request gives up after this long.
pub const SLOW_LOAD: Duration = Duration::from_millis(1_500);

/// Where a rebuild reloads the track, and whether it plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePoint {
    pub uri: String,
    pub position_ms: u32,
    pub play: bool,
}

/// What one `Unavailable` means for the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A real failure: the window hears about it and the queue may advance.
    Forward,
    /// The link is down, so the load never had a chance.
    Link,
    /// A load that never started, well past the key timeout.
    Suspect,
}

/// The current load's position, and how much this session's loads are trusted.
#[derive(Clone, Default)]
pub struct Playhead {
    state: Arc<Mutex<State>>,
    down: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    point: Option<Point>,
    /// When the current load was handed to the player.
    loaded_at: Option<Instant>,
    /// Whether the current load has reached `Playing`, `Paused` or `Duration`.
    started: bool,
    /// Whether any load since the last generation bump has.
    proven: bool,
    generation: u64,
}

struct Point {
    uri: String,
    position_ms: u32,
    /// Whether the track is moving, which is what the position drifts with.
    playing: bool,
    /// When `position_ms` was true.
    at: Instant,
}

impl State {
    fn anchor(&mut self, position_ms: u32, playing: bool, now: Instant) {
        if let Some(point) = self.point.as_mut() {
            point.position_ms = position_ms;
            point.playing = playing;
            point.at = now;
        }
    }

    fn forget(&mut self) {
        self.point = None;
        self.loaded_at = None;
        self.started = false;
    }
}

impl Playhead {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a load, which starts unstarted and unproven.
    pub fn loaded(&self, uri: &str, position_ms: u32, play: bool, now: Instant) {
        let mut state = self.state();
        state.point = Some(Point {
            uri: uri.to_owned(),
            position_ms,
            playing: play,
            at: now,
        });
        state.loaded_at = Some(now);
        state.started = false;
    }

    /// Records what the player did, re-anchoring the position it names.
    ///
    /// An event from a retired generation says nothing about this load.
    pub fn observe(&self, generation: u64, event: &PlaybackEvent, now: Instant) {
        let mut state = self.state();
        if generation != state.generation {
            return;
        }
        match *event {
            PlaybackEvent::Playing { position_ms } => {
                state.anchor(position_ms, true, now);
                state.started = true;
                state.proven = true;
            }
            PlaybackEvent::Paused { position_ms } => {
                state.anchor(position_ms, false, now);
                state.started = true;
                state.proven = true;
            }
            PlaybackEvent::Duration { .. } => {
                state.started = true;
                state.proven = true;
            }
            PlaybackEvent::Seeked { position_ms } => {
                if let Some(point) = state.point.as_mut() {
                    point.position_ms = position_ms;
                    point.at = now;
                }
            }
            PlaybackEvent::Ended | PlaybackEvent::Stopped => state.forget(),
            // A preload failure arrives while the loaded track still plays, so
            // only an unstarted load reads as the track itself failing.
            PlaybackEvent::Unavailable => {
                if !state.started {
                    state.forget();
                }
            }
            PlaybackEvent::Loading => {}
        }
    }

    /// Says what a failed load means, without forgetting anything.
    ///
    /// A failure from a retired generation is forwarded: it is not evidence
    /// about this one.
    pub fn judge_unavailable(&self, generation: u64, now: Instant) -> Verdict {
        let state = self.state();
        if generation != state.generation {
            return Verdict::Forward;
        }
        if self.is_down() {
            return Verdict::Link;
        }
        if !state.proven || state.started {
            return Verdict::Forward;
        }
        match state.loaded_at {
            Some(loaded_at) if now.saturating_duration_since(loaded_at) >= SLOW_LOAD => {
                Verdict::Suspect
            }
            _ => Verdict::Forward,
        }
    }

    /// Where a rebuild reloads the track, and whether it plays.
    pub fn resume_point(&self, now: Instant) -> Option<ResumePoint> {
        let state = self.state();
        let point = state.point.as_ref()?;
        // Only a started load moves: an unstarted one sits where it was asked
        // to load, however long it has been pending.
        let position_ms = if point.playing && state.started {
            point
                .position_ms
                .saturating_add(millis(now.saturating_duration_since(point.at)))
        } else {
            point.position_ms
        };
        Some(ResumePoint {
            uri: point.uri.clone(),
            position_ms,
            play: point.playing,
        })
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    pub fn is_down(&self) -> bool {
        self.down.load(Ordering::SeqCst)
    }

    /// Opens a new session generation: it starts unproven and with the link up,
    /// and keeps the point a rebuild reloads.
    pub fn next_generation(&self) -> u64 {
        let generation = {
            let mut state = self.state();
            state.generation += 1;
            state.proven = false;
            state.generation
        };
        self.set_down(false);
        generation
    }

    pub fn generation(&self) -> u64 {
        self.state().generation
    }

    /// Forgets the load, so a rebuild has nothing to reload.
    pub fn clear(&self) {
        self.state().forget();
    }
}

/// A position gap in milliseconds, capped so it can be added to one.
fn millis(elapsed: Duration) -> u32 {
    u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use takofy_core::playback::PlaybackEvent;

    use super::{Playhead, ResumePoint, SLOW_LOAD, Verdict};

    const URI: &str = "spotify:track:a";

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn point(position_ms: u32, play: bool) -> Option<ResumePoint> {
        Some(ResumePoint {
            uri: URI.to_owned(),
            position_ms,
            play,
        })
    }

    /// A head whose generation has already had one track start.
    fn started(base: Instant) -> Playhead {
        let head = Playhead::new();
        head.loaded(URI, 0, true, base);
        let generation = head.generation();
        head.observe(generation, &PlaybackEvent::Playing { position_ms: 0 }, base);
        head
    }

    /// A head on a proven generation whose current load has not started.
    fn reloaded(base: Instant) -> Playhead {
        let head = started(base);
        head.loaded(URI, 0, true, base);
        head
    }

    #[test]
    fn a_playing_load_resumes_where_it_would_have_got_to() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 1_000, true, base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Playing { position_ms: 1_200 },
            at(base, 500),
        );

        assert_eq!(
            head.resume_point(at(base, 1_500)),
            point(2_200, true),
            "the anchor plus the time since it"
        );
    }

    #[test]
    fn a_paused_load_resumes_where_it_stopped_without_drifting() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 1_000, true, base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Paused { position_ms: 4_000 },
            at(base, 10),
        );

        assert_eq!(
            head.resume_point(at(base, 60_000)),
            point(4_000, false),
            "a still track does not move however long it sits"
        );
    }

    #[test]
    fn a_load_that_was_asked_not_to_play_resumes_paused() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 7_000, false, base);

        assert_eq!(head.resume_point(at(base, 5_000)), point(7_000, false));
    }

    #[test]
    fn a_load_that_never_started_resumes_at_its_loaded_position() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 5_000, true, base);

        assert_eq!(
            head.resume_point(at(base, 10_000)),
            point(5_000, true),
            "a load that never started did not play, so its position stands"
        );
    }

    #[test]
    fn a_seek_on_a_load_that_never_started_does_not_drift() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 5_000, true, base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Seeked { position_ms: 7_000 },
            at(base, 1_000),
        );

        assert_eq!(
            head.resume_point(at(base, 10_000)),
            point(7_000, true),
            "the seek re-anchored a load the player has not started"
        );
    }

    #[test]
    fn a_seek_re_anchors_a_playing_load() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 0, true, base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Playing { position_ms: 100 },
            base,
        );
        head.observe(
            head.generation(),
            &PlaybackEvent::Seeked {
                position_ms: 30_000,
            },
            at(base, 1_000),
        );

        assert_eq!(head.resume_point(at(base, 3_000)), point(32_000, true));
    }

    #[test]
    fn an_end_and_a_stop_forget_the_load() {
        let base = Instant::now();
        for event in [PlaybackEvent::Ended, PlaybackEvent::Stopped] {
            let head = started(base);
            head.observe(head.generation(), &event, at(base, 10));
            assert_eq!(
                head.resume_point(at(base, 10)),
                None,
                "{event:?} leaves nothing to resume"
            );
        }
    }

    #[test]
    fn an_unavailable_on_a_started_load_keeps_the_resume_point() {
        let base = Instant::now();
        let head = started(base);
        let before = head.resume_point(at(base, 500));

        head.observe(
            head.generation(),
            &PlaybackEvent::Unavailable,
            at(base, 500),
        );

        assert_eq!(
            head.resume_point(at(base, 500)),
            before,
            "a preload failure while the track plays is not the track ending"
        );
    }

    #[test]
    fn an_unavailable_on_a_load_that_never_started_forgets_it() {
        let base = Instant::now();
        let head = reloaded(base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Unavailable,
            at(base, 500),
        );

        assert_eq!(head.resume_point(at(base, 500)), None);
    }

    #[test]
    fn a_loading_event_changes_nothing() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 1_000, false, base);
        head.observe(head.generation(), &PlaybackEvent::Loading, at(base, 10));

        assert_eq!(head.resume_point(at(base, 5_000)), point(1_000, false));
    }

    #[test]
    fn a_long_elapsed_time_saturates() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, u32::MAX - 500, true, base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Playing {
                position_ms: u32::MAX - 500,
            },
            base,
        );

        assert_eq!(head.resume_point(at(base, 60_000)), point(u32::MAX, true));
    }

    #[test]
    fn a_clone_sees_what_the_original_sees() {
        let base = Instant::now();
        let head = Playhead::new();
        let clone = head.clone();

        head.loaded(URI, 1_000, true, base);
        clone.observe(
            clone.generation(),
            &PlaybackEvent::Paused { position_ms: 5_000 },
            at(base, 10),
        );
        assert_eq!(head.resume_point(at(base, 60_000)), point(5_000, false));

        clone.set_down(true);
        assert!(head.is_down());

        head.set_down(false);
        assert_eq!(clone.next_generation(), 1);
        assert_eq!(head.generation(), 1);
    }

    #[test]
    fn an_unproven_generation_forwards_a_slow_failure() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 0, true, base);

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 10_000)),
            Verdict::Forward,
            "an unproven session has no evidence the link worked"
        );
    }

    #[test]
    fn a_load_that_started_forwards_a_failure() {
        let base = Instant::now();
        let head = started(base);

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 10_000)),
            Verdict::Forward
        );
    }

    #[test]
    fn a_fast_load_that_never_started_forwards_a_failure() {
        let base = Instant::now();
        let head = reloaded(base);

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 100)),
            Verdict::Forward,
            "under the key timeout the load had not given up yet"
        );
    }

    #[test]
    fn a_slow_load_that_never_started_is_suspect() {
        let base = Instant::now();
        let head = reloaded(base);

        let generation = head.generation();
        assert_eq!(
            head.judge_unavailable(generation, at(base, 1_499)),
            Verdict::Forward
        );
        assert_eq!(
            head.judge_unavailable(generation, at(base, 1_500)),
            Verdict::Suspect,
            "past the key timeout nothing but a dead link explains the failure"
        );
        assert_eq!(
            head.judge_unavailable(generation, at(base, 60_000)),
            Verdict::Suspect
        );
        assert_eq!(SLOW_LOAD, Duration::from_millis(1_500));
    }

    #[test]
    fn a_duration_proves_the_generation() {
        let base = Instant::now();
        let head = Playhead::new();
        head.loaded(URI, 0, true, base);
        head.observe(
            head.generation(),
            &PlaybackEvent::Duration {
                duration_ms: 200_000,
            },
            base,
        );
        head.loaded(URI, 0, true, base);

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 5_000)),
            Verdict::Suspect
        );
    }

    #[test]
    fn a_down_flag_reads_as_a_lost_link() {
        let base = Instant::now();
        let head = reloaded(base);
        head.set_down(true);

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 100)),
            Verdict::Link
        );
    }

    #[test]
    fn judging_keeps_the_resume_point() {
        let base = Instant::now();
        let head = reloaded(base);
        let before = head.resume_point(at(base, 2_000));

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 2_000)),
            Verdict::Suspect
        );
        assert_eq!(head.resume_point(at(base, 2_000)), before);
    }

    #[test]
    fn a_new_generation_forgets_the_proof_and_the_down_flag() {
        let base = Instant::now();
        let head = started(base);
        head.set_down(true);
        let seen = head.generation();
        let before = head.resume_point(base);

        assert_eq!(head.next_generation(), seen + 1);
        assert_eq!(head.generation(), seen + 1);
        assert!(!head.is_down());
        assert_eq!(
            head.resume_point(base),
            before,
            "the point survives the bump"
        );

        head.loaded(URI, 0, true, base);
        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 10_000)),
            Verdict::Forward,
            "a fresh generation starts unproven"
        );
    }

    #[test]
    fn clearing_forgets_the_load() {
        let base = Instant::now();
        let head = started(base);
        head.clear();

        assert_eq!(head.resume_point(base), None);
        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 10_000)),
            Verdict::Forward
        );
    }

    #[test]
    fn a_stale_playing_event_does_not_prove_the_generation() {
        let base = Instant::now();
        let head = Playhead::new();
        let retired = head.generation();
        head.next_generation();
        head.loaded(URI, 0, true, base);
        head.observe(
            retired,
            &PlaybackEvent::Playing { position_ms: 0 },
            at(base, 10),
        );
        head.loaded(URI, 0, true, at(base, 20));

        assert_eq!(
            head.judge_unavailable(head.generation(), at(base, 10_000)),
            Verdict::Forward,
            "a retired drain's event says nothing about this generation"
        );
    }

    #[test]
    fn a_stale_judge_forwards() {
        let base = Instant::now();
        let head = Playhead::new();
        let retired = head.generation();
        head.next_generation();
        let current = head.generation();
        head.loaded(URI, 0, true, base);
        head.observe(current, &PlaybackEvent::Playing { position_ms: 0 }, base);
        head.loaded(URI, 0, true, base);

        assert_eq!(
            head.judge_unavailable(retired, at(base, 60_000)),
            Verdict::Forward,
            "a retired drain's failure is not this generation's verdict"
        );
        assert_eq!(
            head.judge_unavailable(current, at(base, 60_000)),
            Verdict::Suspect,
            "the same failure on the live generation is a lost link"
        );
    }
}
