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

use takofy_model::LikedTrack;
use takofy_model::uri::{UriKind, parse};

use crate::home::Phase;
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
    /// The whole set, as the read found it, in the read's order.
    Set(Vec<LikedTrack>),
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
    /// The last adopted read's tracks with their dates, in the read's order.
    recent: Vec<LikedTrack>,
    /// Whether `set` is last session's copy rather than a read.
    restored: bool,
    /// The URIs a write confirmed as liked since the last adopted read, in the
    /// order they were confirmed.
    fresh: Vec<String>,
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
    /// Adopts last session's copy, to draw until a read replaces it.
    ///
    /// The copy is drawn but not trusted: no heart, no standing and no write
    /// may act on a set that another session's writes have moved since.
    pub fn restored(tracks: Vec<LikedTrack>) -> Self {
        Self {
            set: Some(tracks.iter().map(|track| track.uri.clone()).collect()),
            recent: tracks,
            restored: true,
            ..Self::default()
        }
    }

    /// Whether the set on screen is last session's copy.
    pub fn is_restored(&self) -> bool {
        self.restored
    }

    /// Whether a write is in flight against the set.
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The liked rows the library holds: the session's own likes first, then
    /// the last read's, without the tracks that read no longer holds.
    ///
    /// `None` until a read lands or a copy is restored: an empty answer would
    /// say the library likes nothing, which only a read can say.
    pub fn kept(&self) -> Option<Vec<LikedTrack>> {
        let set = self.set.as_ref()?;
        let mut kept: Vec<LikedTrack> = Vec::new();
        for uri in &self.fresh {
            if !set.contains(uri) {
                continue;
            }
            // A like the read names keeps the date it was read with.
            let saved_at = self
                .recent
                .iter()
                .find(|track| track.uri == *uri)
                .and_then(|track| track.saved_at);
            kept.push(LikedTrack {
                uri: uri.clone(),
                saved_at,
            });
        }
        for track in &self.recent {
            if set.contains(&track.uri) && !kept.iter().any(|held| held.uri == track.uri) {
                kept.push(track.clone());
            }
        }
        Some(kept)
    }

    /// Drops the restored copy, leaving a set a read landed alone.
    pub fn discard(&mut self) {
        if !self.restored {
            return;
        }
        self.restored = false;
        self.set = None;
        self.recent.clear();
    }

    /// Whether `uri` is a liked track, or `None` when the set cannot say.
    ///
    /// Only a track has a heart: an episode and a local file are in no liked
    /// set, and the collection set's albums are a different answer.
    pub fn state(&self, uri: &str) -> Option<bool> {
        if self.restored || !is_track(uri) {
            return None;
        }
        self.set.as_ref().map(|set| set.contains(uri))
    }

    /// The last read's tracks that are still liked, in the read's order.
    ///
    /// `None` until a read lands. A track that is no longer liked is left out
    /// without a write of its own, and a failed write that reverts brings it
    /// back the same way.
    pub fn recent(&self) -> Option<impl Iterator<Item = &LikedTrack>> {
        let set = self.set.as_ref()?;
        Some(
            self.recent
                .iter()
                .filter(move |track| set.contains(&track.uri)),
        )
    }

    /// Whether the set holds any track at all.
    ///
    /// This is the set, not the last read's tracks: a song liked this session
    /// is in the set but not in `recent`, and that user must not be told that
    /// nothing is saved.
    pub fn any(&self) -> bool {
        self.set.as_ref().is_some_and(|set| !set.is_empty())
    }

    /// What Home's shelf needs to know about the liked songs.
    ///
    /// A failed read reports itself even when a set is held; the shelf's own
    /// pool decides whether that set still draws cards. A read that answered is
    /// arrived whether or not it adopted a set, and a held set whose next read
    /// is due is arrived too, because there is still data to draw. Only a set
    /// that has never been read is pending.
    pub fn phase(&self) -> Phase {
        match self.read {
            Read::Failed(failure) => Phase::Failed(failure),
            Read::Current => Phase::Arrived,
            Read::Idle | Read::Stale if self.set.is_some() => Phase::Arrived,
            Read::Idle | Read::Stale => Phase::Pending,
        }
    }

    /// What `uris` have in common, as the menu's save row reads it.
    pub fn standing(&self, uris: &[&str]) -> Standing {
        if self.restored {
            return Standing::Unknown;
        }
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
        if self.restored || self.pending.is_some() || self.set.is_none() {
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
    pub fn request(&mut self, session_ready: bool) -> bool {
        if self.asking.is_some() || !session_ready {
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

    /// Ends the sentence a failed write left behind, without re-arming the read
    /// that heals it.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    fn adopt(&mut self, tracks: Vec<LikedTrack>) {
        let Some(issued) = self.asking.take() else {
            return;
        };
        if self.pending.is_some() {
            // A write is in flight and its answer is the truth; this read may
            // predate it. Its own failure would mark the set stale again.
            self.read = Read::Current;
            return;
        }
        if issued < self.epoch {
            // The read predates a flip. Ask once more rather than adopting an
            // older truth; the next reply is issued at the new epoch.
            self.read = Read::Stale;
            return;
        }
        self.set = Some(tracks.iter().map(|track| track.uri.clone()).collect());
        self.recent = tracks;
        self.restored = false;
        self.fresh.clear();
        self.read = Read::Current;
        self.notice = None;
    }

    fn fail_read(&mut self, failure: Failure) {
        let Some(issued) = self.asking.take() else {
            return;
        };
        // A read issued before a later flip is the one a failed write already
        // asked to heal: its failure must not bury that request. The epoch only
        // moves on a flip, so a failure at the current epoch still stops.
        self.read = if issued < self.epoch {
            Read::Stale
        } else {
            Read::Failed(failure)
        };
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
        for uri in tracks {
            self.fresh.retain(|held| held != uri);
            if liked {
                self.fresh.push(uri.clone());
            }
        }
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
    parse(uri).is_some_and(|parsed| parsed.kind == UriKind::Track)
}

/// Whether two picks name the same tracks, whatever their order.
fn same_tracks(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|uri| b.contains(uri))
}

#[cfg(test)]
mod tests;
