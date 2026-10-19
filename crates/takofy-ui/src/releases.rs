//! The per-release track labels an artist's discography fills in as it scrolls.
//!
//! A release's tracklist is fetched only while the block is on screen, so this
//! store is a want-set rather than a returned command: the draw calls
//! [`Releases::look`] for every block it puts on screen, and the app drains
//! [`Releases::requests`] after the draw, the same way it drains the cover
//! store. A want cannot be dropped by the shell, because it is not a command.
//!
//! The release list is registered by [`Releases::retain`] when the artist lands.
//! That is also the only thing that re-arms a failed release: a failure is not
//! retried behind the user's back, so the frame after it does not spin.

use std::collections::HashMap;

use takofy_model::{Release, TrackEntry};

use crate::message::{Command, Failure, Update};

/// The most label requests the window has outstanding at once.
///
/// Two, because the command loop is sequential: a discography is forty-odd
/// releases, and asking for all of them one after another would keep the queue
/// and the follow write behind a wall of round trips.
pub const MAX_IN_FLIGHT: usize = 2;

/// What a release block's body draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Labels<'a> {
    /// Nothing has been asked for this release yet.
    Idle,
    /// The label request is in flight.
    Loading,
    /// The rows are ready to draw, in the release's own order.
    Ready(&'a [TrackEntry]),
    /// The request failed; the block says so and does not retry by itself.
    Failed(Failure),
}

/// What the store knows about one release.
#[derive(Clone, PartialEq, Eq, Debug)]
enum State {
    Idle,
    Loading,
    Ready(Vec<TrackEntry>),
    Failed(Failure),
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Entry {
    /// The URIs a label request carries. Refreshed whenever the artist lands.
    tracks: Vec<String>,
    state: State,
}

/// The label state of every release the artist page holds.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Releases {
    entries: HashMap<String, Entry>,
    /// The releases looked at since the last drain, in look order.
    wanted: Vec<String>,
}

impl Releases {
    /// Registers the releases the page holds, dropping every other entry.
    ///
    /// A surviving entry keeps its labels, so revisiting an artist does not
    /// refetch what is already on screen. A failed entry is re-armed to `Idle`,
    /// which is what makes a reload the way out of a failure.
    pub fn retain(&mut self, releases: &[Release]) {
        let kept: std::collections::HashSet<&str> = releases
            .iter()
            .map(|release| release.uri.as_str())
            .collect();
        self.entries.retain(|uri, _| kept.contains(uri.as_str()));
        self.wanted.retain(|uri| kept.contains(uri.as_str()));

        for release in releases {
            match self.entries.get_mut(&release.uri) {
                Some(entry) => {
                    entry.tracks = release.tracks.clone();
                    if matches!(entry.state, State::Failed(_)) {
                        entry.state = State::Idle;
                    }
                }
                None => {
                    self.entries.insert(
                        release.uri.clone(),
                        Entry {
                            tracks: release.tracks.clone(),
                            state: State::Idle,
                        },
                    );
                }
            }
        }
    }

