//! The reads the library asks for, and the loads they point at.

use crate::message::Command;
use crate::route::Page;

use super::{Library, Load, albums_chosen, artists_chosen, listed};

impl Library {
    /// Marks a playlist as wanted and returns the command that loads it.
    ///
    /// It never touches the history: Back and Forward move the page first and
    /// only reload what they land on.
    pub(super) fn load(&mut self, playlist: &str) -> Command {
        self.wanted = Some(playlist.to_owned());
        self.tracks = Load::Loading;
        // A navigation to a playlist whose cover is due doubles as its ask: the
        // page read answers the cover too, so no second read is issued.
        self.cover_refresh.asked(playlist);
        Command::LoadTracks {
            playlist: playlist.to_owned(),
        }
    }

    /// Marks an album as wanted and returns the command that loads it.
    ///
    /// Like [`Self::load`] it never touches the history: Back and Forward move
    /// the page first and only reload what they land on.
    fn load_album(&mut self, album: &str) -> Command {
        self.wanted_album = Some(album.to_owned());
        self.album = Load::Loading;
        Command::LoadAlbum {
            album: album.to_owned(),
        }
    }

    /// Marks a show as wanted and returns the command that loads it.
    ///
    /// Like [`Self::load`] and [`Self::load_album`] it never touches the
    /// history: Back and Forward move the page first and only reload what they
    /// land on.
    fn load_show(&mut self, show: &str) -> Command {
        self.wanted_show = Some(show.to_owned());
        self.show = Load::Loading;
        Command::LoadShow {
            show: show.to_owned(),
        }
    }

    /// Points the artist page at `artist` and returns the command that loads it.
    ///
    /// Like [`Self::load`] it never touches the history. It also opens the
    /// Follow state and the discography filter at this artist, so a label
    /// confirmed for another one cannot be drawn over this page and another
    /// artist's filter choice is not carried into this one.
    fn load_artist(&mut self, artist: &str) -> Command {
        if self.wanted_artist.as_deref() != Some(artist) {
            // Another artist's likeness list is no use here.
            self.related = Load::Idle;
            self.wanted_related = None;
        }
        self.wanted_artist = Some(artist.to_owned());
        self.artist = Load::Loading;
        self.follow.open(artist);
        self.filter.open(artist);
        Command::LoadArtist {
            artist: artist.to_owned(),
        }
    }

    /// Returns the command the current page needs, if any.
    ///
    /// A page whose wanted load is in flight or ready is left alone; any other
    /// page of a loadable kind is reloaded through its own loader. A failed load
    /// therefore reloads, which is what makes the Retry control and a Back onto a
    /// failed page the same request. A page never matches another one's want, so
    /// Back onto one whose load was superseded asks again rather than showing
    /// nothing.
    pub(super) fn settle(&mut self) -> Option<Command> {
        match self.history.current() {
            Some(Page::Playlist(playlist)) => {
                let playlist = playlist.clone();
                if self.wanted.as_deref() == Some(playlist.as_str())
                    && matches!(self.tracks, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load(&playlist))
            }
            Some(Page::Album(album)) => {
                let album = album.clone();
                if self.wanted_album.as_deref() == Some(album.as_str())
                    && matches!(self.album, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load_album(&album))
            }
            Some(Page::Artist(artist)) => {
                let artist = artist.clone();
                if self.wanted_artist.as_deref() == Some(artist.as_str())
                    && matches!(self.artist, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load_artist(&artist))
            }
            Some(Page::Show(show)) => {
                let show = show.clone();
                if self.wanted_show.as_deref() == Some(show.as_str())
                    && matches!(self.show, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load_show(&show))
            }
            // A search page is never fetched by `settle`: its read belongs to
            // the search state, and the shell asks for it from `logic` because
            // only the query, not the page, is what that read is about. A held
            // answer is what Back and Forward land on, so it is kept as it
            // stands; a dropped, idle or failed one is pointed at the page's
            // query again, which is what makes Back ask once. The page itself
            // never carries a load, so no command comes back from here.
            Some(Page::Search(query)) => {
                let query = query.clone();
                if matches!(
                    self.search.page_answer(&query),
                    Some(Load::Loading | Load::Ready(_))
                ) {
                    return None;
                }
                self.search.open_results(&query);
                None
            }
            // Neither the root nor Home is a read of its own: the root draws
            // the two library lists, and Home draws their settled state.
            None | Some(Page::Home) => None,
        }
    }

