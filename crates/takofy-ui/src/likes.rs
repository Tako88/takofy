//! The liked-songs set, and the one write in flight against it.
//!
//! A heart flips on the click and the write follows; a 2xx keeps the flip, an
//! error puts it back and marks the set stale so one read can heal it. Every
//! read is issued at the epoch of the last local flip, and a read that was
//! issued before a later flip — or that lands while one is still in flight — is
//! dropped rather than adopted: the set it carries is older than the heart on
//! screen.
//!
//! It imports no I/O: the bridge folds its events in and the shell sends the
//! one command a request asks for.

use std::collections::HashSet;

use crate::message::Failure;

/// What a pick's members have in common, as the menu's save row reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Standing {
    /// Nothing is known: no read has landed, or the pick holds no track.
    #[default]
    Unknown,
    /// None of the pick is saved.
    Unsaved,
    /// All of the pick is saved.
    Saved,
    /// Some of it is saved and some is not.
    Mixed,
}

/// One flip of the set, as the write must name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The URIs the write names, in the pick's order and each once.
    pub tracks: Vec<String>,
    /// What the write asks for.
    pub liked: bool,
}

/// What the bridge reports about the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The whole set, as the read found it.
    Set(Vec<String>),
    /// The read failed; a set already read is kept.
    SetFailed(Failure),
    /// The write landed for these tracks, with this value.
    Confirmed { tracks: Vec<String>, liked: bool },
    /// The write failed for these tracks.
    Failed {
        tracks: Vec<String>,
        failure: Failure,
    },
}

/// How the set was last read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Read {
    /// Nothing has been asked for.
    #[default]
    Idle,
    /// The set is the read's own.
    Current,
    /// A read is due: a reload, or a write that failed.
    Stale,
    /// The last read failed and is not retried until a reload.
    Failed(Failure),
}

/// The liked tracks, and the one write in flight against them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Likes {
    /// The last adopted set. `None` until a read lands.
    set: Option<HashSet<String>>,
    read: Read,
    /// The epoch of the last local flip or revert.
    epoch: u64,
    /// The epoch the in-flight read was issued at.
    asking: Option<u64>,
    /// The write in flight, and what it would put back.
    pending: Option<Change>,
    /// Why the last write failed, until a read issued after it lands.
    notice: Option<Failure>,
}

impl Likes {
    /// Whether `uri` is a liked track, or `None` when the set cannot say.
    ///
    /// Only a track has a heart: an episode and a local file are in no liked
    /// set, and the collection set's albums are a different answer.
    pub fn state(&self, uri: &str) -> Option<bool> {
        if !is_track(uri) {
            return None;
        }
        self.set.as_ref().map(|set| set.contains(uri))
    }

    /// What `uris` have in common, as the menu's save row reads it.
    pub fn standing(&self, uris: &[&str]) -> Standing {
        let Some(set) = self.set.as_ref() else {
            return Standing::Unknown;
        };
        if uris.is_empty() || uris.iter().any(|uri| !is_track(uri)) {
            return Standing::Unknown;
        }
        let liked = uris.iter().filter(|uri| set.contains(**uri)).count();
        match liked {
            0 => Standing::Unsaved,
            n if n == uris.len() => Standing::Saved,
            _ => Standing::Mixed,
        }
    }

    /// Flips the effective URIs of `uris` and records the write.
    ///
    /// A URI already at `liked` is left out: a mixed pick writes only the rows
    /// it would change. The filter reads the set as it is now, so a menu whose
    /// snapshot is older than the set cannot re-date a track that is already
    /// liked.
    pub fn set(&mut self, uris: &[String], liked: bool) -> Option<Change> {
        if self.pending.is_some() || self.set.is_none() {
            return None;
        }
        let mut seen = HashSet::new();
        let mut effective = Vec::new();
        for uri in uris {
            if !is_track(uri) {
                return None;
            }
            if !seen.insert(uri.clone()) {
                continue;
            }
            let current = self.set.as_ref().is_some_and(|set| set.contains(uri));
            if current != liked {
                effective.push(uri.clone());
            }
        }
        if effective.is_empty() {
            return None;
        }
        let set = self.set.as_mut()?;
        for uri in &effective {
            if liked {
                set.insert(uri.clone());
            } else {
                set.remove(uri);
            }
        }
        self.epoch = self.epoch.saturating_add(1);
        let change = Change {
            tracks: effective,
            liked,
        };
        self.pending = Some(change.clone());
        Some(change)
    }