    /// Records that the block for `release` is on screen and reports its state.
    ///
    /// Only an `Idle` release is wanted: one in flight is already asked for, one
    /// that is ready needs nothing, and one that failed waits for a reload. A
    /// release this store does not know reads `Idle` and asks for nothing.
    pub fn look(&mut self, release: &str) -> Labels<'_> {
        let Some(entry) = self.entries.get(release) else {
            return Labels::Idle;
        };
        let idle = matches!(entry.state, State::Idle);
        if idle && !self.wanted.iter().any(|wanted| wanted == release) {
            self.wanted.push(release.to_owned());
        }
        match &self.entries[release].state {
            State::Idle => Labels::Idle,
            State::Loading => Labels::Loading,
            State::Ready(tracks) => Labels::Ready(tracks),
            State::Failed(failure) => Labels::Failed(*failure),
        }
    }

    /// The label requests to send after this frame's draw.
    ///
    /// At most [`MAX_IN_FLIGHT`] are outstanding in total. A want that does not
    /// fit is dropped rather than queued: the block is still on screen and
    /// re-looks next frame, so it is asked for as soon as a slot frees.
    pub fn requests(&mut self) -> Vec<Command> {
        let mut in_flight = self
            .entries
            .values()
            .filter(|entry| matches!(entry.state, State::Loading))
            .count();
        let mut commands = Vec::new();
        for uri in std::mem::take(&mut self.wanted) {
            if in_flight >= MAX_IN_FLIGHT {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&uri) else {
                continue;
            };
            if !matches!(entry.state, State::Idle) {
                continue;
            }
            entry.state = State::Loading;
            in_flight += 1;
            commands.push(Command::LoadReleaseTracks {
                release: uri,
                tracks: entry.tracks.clone(),
            });
        }
        commands
    }

    /// Folds one worker update into the state.
    ///
    /// A reply lands only on the release it names, and only while that release's
    /// request is in flight. Anything else is stale: the page moved on, or the
    /// store already gave up on it.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::ReleaseTracks { release, tracks } => {
                if let Some(entry) = self.entries.get_mut(release)
                    && matches!(entry.state, State::Loading)
                {
                    entry.state = State::Ready(tracks.clone());
                }
            }
            Update::ReleaseTracksFailed { release, failure } => {
                if let Some(entry) = self.entries.get_mut(release)
                    && matches!(entry.state, State::Loading)
                {
                    entry.state = State::Failed(*failure);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(uri: &str, tracks: &[&str]) -> Release {
        Release {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            kind: takofy_model::ReleaseKind::Album,
            year: None,
            cover: None,
            tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
        }
    }

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn load(release: &str, tracks: &[&str]) -> Command {
        Command::LoadReleaseTracks {
            release: release.to_owned(),
            tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
        }
    }

    fn store(releases: &[Release]) -> Releases {
        let mut store = Releases::default();
        store.retain(releases);
        store
    }

    #[test]
    fn a_look_records_a_want_and_still_reads_idle() {
        let mut store = store(&[release("a", &["t1"])]);
        assert_eq!(store.look("a"), Labels::Idle);
        assert_eq!(store.wanted, vec!["a".to_owned()]);
    }

    #[test]
    fn a_look_at_an_unknown_release_asks_for_nothing() {
        let mut store = store(&[release("a", &["t1"])]);
        assert_eq!(store.look("b"), Labels::Idle);
        assert!(store.requests().is_empty());
    }

    #[test]
    fn the_drain_emits_one_request_and_marks_it_in_flight() {
        let mut store = store(&[release("a", &["t1", "t2"])]);
        store.look("a");
        assert_eq!(store.requests(), vec![load("a", &["t1", "t2"])]);
        assert_eq!(store.look("a"), Labels::Loading);
    }

    #[test]
    fn a_release_in_flight_is_not_asked_for_again() {
        let mut store = store(&[release("a", &["t1"])]);
        store.look("a");
        let _ = store.requests();
        store.look("a");
        assert!(store.requests().is_empty());
    }

    #[test]
    fn a_ready_release_is_never_asked_for_again() {
        let mut store = store(&[release("a", &["t1"])]);
        store.look("a");
        let _ = store.requests();
        store.apply(&Update::ReleaseTracks {
            release: "a".to_owned(),
            tracks: vec![track("t1")],
        });

        assert_eq!(store.look("a"), Labels::Ready(&[track("t1")]));
        assert!(store.requests().is_empty());
    }

    #[test]
    fn a_reply_lands_only_on_the_release_it_names() {
        let mut store = store(&[release("a", &["t1"]), release("b", &["t2"])]);
        store.look("a");
        let _ = store.requests();

        store.apply(&Update::ReleaseTracks {
            release: "b".to_owned(),
            tracks: vec![track("t2")],
        });
        assert_eq!(store.look("b"), Labels::Idle, "B was never asked for");
        assert_eq!(store.look("a"), Labels::Loading);
    }

    #[test]
    fn a_reply_for_a_release_that_is_not_in_flight_is_dropped() {
        let mut store = store(&[release("a", &["t1"])]);
        store.apply(&Update::ReleaseTracks {
            release: "a".to_owned(),
            tracks: vec![track("t1")],
        });
        assert_eq!(store.look("a"), Labels::Idle);

        store.look("a");
        let _ = store.requests();
        store.apply(&Update::ReleaseTracks {
            release: "gone".to_owned(),
            tracks: vec![track("x")],
        });
        assert_eq!(
            store.look("a"),
            Labels::Loading,
            "the stray reply is not A's"
        );
    }

    #[test]
    fn a_failure_is_recorded_and_not_retried_until_the_page_reloads() {
        let mut store = store(&[release("a", &["t1"])]);
        store.look("a");
        let _ = store.requests();
        store.apply(&Update::ReleaseTracksFailed {
            release: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(store.look("a"), Labels::Failed(Failure::Load));
        assert!(
            store.requests().is_empty(),
            "a failure is not retried by a look"
        );

        store.retain(&[release("a", &["t1"])]);
        assert_eq!(store.look("a"), Labels::Idle, "a reload re-arms it");
        assert_eq!(store.requests(), vec![load("a", &["t1"])]);
    }

    #[test]
    fn the_drain_caps_at_two_and_frees_a_slot_per_answer() {
        let releases = [
            release("a", &["t1"]),
            release("b", &["t2"]),
            release("c", &["t3"]),
            release("d", &["t4"]),
        ];
        let mut store = store(&releases);
        for entry in &releases {
            store.look(&entry.uri);
        }
        assert_eq!(
            store.requests(),
            vec![load("a", &["t1"]), load("b", &["t2"])],
            "the cap holds at MAX_IN_FLIGHT"
        );
        assert!(
            store.requests().is_empty(),
            "the leftover wants are re-looked"
        );

        store.apply(&Update::ReleaseTracks {
            release: "a".to_owned(),
            tracks: vec![track("t1")],
        });
        for entry in &releases {
            store.look(&entry.uri);
        }
        assert_eq!(
            store.requests(),
            vec![load("c", &["t3"])],
            "one answer frees exactly one slot"
        );
    }

    #[test]
    fn retain_drops_the_releases_the_page_no_longer_holds() {
        let mut store = store(&[release("a", &["t1"]), release("b", &["t2"])]);
        store.look("a");
        let _ = store.requests();

        store.retain(&[release("b", &["t2"])]);
        assert!(!store.entries.contains_key("a"));
        assert_eq!(store.look("a"), Labels::Idle);
        assert!(store.requests().is_empty());
    }

    #[test]
    fn retain_refreshes_a_releases_tracks() {
        let mut store = store(&[release("a", &["t1"])]);
        store.look("a");
        let _ = store.requests();
        store.apply(&Update::ReleaseTracks {
            release: "a".to_owned(),
            tracks: vec![track("t1")],
        });

        store.retain(&[release("a", &["t1", "t9"])]);
        assert_eq!(
            store.look("a"),
            Labels::Ready(&[track("t1")]),
            "a ready release keeps its labels"
        );
        assert_eq!(store.entries["a"].tracks, vec!["t1", "t9"]);
    }
}
