//! Whether a track, album, show, playlist or artist is saved, as the last
//! read or write said.

use takofy_model::uri::{UriKind, parse};

use crate::message::Command;

use super::{Library, Load, SidebarPending, listed};

impl Library {
    /// Whether a saved write of `uri` is in flight, in this direction.
    ///
    /// A page's own write asks this before sending: two writes for one saved
    /// thing would land in order and leave its row and its label disagreeing.
    pub fn saved_pending(&self, uri: &str, saved: bool) -> bool {
        matches!(
            &self.sidebar_pending,
            Some(SidebarPending::Saved { uri: pending, saved: pending_saved })
                if pending == uri && *pending_saved == saved
        )
    }

    /// Whether the library has `uri`, as the last read or the last write said.
    ///
    /// The write's echo outlives the list it would change: an add names no card,
    /// so the list cannot gain the row until its next read, and the heart would
    /// fall back off in between.
    pub fn is_saved(&self, uri: &str) -> Option<bool> {
        if let Some((_, saved)) = self.saved_known.iter().find(|(known, _)| known == uri) {
            return Some(*saved);
        }
        let kind = parse(uri)?.kind;
        let (here, ready) = match kind {
            UriKind::Album => (
                listed(&self.saved_albums, uri, |album| &album.uri),
                matches!(self.saved_albums, Load::Ready(_)),
            ),
            UriKind::Show => (
                listed(&self.followed, uri, |show| &show.uri),
                matches!(self.followed, Load::Ready(_)),
            ),
            UriKind::Playlist => (
                listed(&self.playlists, uri, |entry| &entry.uri),
                !matches!(self.playlists, Load::Idle | Load::Loading),
            ),
            // An artist's entry in the library is its follow, and the page's own
            // state is the only thing that knows it before the list is read.
            UriKind::Artist => (
                listed(&self.followed_artists, uri, |artist| &artist.uri)
                    || (self.follow.artist() == Some(uri) && self.follow.confirmed() == Some(true)),
                !matches!(self.followed_artists, Load::Idle),
            ),
            _ => return None,
        };
        if here {
            return Some(true);
        }
        ready.then_some(false)
    }

    /// Records one URI's place in the library, replacing an older answer.
    pub(super) fn remember_saved(&mut self, uri: &str, saved: bool) {
        self.saved_known.retain(|(known, _)| known != uri);
        self.saved_known.push((uri.to_owned(), saved));
        // A page can only be one place at a time, so one answer is enough to keep.
        if self.saved_known.len() > 64 {
            self.saved_known.remove(0);
        }
    }

    /// Asks whether the page's own album or show is in the library.
    ///
    /// The lists cannot answer this: a saved album is listed only after the
    /// albums read has run, which happens when that chip is chosen. A band's
    /// heart would therefore be inert on every page opened from search or from
    /// another artist.
    pub fn saved_request(&mut self) -> Option<Command> {
        let uri = match self.page()? {
            crate::route::Page::Album(uri) | crate::route::Page::Show(uri) => uri.clone(),
            _ => return None,
        };
        if self.saved_known.iter().any(|(known, _)| *known == uri) {
            return None;
        }
        if self.saved_probe.as_deref() == Some(uri.as_str()) {
            return None;
        }
        self.saved_probe = Some(uri.clone());
        Some(Command::LoadSaved { uri })
    }

    /// Ends a saved write: a removal drops the row, an add remembers it.
    pub(super) fn end_saved(&mut self, uri: &str, saved: bool) {
        if !self.saved_pending(uri, saved) {
            return;
        }
        self.sidebar_pending = None;
        self.sidebar_notice = None;
        self.remember_saved(uri, saved);
        if saved {
            // An add names no card, so the list cannot gain the row on its own.
            // Re-arming the read is what puts it in the sidebar; the next visit to
            // the chip that lists this kind draws it there.
            match parse(uri).map(|parsed| parsed.kind) {
                Some(UriKind::Album) => self.saved_albums = Load::Idle,
                Some(UriKind::Show) => self.followed = Load::Idle,
                _ => {}
            }
            return;
        }
        if let Load::Ready(albums) = &mut self.saved_albums {
            albums.retain(|album| album.uri != uri);
        }
        if let Load::Ready(shows) = &mut self.followed {
            shows.retain(|show| show.uri != uri);
        }
        if let Load::Ready(artists) = &mut self.followed_artists {
            artists.retain(|artist| artist.uri != uri);
        }
    }
}

#[cfg(test)]
mod tests;