    /// Marks the playlists loading again and returns the command that fetches
    /// them.
    ///
    /// The followed lists restart with them: a playlist retry is the root's
    /// Retry, so every half of the page asks again. A reply already in flight
    /// from before the retry still lands, and its list is valid — only a little
    /// older than the retry's.
    pub fn reload(&mut self) -> Command {
        self.playlists = Load::Loading;
        self.followed = Load::Idle;
        self.followed_artists = Load::Idle;
        self.saved_albums = Load::Idle;
        self.likes.reload();
        // The retry is the page's own: it asks for the labels a failed read
        // left unanswered instead of keeping their misses.
        self.home.forget();
        Command::ReloadLibrary
    }

    /// The library read a failed link asks for, if one is due.
    ///
    /// Want-driven like the followed lists: the update carries no command, so it
    /// is asked for after the drain, when the failed write is no longer pending
    /// and the read can therefore be adopted.
    pub fn rootlist_request(&mut self) -> Option<Command> {
        if self.write_in_flight() || !std::mem::take(&mut self.rootlist_stale) {
            return None;
        }
        Some(self.reload())
    }

    /// The command that reads a playlist's cover again, if one is due.
    ///
    /// A page whose playlist has no rows loaded is asked for itself: a write can
    /// leave the open page blank, and only the page's own read would ever fill
    /// it. That ask is the read the page needs anyway, so it goes through
    /// [`Self::load`] and doubles as the cover ask.
    ///
    /// A read for any other due playlist is a bare `LoadTracks`: its reply
    /// writes that playlist's cover only, because the rows are written for the
    /// wanted playlist alone. It therefore leaves the want and the page on
    /// screen exactly as they are. The page on screen is kept out of the asks
    /// only while its own read is in flight, because that reply answers its
    /// cover; a settled page has no reply coming, so its dropped cover must be
    /// asked for like any other.
    pub fn tracks_request(&mut self) -> Option<Command> {
        if let Some(Page::Playlist(playlist)) = self.history.current() {
            let playlist = playlist.clone();
            if listed(&self.playlists, &playlist, |entry| &entry.uri)
                && matches!(self.tracks, Load::Idle)
                && self.wanted.as_deref() != Some(playlist.as_str())
            {
                return Some(self.load(&playlist));
            }
        }
        let present: Vec<String> = match &self.playlists {
            Load::Ready(playlists) => playlists.iter().map(|entry| entry.uri.clone()).collect(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        };
        let keep = match (self.selected(), &self.tracks) {
            (Some(page), Load::Loading) if self.wanted.as_deref() == Some(page) => {
                Some(page.to_owned())
            }
            _ => None,
        };
        let playlist = self.cover_refresh.take_due(&present, keep.as_deref())?;
        Some(Command::LoadTracks { playlist })
    }

    /// The command that starts the likeness load, if one is due.
    ///
    /// The cards are the page's second read: they are asked for only once the
    /// artist's own detail has landed and named somebody, so the hero and the
    /// discography never wait on a dozen `Artist::get` calls. A page that names
    /// nobody asks for nothing.
    pub fn related_request(&mut self) -> Option<Command> {
        let Load::Ready(detail) = &self.artist else {
            return None;
        };
        let artist = detail.uri.clone();
        let uris = detail.related.clone();
        if uris.is_empty() {
            return None;
        }
        if matches!(self.related, Load::Loading)
            || (matches!(self.related, Load::Ready(_) | Load::Failed(_))
                && self.wanted_related.as_deref() == Some(artist.as_str()))
        {
            return None;
        }
        self.wanted_related = Some(artist.clone());
        self.related = Load::Loading;
        Some(Command::LoadRelated { artist, uris })
    }

    /// Marks the likeness list loading again, for the page's own Retry.
    pub fn reload_related(&mut self) -> Option<Command> {
        self.related = Load::Idle;
        self.related_request()
    }

    /// Marks the results page's read idle again and returns the command that
    /// fetches it, for the page's own Retry.
    ///
    /// The page is the only place that knows a Retry was pressed, and the read
    /// is want-driven, so this resets the read and asks for it in the same call
    /// rather than waiting a frame for `logic`'s next look. It asks for the
    /// page's own query even while the menu is open and due, which is what
    /// [`crate::search::Search::request_page`] is for.
    pub fn reload_search(&mut self) -> Option<Command> {
        let query = self.page_search_query().map(str::to_owned)?;
        self.search.retry_page(&query);
        self.search.request_page(Some(&query))
    }

    /// Marks the followed-shows list loading again and returns the command that
    /// fetches it.
    ///
    /// This is the list's own Retry, so it asks directly rather than going
    /// through the playlists.
    pub fn reload_followed(&mut self) -> Command {
        self.followed = Load::Loading;
        Command::LoadFollowedShows
    }

    /// The command that starts the followed-shows load, if one is due.
    ///
    /// The list is want-driven rather than chained onto the rootlist, so the
    /// worker's update stream is unchanged. It asks exactly when nothing has
    /// been asked for yet and the playlists can name a signed-in window; a
    /// window that never loads its playlists therefore never asks for shows.
    pub fn followed_request(&mut self) -> Option<Command> {
        if self.followed == Load::Idle && matches!(self.playlists, Load::Ready(_)) {
            self.followed = Load::Loading;
            Some(Command::LoadFollowedShows)
        } else {
            None
        }
    }

    /// Marks the followed-artists list loading again and returns the command
    /// that fetches it.
    ///
    /// This is the list's own Retry, so it asks directly rather than going
    /// through the playlists. A reply already in flight from before the retry
    /// still lands, and its list is valid — only a little older than the
    /// retry's.
    pub fn reload_followed_artists(&mut self) -> Command {
        self.followed_artists = Load::Loading;
        Command::LoadFollowedArtists
    }

    /// Marks the saved-albums list loading again and returns the command that
    /// fetches it.
    ///
    /// The followed-artists shape, one list over.
    pub fn reload_saved_albums(&mut self) -> Command {
        self.saved_albums = Load::Loading;
        Command::LoadSavedAlbums
    }

    /// The command that starts the followed-artists load, if one is due.
    ///
    /// Like the followed shows it is want-driven: it asks exactly when nothing
    /// has been asked for yet, the playlists can name a signed-in window, and a
    /// chip naming artists is the one the user is looking at.
    pub fn followed_artists_request(&mut self) -> Option<Command> {
        if self.followed_artists == Load::Idle
            && matches!(self.playlists, Load::Ready(_))
            && (artists_chosen(self.root_filter.chosen(), self.sidebar_kind.chosen())
                // Home's related shelf reads them, and the chip that would ask
                // for them is not on screen there.
                || self.on_home())
        {
            self.followed_artists = Load::Loading;
            Some(Command::LoadFollowedArtists)
        } else {
            None
        }
    }

    /// The command that starts the saved-albums load, if one is due.
    ///
    /// The followed-artists shape, one chip over. Home also wants the list,
    /// whatever chip the root was left on: its shelf draws albums, and the chip
    /// that would ask for them is not on screen to do it.
    pub fn saved_albums_request(&mut self) -> Option<Command> {
        if self.saved_albums == Load::Idle
            && matches!(self.playlists, Load::Ready(_))
            && (albums_chosen(self.root_filter.chosen(), self.sidebar_kind.chosen())
                || self.on_artist_page()
                || self.on_home())
        {
            self.saved_albums = Load::Loading;
            Some(Command::LoadSavedAlbums)
        } else {
            None
        }
    }

    /// Whether the page on screen draws a heart on releases it did not read.
    ///
    /// An artist page draws one per release, and only the saved-albums set can
    /// say what each one is.
    fn on_artist_page(&self) -> bool {
        matches!(self.page(), Some(crate::route::Page::Artist(_)))
    }

    /// The command that starts the liked-songs read, if one is due.
    ///
    /// It waits for the playlists, because a signed-out window has no set to
    /// read and no heart to draw; every later sweep is the set's own to ask for.
    pub fn likes_request(&mut self) -> Option<Command> {
        self.likes
            .request(matches!(self.playlists, Load::Ready(_)))
            .then_some(Command::LoadLikedTracks)
    }

    /// The label read Home's liked cards owe: asked once per URI, marked at send,
    /// and only on a settled pool.
    pub fn home_labels_request(&mut self) -> Option<Command> {
        if !self.on_home() {
            return None;
        }
        if !self.home_intake().settled() {
            return None;
        }
        let tracks = self.home_unlabelled();
        if tracks.is_empty() {
            return None;
        }
        self.home.mark_asked(&tracks);
        Some(Command::LabelSaved { tracks })
    }

    /// Flips the liked set and asks for the write, or changes nothing.
    ///
    /// The set mutates only when it has a change to send, so a refused or empty
    /// flip never leaves a write pending with nothing on the wire.
    pub fn set_liked(&mut self, tracks: Vec<String>, liked: bool) -> Option<Command> {
        let change = self.likes.set(&tracks, liked)?;
        Some(Command::SetLiked {
            tracks: change.tracks,
            liked: change.liked,
        })
    }
}