    /// Folds one bridge event into the set.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::Set(tracks) => self.adopt(tracks.clone()),
            Event::SetFailed(failure) => self.fail_read(*failure),
            Event::Confirmed { tracks, liked } => self.confirm(tracks, *liked),
            Event::Failed { tracks, failure } => self.fail_write(tracks, *failure),
        }
    }

    /// Whether a read is due, and marks it in flight when it is.
    ///
    /// A failed read is not retried until a reload: a request loop behind a
    /// persistent failure would never stop.
    pub fn request(&mut self, playlists_ready: bool) -> bool {
        if self.asking.is_some() || !playlists_ready {
            return false;
        }
        match self.read {
            Read::Idle | Read::Stale => {
                self.asking = Some(self.epoch);
                true
            }
            Read::Current | Read::Failed(_) => false,
        }
    }

    /// Marks the set for a fresh read without hiding a heart that is already on
    /// screen, and re-arms a failed read.
    ///
    /// A write in flight is left alone: a failure afterwards still has to know
    /// what to revert.
    pub fn reload(&mut self) {
        match self.read {
            Read::Current => self.read = Read::Stale,
            Read::Failed(_) => self.read = Read::Idle,
            Read::Idle | Read::Stale => {}
        }
    }

    /// Why the last write failed, if it did.
    pub fn notice(&self) -> Option<Failure> {
        self.notice
    }

    fn adopt(&mut self, tracks: Vec<String>) {
        let Some(issued) = self.asking.take() else {
            return;
        };
        if self.pending.is_some() || issued < self.epoch {
            self.read = if self.set.is_some() {
                Read::Current
            } else {
                Read::Stale
            };
            return;
        }
        self.set = Some(tracks.into_iter().collect());
        self.read = Read::Current;
        self.notice = None;
    }

    fn fail_read(&mut self, failure: Failure) {
        if self.asking.take().is_none() {
            return;
        }
        self.read = Read::Failed(failure);
    }

    fn confirm(&mut self, tracks: &[String], liked: bool) {
        let Some(pending) = self.pending.as_ref() else {
            return;
        };
        if pending.liked != liked || !same_tracks(&pending.tracks, tracks) {
            return;
        }
        self.pending = None;
        self.notice = None;
        self.epoch = self.epoch.saturating_add(1);
    }

    fn fail_write(&mut self, tracks: &[String], failure: Failure) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        if !same_tracks(&pending.tracks, tracks) {
            return;
        }
        let Some(set) = self.set.as_mut() else {
            return;
        };
        for uri in &pending.tracks {
            if pending.liked {
                set.remove(uri);
            } else {
                set.insert(uri.clone());
            }
        }
        self.pending = None;
        self.notice = Some(failure);
        self.epoch = self.epoch.saturating_add(1);
        self.read = Read::Stale;
    }
}

/// Whether a URI names a track the collection set can hold.
fn is_track(uri: &str) -> bool {
    uri.strip_prefix("spotify:track:")
        .is_some_and(|id| !id.is_empty())
}

