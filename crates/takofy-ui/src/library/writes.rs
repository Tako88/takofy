//! The writes in flight, and the rows and counts they hold while they land.

use takofy_model::{CoverOrigin, Revision};

use crate::cover_refresh::first_track_uri;
use crate::message::Command;

use super::{Library, Load};

impl Library {
    /// Ends the page write for `playlist` and holds `revision` for the next one.
    pub(super) fn end_page_write(&mut self, playlist: &str, revision: &Revision) -> bool {
        if !self.writing(playlist) {
            return false;
        }
        self.pending = None;
        if let Load::Ready(tracks) = &mut self.tracks
            && tracks.playlist == *playlist
        {
            tracks.notice = None;
            if let Some(access) = &mut tracks.access {
                access.revision = revision.clone();
            }
        }
        if let Load::Ready(playlists) = &mut self.playlists
            && let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == *playlist)
            && let Some(access) = &mut entry.access
        {
            access.revision = revision.clone();
        }
        true
    }

    /// Drops `playlist`'s derived cover and queues the read that replaces it.
    ///
    /// A chosen picture is the user's own, so no row edit invalidates it. The
    /// cached context image goes with the cover it was drawn from, and the ask
    /// is only queued: one drain turns it into a read.
    pub(super) fn drop_derived_cover(&mut self, playlist: &str) {
        let entry = match &mut self.playlists {
            Load::Ready(playlists) => playlists.iter_mut().find(|entry| entry.uri == playlist),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        };
        if entry.as_ref().is_some_and(|entry| {
            entry
                .cover
                .as_ref()
                .is_some_and(|cover| cover.origin == CoverOrigin::Chosen)
        }) {
            return;
        }
        // The page's own copy goes too: it is the reply's cover, taken from the
        // first track, so a page headed by a read that names no picture of its
        // own would keep drawing the picture that track no longer gives.
        if let Load::Ready(tracks) = &mut self.tracks
            && tracks.playlist == *playlist
            && tracks
                .head
                .as_ref()
                .is_none_or(|head| head.picture.is_none())
        {
            tracks.cover = None;
        }
        self.context_covers.remove(playlist);
        if let Some(entry) = entry {
            entry.cover = None;
            entry.composed.clear();
        }
        self.cover_refresh.drop_for(playlist);
    }

    /// The first track of the page write in flight for `playlist`, before and
    /// after the write.
    ///
    /// Both are read before the reply ends the write, because that is when the
    /// rows it held are dropped. A reply with no write pending names no row, so
    /// it can change nothing.
    pub(super) fn pending_first_tracks(&self, playlist: &str) -> (Option<String>, Option<String>) {
        let Some(pending) = self
            .pending
            .as_ref()
            .filter(|pending| pending.playlist == playlist)
        else {
            return (None, None);
        };
        (
            first_track_uri(&pending.before).map(str::to_owned),
            first_track_uri(&pending.after).map(str::to_owned),
        )
    }

    /// Brings the artist list in step with an answer about one artist.
    ///
    /// The list is a whole set read from the server, so an answer that disagrees
    /// with it is the only thing worth acting on: a row the server says is gone
    /// goes, and one it says should be there means the set is stale, which only a
    /// read can fix. An answer that agrees changes nothing, so a page's opening
    /// probe costs nothing.
    pub(super) fn settle_follow(&mut self, artist: &str, followed: bool) {
        // Recorded before the list is consulted, because the card's tile reads
        // it whether or not the followed list is ready to answer.
        if followed {
            self.home.discovery.note_follow(artist);
        } else {
            self.home.discovery.note_unfollow(artist);
        }
        let Load::Ready(artists) = &self.followed_artists else {
            return;
        };
        let held = artists.iter().any(|held| held.uri == artist);
        match (followed, held) {
            (false, true) => {
                if let Load::Ready(artists) = &mut self.followed_artists {
                    artists.retain(|held| held.uri != artist);
                }
            }
            // A row the list does not hold needs its name and portrait, which
            // only the read brings. The rows already on screen stay while that
            // read is in flight, so nothing drawn from the list loses its place.
            (true, false) => {
                self.artists_stale = true;
            }
            _ => {}
        }
    }

    /// Lowers the sidebar's declared count for `playlist` by `gone` rows.
    pub(super) fn lower_page_count(&mut self, playlist: &str, gone: usize) {
        if gone == 0 {
            return;
        }
        if let Load::Ready(playlists) = &mut self.playlists
            && let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == playlist)
        {
            entry.tracks = entry.tracks.map(|count| count.saturating_sub(gone));
        }
    }

    /// The follow or unfollow write a view asked for, if it may go out.
    ///
    /// One write per artist at a time: the artist page's button holds the artist
    /// it names, and a card's tile holds it until its own answer lands.
    pub fn ask_follow(&mut self, artist: &str, followed: bool) -> Option<Command> {
        if self.follow.artist() == Some(artist) && self.follow.pending() {
            return None;
        }
        if self.follow_writes.contains_key(artist) {
            return None;
        }
        self.follow_writes.insert(artist.to_owned(), followed);
        Some(Command::SetFollow {
            artist: artist.to_owned(),
            followed,
        })
    }

    /// Whether a follow write is in flight for `artist`.
    pub fn follow_pending(&self, artist: &str) -> bool {
        self.follow_writes.contains_key(artist)
    }

    /// Whether a write for this playlist is in flight.
    pub(super) fn writing(&self, playlist: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.playlist == playlist)
    }

    /// Whether any write is in flight, on the page or in the sidebar.
    ///
    /// A menu opened over a write would act on rows that write is already
    /// replacing, so it waits for the reply.
    pub fn write_in_flight(&self) -> bool {
        self.pending.is_some() || self.sidebar_pending.is_some()
    }
}
