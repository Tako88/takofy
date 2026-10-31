//! The copy the library draws before the session answers, and the marks that
//! say how far each list has been refreshed.
//!
//! A list holds the copy's rows and says they are last session's until the
//! session's own read replaces them. A failed refresh keeps those rows and
//! says they are old; a list the session read itself is replaced by its
//! failure.

use takofy_model::Account;

use crate::likes::Likes;
use crate::message::Failure;

use super::{Library, Load, Snapshot};

/// Where the rows a list draws came from, and what it is owed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// The session's own read, or no rows at all.
    #[default]
    Live,
    /// Last session's copy, which no refresh has been asked for yet.
    LastSession,
    /// Last session's copy, with a refresh in flight.
    Asked,
    /// Last session's copy, whose refresh failed.
    Unrefreshed(Failure),
}

/// One [`Source`] per list a copy holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Sources {
    pub(super) playlists: Source,
    pub(super) shows: Source,
    pub(super) artists: Source,
    pub(super) albums: Source,
}

/// Whether `source` says the rows on screen are the copy's.
fn copy(source: Source) -> bool {
    matches!(
        source,
        Source::LastSession | Source::Asked | Source::Unrefreshed(_)
    )
}

/// Whether a list holds the session's own rows.
pub(super) fn live<T>(load: &Load<T>, source: Source) -> bool {
    source == Source::Live && matches!(load, Load::Ready(_))
}

/// Re-arms a list's read.
///
/// The copy's rows stay on screen while the read is in flight; a list the
/// session read itself is put back to `fresh`, because the retry replaces it
/// and there is nothing of last session's to keep.
pub(super) fn reset<T>(load: &mut Load<T>, source: &mut Source, fresh: Load<T>) {
    if copy(*source) && matches!(load, Load::Ready(_)) {
        *source = if matches!(fresh, Load::Idle) {
            Source::LastSession
        } else {
            Source::Asked
        };
        return;
    }
    *load = fresh;
    *source = Source::Live;
}

/// Marks a list's read in flight for its own Retry.
///
/// The rows go: only a failed list offers that Retry, and a failure holds
/// nothing of the session's to keep.
pub(super) fn asked<T>(load: &mut Load<T>, source: &mut Source) {
    *load = Load::Loading;
    *source = if copy(*source) {
        Source::Asked
    } else {
        Source::Live
    };
}

/// Asks for a list once, keeping the copy's rows on screen.
///
/// A list the session has not read is asked for although it has rows: the copy
/// is what the read replaces. `false` means nothing is due.
pub(super) fn ask<T>(load: &mut Load<Vec<T>>, source: &mut Source) -> bool {
    if matches!(load, Load::Idle) {
        *load = Load::Loading;
        return true;
    }
    if *source != Source::LastSession {
        return false;
    }
    *source = Source::Asked;
    true
}

/// Records a failed read of a list.
///
/// A refresh that failed over the copy's rows keeps them and marks them old; a
/// list the session read is replaced by its failure.
pub(super) fn fail_list<T>(load: &mut Load<T>, source: &mut Source, failure: Failure) {
    if copy(*source) && matches!(load, Load::Ready(_)) {
        *source = Source::Unrefreshed(failure);
        return;
    }
    *load = Load::Failed(failure);
    *source = Source::Live;
}

/// Draws one list's rows from the copy, unless the session already read it.
fn draw<T>(load: &mut Load<Vec<T>>, source: &mut Source, rows: Option<Vec<T>>) {
    let Some(rows) = rows else {
        return;
    };
    if !matches!(load, Load::Idle) {
        return;
    }
    *load = Load::Ready(rows);
    *source = Source::LastSession;
}

/// Drops one list's rows because they came from a discarded copy.
fn drop_copy<T>(load: &mut Load<T>, source: &mut Source) {
    if copy(*source) {
        *load = Load::Idle;
        *source = Source::Live;
    }
}

impl Library {
    /// Draws `snapshot` until the session's own reads replace it, list by list.
    ///
    /// A list the session has already read is left alone, and an account the
    /// session named before the copy arrived has to be the copy's own.
    pub fn restore(&mut self, snapshot: Snapshot) {
        if self.account.is_some_and(|known| known != snapshot.account) {
            return;
        }
        self.copy_of = Some(snapshot.account);
        draw(
            &mut self.playlists,
            &mut self.sources.playlists,
            snapshot.playlists,
        );
        draw(&mut self.followed, &mut self.sources.shows, snapshot.shows);
        draw(
            &mut self.followed_artists,
            &mut self.sources.artists,
            snapshot.artists,
        );
        draw(
            &mut self.saved_albums,
            &mut self.sources.albums,
            snapshot.albums,
        );
        if self.likes.kept().is_none()
            && let Some(liked) = snapshot.liked
        {
            self.likes = Likes::restored(liked);
        }
    }