/// Whether two picks name the same tracks, whatever their order.
fn same_tracks(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|uri| b.contains(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "spotify:track:a";
    const B: &str = "spotify:track:b";
    const C: &str = "spotify:track:c";

    fn likes(tracks: &[&str]) -> Likes {
        let mut likes = Likes::default();
        assert!(likes.request(true));
        likes.apply(&Event::Set(
            tracks.iter().map(|uri| (*uri).to_owned()).collect(),
        ));
        likes
    }

    fn set(likes: &mut Likes, uris: &[&str], liked: bool) -> Option<Change> {
        let uris: Vec<String> = uris.iter().map(|uri| (*uri).to_owned()).collect();
        likes.set(&uris, liked)
    }

    #[test]
    fn a_new_set_knows_nothing() {
        let mut likes = Likes::default();
        assert_eq!(likes.state(A), None);
        assert_eq!(likes.standing(&[A]), Standing::Unknown);
        assert_eq!(set(&mut likes, &[A], true), None);
        assert_eq!(likes.notice(), None);
    }

    #[test]
    fn a_read_makes_the_set_ready_and_answers_per_uri() {
        let likes = likes(&[A, C]);
        assert_eq!(likes.state(A), Some(true));
        assert_eq!(likes.state(B), Some(false));
        assert_eq!(likes.state(C), Some(true));
    }

    #[test]
    fn only_track_uris_have_a_state() {
        for uri in [
            "spotify:episode:e",
            "spotify:album:a",
            "spotify:local:A:B:C:1000",
            "spotify:track:",
            "track:a",
            "",
        ] {
            assert_eq!(likes(&[A]).state(uri), None, "{uri:?}");
        }
    }

    #[test]
    fn set_flips_every_effective_uri_and_returns_one_change_in_order() {
        let mut likes = likes(&[]);
        assert_eq!(
            set(&mut likes, &[A, B], true),
            Some(Change {
                tracks: vec![A.to_owned(), B.to_owned()],
                liked: true,
            })
        );
        assert_eq!(likes.state(A), Some(true));
        assert_eq!(likes.state(B), Some(true));
    }

    #[test]
    fn a_pick_that_is_partly_liked_writes_only_the_unliked() {
        let mut likes = likes(&[A]);
        assert_eq!(
            set(&mut likes, &[A, B, C], true),
            Some(Change {
                tracks: vec![B.to_owned(), C.to_owned()],
                liked: true,
            }),
            "the already-liked row is not re-dated"
        );
        assert_eq!(likes.state(A), Some(true));
        assert_eq!(likes.state(B), Some(true));
    }

    #[test]
    fn a_set_that_changes_nothing_returns_none_and_stays_idle() {
        let mut likes = likes(&[A, B]);
        assert_eq!(set(&mut likes, &[A, B], true), None);
        assert_eq!(set(&mut likes, &[C], false), None);
        assert_eq!(
            set(&mut likes, &[A], false),
            Some(Change {
                tracks: vec![A.to_owned()],
                liked: false
            }),
            "nothing was stuck pending by the writes that changed nothing"
        );
    }

    #[test]
    fn a_second_set_while_one_is_pending_is_refused_and_changes_nothing() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[B], true).is_some());
        assert_eq!(set(&mut likes, &[C], true), None);
        assert_eq!(likes.state(C), Some(false));
    }

    #[test]
    fn duplicate_uris_in_a_pick_are_written_once() {
        let mut likes = likes(&[]);
        assert_eq!(
            set(&mut likes, &[A, A, B], true),
            Some(Change {
                tracks: vec![A.to_owned(), B.to_owned()],
                liked: true,
            })
        );
    }

    #[test]
    fn a_non_track_in_the_pick_refuses_the_whole_set() {
        let mut likes = likes(&[]);
        assert_eq!(set(&mut likes, &[A, "spotify:episode:e"], true), None);
        assert_eq!(likes.state(A), Some(false), "nothing was flipped");
        assert_eq!(
            set(&mut likes, &[A], true),
            Some(Change {
                tracks: vec![A.to_owned()],
                liked: true
            }),
            "the refusal stuck nothing pending"
        );
    }

    #[test]
    fn a_confirmed_write_ends_the_wait_and_clears_the_notice() {
        let mut likes = likes(&[A]);
        let change = set(&mut likes, &[B], true).expect("a change");
        likes.apply(&Event::Confirmed {
            tracks: change.tracks,
            liked: true,
        });
        assert_eq!(likes.notice(), None);
        assert!(
            set(&mut likes, &[C], true).is_some(),
            "the wait ended, so the next like is sent"
        );
    }

    #[test]
    fn a_confirmation_that_names_other_tracks_is_ignored() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[B], true).is_some());
        likes.apply(&Event::Confirmed {
            tracks: vec![C.to_owned()],
            liked: true,
        });
        assert_eq!(
            set(&mut likes, &[C], true),
            None,
            "the write is still the one in flight"
        );
    }

    #[test]
    fn a_failed_write_restores_every_uri_sets_save_and_marks_stale() {
        let mut likes = likes(&[A]);
        let change = set(&mut likes, &[A, B], false).expect("a change");
        assert_eq!(
            change.tracks,
            vec![A.to_owned()],
            "only the row the write would change is named"
        );
        assert_eq!(likes.state(A), Some(false), "the flip was optimistic");

        likes.apply(&Event::Failed {
            tracks: change.tracks,
            failure: Failure::Save,
        });
        assert_eq!(likes.state(A), Some(true), "the revert put A back");
        assert_eq!(likes.state(B), Some(false));
        assert_eq!(likes.notice(), Some(Failure::Save));
        assert!(likes.request(true), "a failed write asks for one read");
    }

    #[test]
    fn a_failure_that_names_other_tracks_is_ignored() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[B], true).is_some());
        likes.apply(&Event::Failed {
            tracks: vec![C.to_owned()],
            failure: Failure::Save,
        });
        assert_eq!(likes.notice(), None);
        assert_eq!(
            set(&mut likes, &[C], true),
            None,
            "the write is still the one in flight"
        );
    }

    #[test]
    fn request_emits_one_load_after_a_failure_and_then_waits_for_the_reply() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[B], true).is_some());
        let _ = likes.asking.take();
        likes.apply(&Event::Failed {
            tracks: vec![B.to_owned()],
            failure: Failure::Save,
        });
        assert!(!likes.request(false), "the playlists are not ready yet");
        assert!(likes.request(true));
        assert!(!likes.request(true), "one read is in flight");
        likes.apply(&Event::Set(vec![A.to_owned()]));
        assert!(!likes.request(true), "the read answered");
    }

    #[test]
    fn a_read_landing_during_a_pending_write_keeps_the_write() {
        let mut likes = likes(&[A]);
        likes.reload();
        assert!(likes.request(true));
        assert!(set(&mut likes, &[B], true).is_some());
        likes.apply(&Event::Set(vec![A.to_owned()]));
        assert_eq!(likes.state(B), Some(true), "the pending flip survived");
        assert_eq!(
            set(&mut likes, &[C], true),
            None,
            "and the write is still in flight"
        );
    }

    #[test]
    fn a_stale_read_landing_after_a_confirmed_like_keeps_the_like() {
        let mut likes = likes(&[]);
        likes.reload();
        assert!(likes.request(true), "the first sweep is in flight");
        let change = set(&mut likes, &[A], true).expect("a change");
        likes.apply(&Event::Confirmed {
            tracks: change.tracks,
            liked: true,
        });
        // The read was issued before the like, so it carries the truth from
        // before it: adopting it would wipe a confirmed like with no healing
        // read behind it.
        likes.apply(&Event::Set(vec![]));
        assert_eq!(likes.state(A), Some(true));
    }

    #[test]
    fn a_read_with_no_pending_write_replaces_the_set_wholesale() {
        let mut likes = likes(&[A]);
        likes.reload();
        assert!(likes.request(true));
        likes.apply(&Event::Set(vec![B.to_owned()]));
        assert_eq!(likes.state(A), Some(false));
        assert_eq!(likes.state(B), Some(true));
    }

    #[test]
    fn an_adopted_read_clears_the_notice() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[B], true).is_some());
        likes.apply(&Event::Failed {
            tracks: vec![B.to_owned()],
            failure: Failure::Save,
        });
        assert_eq!(likes.notice(), Some(Failure::Save));
        assert!(likes.request(true));
        likes.apply(&Event::Set(vec![A.to_owned()]));
        assert_eq!(likes.notice(), None);
    }

    #[test]
    fn a_failed_refresh_keeps_the_set() {
        let mut likes = likes(&[A]);
        likes.reload();
        assert!(likes.request(true));
        likes.apply(&Event::SetFailed(Failure::Load));
        assert_eq!(likes.state(A), Some(true), "a failed refresh hid a heart");
    }

    #[test]
    fn a_failed_first_read_is_failed_and_every_state_is_unknown() {
        let mut likes = Likes::default();
        assert!(likes.request(true));
        likes.apply(&Event::SetFailed(Failure::Load));
        assert_eq!(likes.state(A), None);
        assert_eq!(likes.standing(&[A]), Standing::Unknown);
        assert!(
            !likes.request(true),
            "a failed read is not retried every frame"
        );
    }

    #[test]
    fn reload_marks_a_ready_set_stale_without_hiding_any_heart() {
        let mut likes = likes(&[A]);
        likes.reload();
        assert_eq!(likes.state(A), Some(true));
        assert!(likes.request(true), "the reload asks for the set");
    }

    #[test]
    fn reload_rearms_a_failed_read() {
        let mut likes = Likes::default();
        assert!(likes.request(true));
        likes.apply(&Event::SetFailed(Failure::Load));
        likes.reload();
        assert!(likes.request(true));
    }

    #[test]
    fn reload_while_a_write_is_pending_keeps_the_wait() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[A], false).is_some());
        likes.reload();
        likes.apply(&Event::Failed {
            tracks: vec![A.to_owned()],
            failure: Failure::Save,
        });
        assert_eq!(
            likes.state(A),
            Some(true),
            "the reload dropped the write the failure had to revert"
        );
    }

    #[test]
    fn standing_of_a_pick() {
        let likes = likes(&[A, B]);
        assert_eq!(likes.standing(&[A, B]), Standing::Saved);
        assert_eq!(likes.standing(&[C]), Standing::Unsaved);
        assert_eq!(likes.standing(&[A, C]), Standing::Mixed);
        assert_eq!(likes.standing(&[]), Standing::Unknown);
        assert_eq!(likes.standing(&[A, "spotify:episode:e"]), Standing::Unknown);
        assert_eq!(Likes::default().standing(&[A]), Standing::Unknown);
    }

    #[test]
    fn pending_does_not_change_standing() {
        let mut likes = likes(&[A]);
        assert!(set(&mut likes, &[B], true).is_some());
        assert_eq!(likes.standing(&[A, B]), Standing::Saved);
    }

    #[test]
    fn the_first_request_waits_for_the_playlists() {
        let mut likes = Likes::default();
        assert!(!likes.request(false));
        assert!(likes.request(true));
    }
}