    /// The copy to save: every list the window holds, for the account it knows.
    ///
    /// `None` while the account is unknown and while a write is in flight: the
    /// rows on screen would name a list that write is already replacing. A list
    /// that is not ready to draw is left out rather than written as empty.
    pub fn snapshot(&self) -> Option<Snapshot> {
        if self.write_in_flight() || self.likes.pending() {
            return None;
        }
        let mut snapshot = Snapshot::new(self.account?);
        if let Load::Ready(playlists) = &self.playlists {
            snapshot.playlists = Some(playlists.clone());
        }
        if let Load::Ready(shows) = &self.followed {
            snapshot.shows = Some(shows.clone());
        }
        if let Load::Ready(artists) = &self.followed_artists {
            snapshot.artists = Some(artists.clone());
        }
        if let Load::Ready(albums) = &self.saved_albums {
            snapshot.albums = Some(albums.clone());
        }
        snapshot.liked = self.likes.kept();
        Some(snapshot)
    }

    /// Forgets the copy, dropping every row that came from it.
    ///
    /// A list the session read itself keeps its rows: they are this session's
    /// own answer, and a discarded copy says nothing about them.
    pub fn discard_copy(&mut self) {
        if self.copy_of.is_none() {
            return;
        }
        self.copy_of = None;
        drop_copy(&mut self.playlists, &mut self.sources.playlists);
        drop_copy(&mut self.followed, &mut self.sources.shows);
        drop_copy(&mut self.followed_artists, &mut self.sources.artists);
        drop_copy(&mut self.saved_albums, &mut self.sources.albums);
        self.likes.discard();
    }

    /// How many updates have changed what a copy would hold.
    ///
    /// A save waits for a quiet window after the last one, so a burst of updates
    /// costs one write.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether any list's refresh failed over the copy's rows.
    pub fn copy_failed(&self) -> bool {
        [
            self.sources.playlists,
            self.sources.shows,
            self.sources.artists,
            self.sources.albums,
        ]
        .into_iter()
        .any(|source| matches!(source, Source::Unrefreshed(_)))
    }

    /// Whether the playlists on screen are the session's own read.
    ///
    /// A read may only be sent for the playlists' rows, and a cover or a device
    /// watch may only name them, once this holds. For a session that restored
    /// nothing it is the playlists' readiness alone.
    pub fn playlists_live(&self) -> bool {
        live(&self.playlists, self.sources.playlists)
    }

    /// Whether any restored list is still waiting for its refresh.
    ///
    /// Home draws the copy, so its own reads start from live data: while a list
    /// the page builds from is still last session's, there is nothing to ask
    /// for. The playlists are excepted when their reply was dropped by a pending
    /// rootlist write, because that reply proves the session connected, and the
    /// re-ask that the drop owes is what reads the list again.
    pub(super) fn restored_waiting(&self) -> bool {
        // A failed playlists refresh is the connect failing: nothing else will
        // refresh the copy until a Retry, so Home must not wait on it.
        if matches!(self.sources.playlists, Source::Unrefreshed(_)) {
            return false;
        }
        fn waiting(source: Source) -> bool {
            matches!(source, Source::LastSession | Source::Asked)
        }
        let playlists =
            waiting(self.sources.playlists) && !(self.rootlist_dropped || self.rootlist_stale);
        playlists
            || waiting(self.sources.shows)
            || waiting(self.sources.artists)
            || waiting(self.sources.albums)
    }

    /// Records the account the session named, and checks it against the copy.
    ///
    /// The first account is the one the window keeps: a second, different one
    /// would be a bug somewhere else, and discarding the copy on it could drop
    /// rows the session already read.
    pub(super) fn learn_account(&mut self, account: Account) {
        if self.account.is_some_and(|known| known != account) {
            return;
        }
        self.account = Some(account);
        if self.copy_of.is_some_and(|copy| copy != account) {
            self.discard_copy();
        }
    }
}

#[cfg(test)]
mod tests;
