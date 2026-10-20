//! The load state for the playlists and their tracks.
//!
//! The library is a reducer over worker updates: it decides what is on screen
//! and which load a reply belongs to, and it hands the view a command when the
//! user asks for one.

use std::collections::HashMap;

use takofy_model::{
    AlbumDetail, ArtistDetail, CoverUrl, FollowedShow, PlaylistEntry, ShowDetail, TrackEntry,
};

use crate::discography::Filter;
use crate::follow::Follow;
use crate::library_filter::RootFilter;
use crate::message::{Command, Failure, Update};
use crate::releases::Releases;
use crate::route::{History, Page};
use crate::sidebar_kind::SidebarFilter;

/// What a loadable value is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load<T> {
    /// Nothing has been asked for.
    #[default]
    Idle,
    /// A request is in flight.
    Loading,
    /// The value is ready to draw.
    Ready(T),
    /// The request failed and may be retried.
    Failed(Failure),
}

/// The tracks of one playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracksFor {
    pub playlist: String,
    pub tracks: Vec<TrackEntry>,
}

/// What the playlist band draws for its cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandCover<'a> {
    /// The selected playlist's tracks are still loading, so the band waits
    /// rather than fetching a first-track cover it may replace with the
    /// context image a moment later.
    Pending,
    /// The cover the playlist entry names, if any.
    Entry(Option<&'a CoverUrl>),
}

/// The band's cover source: the playlist entry alone, once its tracks have
/// settled.
///
/// The band never derives a cover from the track list: the entry already holds
/// the picture or the first track's album cover, and a second derivation could
/// disagree with it.
pub fn band_cover(entry: &PlaylistEntry, settled: bool) -> BandCover<'_> {
    if settled {
        BandCover::Entry(entry.cover.as_ref())
    } else {
        BandCover::Pending
    }
}

/// The artist page's state, borrowed for one frame.
pub struct ArtistPage<'a> {
    /// The artist's load, so the page can draw its spinner or failure.
    pub load: &'a Load<ArtistDetail>,
    /// The loaded artist, only when it names the page on screen.
    pub current: Option<&'a ArtistDetail>,
    /// The Follow button's state.
    pub follow: &'a mut Follow,
    /// The discography's filter row.
    pub filter: &'a mut Filter,
    /// The per-release label store the discography fills in.
    pub releases: &'a mut Releases,
}

/// The sidebar's playlists and the tracks the current page has loaded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Library {
    pub playlists: Load<Vec<PlaylistEntry>>,
    /// The shows the user follows, as the library root draws them.
    pub followed: Load<Vec<FollowedShow>>,
    pub tracks: Load<TracksFor>,
    /// The album the album page draws.
    pub album: Load<AlbumDetail>,
    /// The show the show page draws.
    pub show: Load<ShowDetail>,
    /// The artist the artist page draws.
    pub artist: Load<ArtistDetail>,
    /// The artist page's Follow state.
    pub follow: Follow,
    /// The artist page's discography filter. Unlike `follow`, going home does
    /// not reset it: home then Back reloads the same artist, so the choice is
    /// kept, and opening a different artist is what resets it.
    pub filter: Filter,
    /// The per-release track labels the discography fills in as it scrolls.
    pub releases: Releases,
    /// The library root's filter row. Unlike `filter`, going home does not
    /// reset it: home then Back reloads the same root, so the choice is kept,
    /// and the root is the page the choice belongs to.
    pub root_filter: RootFilter,
    /// The sidebar's kind chips. Like `root_filter`, opening a page or going
    /// home does not reset it.
    pub sidebar_kind: SidebarFilter,
    /// The playlist `tracks` currently concerns; keeps a stale reply out.
    ///
    /// It is not the page: a reply that lands while the user is on an album
    /// page is kept, so Back finds it ready.
    wanted: Option<String>,
    /// The album `album` currently concerns; keeps a stale reply out.
    wanted_album: Option<String>,
    /// The show `show` currently concerns; keeps a stale reply out.
    wanted_show: Option<String>,
    /// The artist `artist` currently concerns; keeps a stale reply out.
    wanted_artist: Option<String>,
    /// Where the library is, and the pages Back and Forward return to.
    history: History,
    /// Context images, keyed by the playlist that owns them, so an upgrade that
    /// arrives before the playlists list is ready is not lost.
    context_covers: HashMap<String, CoverUrl>,
}

impl Library {
    /// Folds one worker update into the state.
    ///
    /// The sign-in and library updates are order-independent: `SignedIn` only
    /// starts a load that has not started, so a `Playlists` that arrives first
    /// is left alone.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::SignedIn => {
                if self.playlists == Load::Idle {
                    self.playlists = Load::Loading;
                }
            }
            Update::Playlists(playlists) => {
                let mut playlists = playlists.clone();
                for entry in &mut playlists {
                    if let Some(cover) = self.context_covers.get(&entry.uri) {
                        entry.cover = Some(cover.clone());
                    }
                }
                self.context_covers
                    .retain(|uri, _| playlists.iter().any(|entry| entry.uri == *uri));
                let present = |uri: &str| playlists.iter().any(|entry| entry.uri == uri);
                // Only playlist pages can be pruned; album, artist and show
                // pages are not in the list and must survive it.
                self.history.retain(|page| match page {
                    Page::Playlist(uri) => present(uri),
                    Page::Album(_) | Page::Artist(_) | Page::Show(_) => true,
                });
                if self
                    .wanted
                    .as_deref()
                    .is_some_and(|wanted| !present(wanted))
                {
                    self.wanted = None;
                    self.tracks = Load::Idle;
                }
                // A ready list names its playlist, so it is kept only while
                // that playlist exists. The root can leave one behind, with no
                // wanted load to prune it by.
                let ready = match &self.tracks {
                    Load::Ready(tracks) => Some(tracks.playlist.clone()),
                    Load::Idle | Load::Loading | Load::Failed(_) => None,
                };
                if ready.is_some_and(|playlist| !present(&playlist)) {
                    self.tracks = Load::Idle;
                }
                self.playlists = Load::Ready(playlists);
            }
            Update::PlaylistFailed(failure) => {
                self.playlists = Load::Failed(*failure);
            }
            Update::Tracks {
                playlist,
                tracks,
                cover,
            } => {
                // The cover upgrade is keyed by the update's own playlist, never
                // the selected one, so a reply for A cannot land on B.
                if let Some(cover) = cover {
                    self.context_covers.insert(playlist.clone(), cover.clone());
                    if let Load::Ready(playlists) = &mut self.playlists
                        && let Some(entry) =
                            playlists.iter_mut().find(|entry| entry.uri == *playlist)
                    {
                        entry.cover = Some(cover.clone());
                    }
                }
                if self.wanted.as_deref() == Some(playlist.as_str()) {
                    self.tracks = Load::Ready(TracksFor {
                        playlist: playlist.clone(),
                        tracks: tracks.clone(),
                    });
                }
            }
            Update::TracksFailed { playlist, failure } => {
                if self.wanted.as_deref() == Some(playlist.as_str()) {
                    self.tracks = Load::Failed(*failure);
                }
            }
            // A reply lands only while its own load is in flight: a stale one is
            // discarded, and a second reply cannot replace the first.
            Update::Album(detail) => {
                if self.wanted_album.as_deref() == Some(detail.uri.as_str())
                    && matches!(self.album, Load::Loading)
                {
                    self.album = Load::Ready(detail.clone());
                }
            }
            Update::AlbumFailed { album, failure } => {
                if self.wanted_album.as_deref() == Some(album.as_str())
                    && matches!(self.album, Load::Loading)
                {
                    self.album = Load::Failed(*failure);
                }
            }
            // The artist's reply is guarded the same way the album's is, and it
            // is the one thing that registers the releases the page may label.
            Update::Artist(detail) => {
                if self.wanted_artist.as_deref() == Some(detail.uri.as_str())
                    && matches!(self.artist, Load::Loading)
                {
                    self.releases.retain(&detail.releases);
                    self.artist = Load::Ready(detail.clone());
                }
            }
            Update::ArtistFailed { artist, failure } => {
                if self.wanted_artist.as_deref() == Some(artist.as_str())
                    && matches!(self.artist, Load::Loading)
                {
                    self.artist = Load::Failed(*failure);
                }
            }
            // The show's reply is guarded exactly as the album's is.
            Update::Show(detail) => {
                if self.wanted_show.as_deref() == Some(detail.show.uri.as_str())
                    && matches!(self.show, Load::Loading)
                {
                    self.show = Load::Ready(detail.clone());
                }
            }
            Update::ShowFailed { show, failure } => {
                if self.wanted_show.as_deref() == Some(show.as_str())
                    && matches!(self.show, Load::Loading)
                {
                    self.show = Load::Failed(*failure);
                }
            }
            // The followed list is a whole set, not keyed by URI: a reply is
            // accepted whenever it arrives, whatever page is on screen.
            Update::FollowedShows(shows) => {
                self.followed = Load::Ready(shows.clone());
            }
            Update::FollowedShowsFailed(failure) => {
                self.followed = Load::Failed(*failure);
            }
            Update::ReleaseTracks { .. } | Update::ReleaseTracksFailed { .. } => {
                self.releases.apply(update);
            }
            Update::Followed { .. } | Update::FollowFailed { .. } => {
                self.follow.apply(update);
            }
            Update::NeedsAuthorization
            | Update::SignInFailed(_)
            | Update::Transport(_)
            | Update::Cover { .. }
            | Update::CoverFailed { .. } => {}
        }
    }

    /// The page on screen, or `None` for the library root.
    pub fn page(&self) -> Option<&Page> {
        self.history.current()
    }

    /// Which playlist's tracks are on screen, if any.
    ///
    /// This is `None` on an album, artist or show page: the main panel is not
    /// showing a playlist, so the sidebar highlight clears and the entry and
    /// name helpers see no selection.
    pub fn selected(&self) -> Option<&str> {
        match self.history.current()? {
            Page::Playlist(uri) => Some(uri),
            Page::Album(_) | Page::Artist(_) | Page::Show(_) => None,
        }
    }

    /// Moves to a playlist and returns the command that loads it.
    ///
    /// Re-selecting the page already on screen reloads it without growing the
    /// history.
    pub fn select(&mut self, playlist: &str) -> Command {
        self.history.go(Some(Page::Playlist(playlist.to_owned())));
        self.load(playlist)
    }

    /// Moves to any page and returns the command that settles it.
    pub fn open(&mut self, page: Page) -> Option<Command> {
        self.history.go(Some(page));
        self.settle()
    }

    /// Moves to the playlists grid.
    ///
    /// The stale-reply guard is cleared so a late reply for the playlist just
    /// left is discarded. A settled list is kept for the page Back returns to;
    /// an in-flight or failed one is dropped, because nothing will ever settle
    /// it and the root must not sit on a spinner.
    pub fn to_playlists(&mut self) {
        self.history.go(None);
        self.wanted = None;
        self.wanted_album = None;
        self.wanted_artist = None;
        self.wanted_show = None;
        self.follow = Follow::default();
        if matches!(self.tracks, Load::Loading | Load::Failed(_)) {
            self.tracks = Load::Idle;
        }
        if matches!(self.album, Load::Loading | Load::Failed(_)) {
            self.album = Load::Idle;
        }
        if matches!(self.artist, Load::Loading | Load::Failed(_)) {
            self.artist = Load::Idle;
        }
        if matches!(self.show, Load::Loading | Load::Failed(_)) {
            self.show = Load::Idle;
        }
    }

    /// Moves back one page and returns the command that settles it.
    ///
    /// A Back with nowhere to go changes nothing, so it never reloads the page
    /// it is already on.
    pub fn back(&mut self) -> Option<Command> {
        if !self.history.back() {
            return None;
        }
        self.settle()
    }

    /// Moves forward one page and returns the command that settles it.
    ///
    /// A Forward with nowhere to go changes nothing, so it never reloads the
    /// page it is already on.
    pub fn forward(&mut self) -> Option<Command> {
        if !self.history.forward() {
            return None;
        }
        self.settle()
    }

    /// Whether there is a page to go back to.
    pub fn can_back(&self) -> bool {
        self.history.can_back()
    }

    /// Whether there is a page to go forward to.
    pub fn can_forward(&self) -> bool {
        self.history.can_forward()
    }

    /// Marks a playlist as wanted and returns the command that loads it.
    ///
    /// It never touches the history: Back and Forward move the page first and
    /// only reload what they land on.
    fn load(&mut self, playlist: &str) -> Command {
        self.wanted = Some(playlist.to_owned());
        self.tracks = Load::Loading;
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
    fn settle(&mut self) -> Option<Command> {
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
            _ => None,
        }
    }

    /// Marks the playlists loading again and returns the command that fetches
    /// them.
    ///
    /// The followed-shows list restarts with them: a playlist retry is the
    /// root's Retry, so both halves of the page ask again. A reply already in
    /// flight from before the retry still lands, and its list is valid — only a
    /// little older than the retry's.
    pub fn reload(&mut self) -> Command {
        self.playlists = Load::Loading;
        self.followed = Load::Idle;
        Command::ReloadLibrary
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

    /// The album the album page draws, once it is loaded and named by that page.
    ///
    /// The URI check is not decoration: a ready detail from an earlier page must
    /// not name a crumb for a page that is still loading something else.
    pub fn current_album(&self) -> Option<&AlbumDetail> {
        let Page::Album(uri) = self.history.current()? else {
            return None;
        };
        match &self.album {
            Load::Ready(detail) if detail.uri == *uri => Some(detail),
            Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
        }
    }

    /// The artist the artist page draws, once it is loaded and named by that
    /// page.
    ///
    /// The URI check is the album's: a ready detail from an earlier page must
    /// not name a crumb, or fill a hero, for a page loading something else.
    pub fn current_artist(&self) -> Option<&ArtistDetail> {
        let Page::Artist(uri) = self.history.current()? else {
            return None;
        };
        match &self.artist {
            Load::Ready(detail) if detail.uri == *uri => Some(detail),
            Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
        }
    }

    /// The show the show page draws, once it is loaded and named by that page.
    ///
    /// The URI check is the album's: a ready detail from an earlier page must
    /// not name a crumb, or fill a band, for a page loading something else.
    pub fn current_show(&self) -> Option<&ShowDetail> {
        let Page::Show(uri) = self.history.current()? else {
            return None;
        };
        match &self.show {
            Load::Ready(detail) if detail.show.uri == *uri => Some(detail),
            Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
        }
    }

    /// The artist page's four state pieces, borrowed for one frame.
    ///
    /// They are handed out together because the draw needs all four and the
    /// borrow checker will not let it take them one call at a time; the page's
    /// own URI is read from [`Self::page`] before this borrow starts.
    pub fn artist_page(&mut self) -> ArtistPage<'_> {
        let Self {
            artist,
            follow,
            filter,
            releases,
            history,
            ..
        } = self;
        let current = match history.current() {
            Some(Page::Artist(uri)) => match &*artist {
                Load::Ready(detail) if detail.uri == *uri => Some(detail),
                Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
            },
            _ => None,
        };
        ArtistPage {
            load: artist,
            current,
            follow,
            filter,
            releases,
        }
    }

    /// The selected playlist's name, once the list that names it is ready.
    pub fn selected_name(&self) -> Option<&str> {
        self.selected_entry().map(|entry| entry.name.as_str())
    }

    /// The selected playlist's entry, once the list that holds it is ready.
    pub fn selected_entry(&self) -> Option<&PlaylistEntry> {
        let selected = self.selected()?;
        match &self.playlists {
            Load::Ready(playlists) => playlists.iter().find(|entry| entry.uri == selected),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use takofy_model::{
        AddedAt, AlbumDetail, ArtistDetail, CoverUrl, Episode, FollowedShow, PlaylistEntry,
        ReleaseKind, Show, ShowDetail, TrackEntry,
    };

    use super::*;
    use crate::discography::Chip;
    use crate::library_filter::RootKind;
    use crate::message::TransportUpdate;
    use crate::sidebar_kind::SidebarKind;

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
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

    fn dated(uri: &str, wire: &str) -> TrackEntry {
        TrackEntry {
            added_at: AddedAt::from_wire(wire),
            ..track(uri)
        }
    }

    fn tracks_for(playlist: &str, tracks: Vec<TrackEntry>) -> TracksFor {
        TracksFor {
            playlist: playlist.to_owned(),
            tracks,
        }
    }

    fn album(uri: &str) -> Page {
        Page::Album(uri.to_owned())
    }

    fn artist_command(uri: &str) -> Command {
        Command::LoadArtist {
            artist: uri.to_owned(),
        }
    }

    fn artist_detail(uri: &str) -> ArtistDetail {
        ArtistDetail {
            uri: uri.to_owned(),
            name: "Vela".to_owned(),
            portrait: None,
            top_tracks: vec![track("top")],
            releases: vec![release("spotify:album:r1")],
        }
    }

    fn release(uri: &str) -> takofy_model::Release {
        takofy_model::Release {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            kind: takofy_model::ReleaseKind::Album,
            year: None,
            cover: None,
            tracks: vec![uri.to_owned()],
        }
    }

    fn album_detail(uri: &str) -> AlbumDetail {
        AlbumDetail {
            uri: uri.to_owned(),
            name: "Record".to_owned(),
            artists: "Band".to_owned(),
            year: Some(1997),
            cover: None,
            tracks: vec![track("t")],
        }
    }

    fn artist(uri: &str) -> Page {
        Page::Artist(uri.to_owned())
    }

    fn show(uri: &str) -> Page {
        Page::Show(uri.to_owned())
    }

    fn show_command(uri: &str) -> Command {
        Command::LoadShow {
            show: uri.to_owned(),
        }
    }

    fn episode(uri: &str) -> Episode {
        Episode {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            show: "Lateral".to_owned(),
            show_uri: Some("spotify:show:a".to_owned()),
            description: None,
            published: None,
            duration_ms: None,
            cover: None,
            named: true,
        }
    }

    fn show_detail(uri: &str) -> ShowDetail {
        ShowDetail {
            show: Show {
                uri: uri.to_owned(),
                name: "Show".to_owned(),
                publisher: "Lateral".to_owned(),
                description: String::new(),
                cover: None,
            },
            episodes: vec![episode("spotify:episode:e")],
        }
    }

    fn followed_show(uri: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            named: true,
            publisher: None,
            cover: None,
        }
    }

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: Some(cover),
            description: None,
            owner: None,
        }
    }

    #[test]
    fn selected_is_none_before_a_click() {
        assert_eq!(Library::default().selected(), None);
    }

    #[test]
    fn signed_in_starts_the_playlists_loading() {
        let mut library = Library::default();
        library.apply(&Update::SignedIn);
        assert_eq!(library.playlists, Load::Loading);
    }

    #[test]
    fn signed_in_leaves_ready_playlists_alone() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::SignedIn);
        assert_eq!(library.playlists, Load::Ready(vec![playlist("a", "A")]));
    }

    #[test]
    fn playlists_become_ready_without_touching_the_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.playlists, Load::Ready(vec![playlist("a", "A")]));
        assert_eq!(library.tracks, Load::Idle);
    }

    #[test]
    fn a_playlist_failure_is_recorded() {
        let mut library = Library::default();
        library.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(library.playlists, Load::Failed(Failure::Load));
    }

    #[test]
    fn select_marks_the_load_and_returns_the_command() {
        let mut library = Library::default();
        let command = library.select("spotify:playlist:a");

        assert_eq!(
            command,
            Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }
        );
        assert_eq!(library.tracks, Load::Loading);
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned()))
        );
        assert_eq!(library.wanted.as_deref(), Some("spotify:playlist:a"));
    }

    #[test]
    fn select_of_the_current_playlist_reloads_without_growing_the_history() {
        let mut library = Library::default();
        library.select("a");

        let command = library.select("a");

        assert_eq!(
            command,
            Command::LoadTracks {
                playlist: "a".to_owned()
            }
        );
        assert_eq!(library.tracks, Load::Loading);
        assert_eq!(library.back(), None);
        assert_eq!(library.page(), None);
        assert!(
            !library.can_back(),
            "re-selecting the current page must add no history entry"
        );
    }

    #[test]
    fn selected_is_none_on_an_album_page() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");

        library.open(album("album"));

        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(library.selected(), None, "the sidebar highlight clears");
        assert_eq!(library.selected_name(), None);
        assert_eq!(library.selected_entry(), None);
    }

    #[test]
    fn open_an_album_starts_its_load_and_keeps_the_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        assert_eq!(
            library.open(album("spotify:album:rec")),
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        assert_eq!(library.page(), Some(&album("spotify:album:rec")));
        assert_eq!(library.album, Load::Loading);
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")])),
            "the playlist behind the album stays ready for Back"
        );
    }

    /// The reply path: only the wanted album's answer lands, the first one wins,
    /// and a failure is recorded the same way a playlist's is.
    #[test]
    fn an_album_reply_lands_only_on_its_own_want() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(library.album, Load::Ready(album_detail("spotify:album:a")));

        library.apply(&Update::Album(album_detail("spotify:album:b")));
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "a reply nobody asked for is discarded"
        );
    }

    #[test]
    fn a_stale_album_reply_is_discarded() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.open(album("spotify:album:b"));

        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(library.album, Load::Loading, "A's reply is not B's");
        library.apply(&Update::Album(album_detail("spotify:album:b")));
        assert_eq!(library.album, Load::Ready(album_detail("spotify:album:b")));
    }

    #[test]
    fn a_stale_album_failure_is_discarded() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.open(album("spotify:album:b"));

        library.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.album, Load::Loading, "A's failure is not B's");

        library.apply(&Update::AlbumFailed {
            album: "spotify:album:b".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.album, Load::Failed(Failure::Load));
    }

    #[test]
    fn the_first_album_reply_wins() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        let first = AlbumDetail {
            name: "First".to_owned(),
            ..album_detail("spotify:album:a")
        };
        let second = AlbumDetail {
            name: "Second".to_owned(),
            ..album_detail("spotify:album:a")
        };

        library.apply(&Update::Album(first.clone()));
        library.apply(&Update::Album(second));

        assert_eq!(library.album, Load::Ready(first));
    }

    #[test]
    fn back_onto_a_ready_album_needs_no_command() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));
        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the artist page starts its own load"
        );

        assert_eq!(library.back(), None, "a ready album is already settled");
        assert_eq!(library.page(), Some(&album("spotify:album:a")));
    }

    #[test]
    fn back_onto_a_failed_album_reloads_it() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a"))
        );

        assert_eq!(
            library.back(),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "a failed album has nothing to draw, so Back asks again"
        );
        assert_eq!(library.album, Load::Loading);
    }

    #[test]
    fn reopening_a_failed_album_retries_it() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the page's Retry is this call, so it must re-issue the load"
        );
        assert_eq!(library.album, Load::Loading);
    }

    #[test]
    fn to_playlists_clears_the_album_want_and_drops_an_unsettled_load() {
        let mut loading = Library::default();
        loading.open(album("spotify:album:a"));
        loading.to_playlists();
        assert_eq!(loading.album, Load::Idle, "a load nobody wants is dropped");
        assert_eq!(
            loading.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the want was cleared, so the album is asked for again"
        );

        let mut failed = Library::default();
        failed.open(album("spotify:album:a"));
        failed.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });
        failed.to_playlists();
        assert_eq!(failed.album, Load::Idle);

        let mut ready = Library::default();
        ready.open(album("spotify:album:a"));
        ready.apply(&Update::Album(album_detail("spotify:album:a")));
        ready.to_playlists();
        assert_eq!(
            ready.album,
            Load::Ready(album_detail("spotify:album:a")),
            "a settled album is kept for the page Back returns to"
        );
        assert_eq!(
            ready.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the want is cleared even when the album stays ready"
        );
    }

    #[test]
    fn a_playlist_prune_leaves_the_album_alone() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        // The list no longer holds the playlist, so its page is pruned.
        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.page(), Some(&album("spotify:album:a")));
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "a playlist prune never touches the album"
        );
        assert_eq!(
            library.current_album(),
            Some(&album_detail("spotify:album:a"))
        );
    }

    #[test]
    fn current_album_is_none_while_loading() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        assert_eq!(library.current_album(), None);
    }

    #[test]
    fn current_album_is_none_when_a_ready_detail_names_another_page() {
        let mut library = Library::default();
        library.open(album("spotify:album:b"));
        library.album = Load::Ready(album_detail("spotify:album:a"));

        assert_eq!(
            library.current_album(),
            None,
            "A's detail must not name B's page"
        );
    }

    #[test]
    fn current_album_is_some_when_it_names_the_page() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(
            library.current_album(),
            Some(&album_detail("spotify:album:a"))
        );
    }

    #[test]
    fn open_a_show_starts_its_load_and_keeps_the_album() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(
            library.open(show("spotify:show:s")),
            Some(show_command("spotify:show:s"))
        );
        assert_eq!(library.page(), Some(&show("spotify:show:s")));
        assert_eq!(library.show, Load::Loading);
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "the album behind the show stays ready for Back"
        );
    }

    #[test]
    fn a_show_reply_lands_only_on_its_own_want() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::Show(show_detail("spotify:show:a")));

        assert_eq!(library.show, Load::Ready(show_detail("spotify:show:a")));

        library.apply(&Update::Show(show_detail("spotify:show:b")));
        assert_eq!(
            library.show,
            Load::Ready(show_detail("spotify:show:a")),
            "a reply nobody asked for is discarded"
        );
    }

    #[test]
    fn a_stale_show_reply_is_discarded() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.open(show("spotify:show:b"));

        library.apply(&Update::Show(show_detail("spotify:show:a")));

        assert_eq!(library.show, Load::Loading, "A's reply is not B's");
        library.apply(&Update::Show(show_detail("spotify:show:b")));
        assert_eq!(library.show, Load::Ready(show_detail("spotify:show:b")));
    }

    #[test]
    fn a_stale_show_failure_is_discarded() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.open(show("spotify:show:b"));

        library.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.show, Load::Loading, "A's failure is not B's");

        library.apply(&Update::ShowFailed {
            show: "spotify:show:b".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.show, Load::Failed(Failure::Load));
    }

    #[test]
    fn the_first_show_reply_wins() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        let first = ShowDetail {
            show: Show {
                name: "First".to_owned(),
                ..show_detail("spotify:show:a").show
            },
            ..show_detail("spotify:show:a")
        };
        let second = ShowDetail {
            show: Show {
                name: "Second".to_owned(),
                ..show_detail("spotify:show:a").show
            },
            ..show_detail("spotify:show:a")
        };

        library.apply(&Update::Show(first.clone()));
        library.apply(&Update::Show(second));

        assert_eq!(library.show, Load::Ready(first));
    }

    #[test]
    fn back_onto_a_ready_show_needs_no_command() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::Show(show_detail("spotify:show:a")));
        assert_eq!(
            library.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the album page starts its own load"
        );

        assert_eq!(library.back(), None, "a ready show is already settled");
        assert_eq!(library.page(), Some(&show("spotify:show:a")));
    }

    #[test]
    fn back_onto_a_failed_show_reloads_it() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a"))
        );

        assert_eq!(
            library.back(),
            Some(show_command("spotify:show:a")),
            "a failed show has nothing to draw, so Back asks again"
        );
        assert_eq!(library.show, Load::Loading);
    }

    #[test]
    fn reopening_a_failed_show_retries_it() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.open(show("spotify:show:a")),
            Some(show_command("spotify:show:a")),
            "the page's Retry is this call, so it must re-issue the load"
        );
        assert_eq!(library.show, Load::Loading);
    }

    #[test]
    fn to_playlists_clears_the_show_want_and_drops_an_unsettled_load() {
        let mut loading = Library::default();
        loading.open(show("spotify:show:a"));
        loading.to_playlists();
        assert_eq!(loading.show, Load::Idle, "a load nobody wants is dropped");
        assert_eq!(
            loading.open(show("spotify:show:a")),
            Some(show_command("spotify:show:a")),
            "the want was cleared, so the show is asked for again"
        );

        let mut failed = Library::default();
        failed.open(show("spotify:show:a"));
        failed.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });
        failed.to_playlists();
        assert_eq!(failed.show, Load::Idle);

        let mut ready = Library::default();
        ready.open(show("spotify:show:a"));
        ready.apply(&Update::Show(show_detail("spotify:show:a")));
        ready.to_playlists();
        assert_eq!(
            ready.show,
            Load::Ready(show_detail("spotify:show:a")),
            "a settled show is kept for the page Back returns to"
        );
        assert_eq!(
            ready.open(show("spotify:show:a")),
            Some(show_command("spotify:show:a")),
            "the want is cleared even when the show stays ready"
        );
    }

    #[test]
    fn a_playlist_prune_leaves_the_show_alone() {
        let mut library = Library::default();
        library.select("a");
        library.open(show("spotify:show:a"));
        library.apply(&Update::Show(show_detail("spotify:show:a")));

        // The list no longer holds the playlist, so its page is pruned.
        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.page(), Some(&show("spotify:show:a")));
        assert_eq!(
            library.show,
            Load::Ready(show_detail("spotify:show:a")),
            "a playlist prune never touches the show"
        );
        assert_eq!(library.current_show(), Some(&show_detail("spotify:show:a")));
    }

    #[test]
    fn current_show_is_none_while_loading_or_for_another_uri() {
        let mut library = Library::default();
        library.open(show("spotify:show:b"));
        assert_eq!(library.current_show(), None, "loading names nothing");

        library.show = Load::Ready(show_detail("spotify:show:a"));
        assert_eq!(
            library.current_show(),
            None,
            "A's detail must not name B's page"
        );

        let mut loaded = Library::default();
        loaded.open(show("spotify:show:a"));
        loaded.apply(&Update::Show(show_detail("spotify:show:a")));
        assert_eq!(
            loaded.current_show(),
            Some(&show_detail("spotify:show:a")),
            "a detail that names the page is drawn"
        );
    }

    #[test]
    fn followed_starts_idle() {
        assert_eq!(Library::default().followed, Load::Idle);
    }

    #[test]
    fn followed_request_waits_for_ready_playlists() {
        let mut library = Library::default();
        assert_eq!(library.followed_request(), None, "nothing has loaded yet");

        library.apply(&Update::SignedIn);
        assert_eq!(library.followed_request(), None, "loading is not ready");

        library.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(
            library.followed_request(),
            None,
            "a failed library never names a window"
        );
        assert_eq!(library.followed, Load::Idle);

        let mut ready = Library::default();
        ready.apply(&Update::Playlists(Vec::new()));
        assert_eq!(ready.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(ready.followed, Load::Loading);
    }

    #[test]
    fn followed_request_fires_once() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(
            library.followed_request(),
            None,
            "an in-flight list is not asked for twice"
        );
    }

    #[test]
    fn followed_request_is_none_while_loading_ready_or_failed() {
        for state in [
            Load::Loading,
            Load::Ready(vec![followed_show("spotify:show:a")]),
            Load::Failed(Failure::Followed),
        ] {
            let mut library = Library::default();
            library.apply(&Update::Playlists(vec![playlist("a", "A")]));
            library.followed = state.clone();

            assert_eq!(library.followed_request(), None, "{state:?} asked again");
            assert_eq!(library.followed, state);
        }
    }

    #[test]
    fn a_followed_reply_is_recorded_whatever_the_page() {
        let shows = vec![followed_show("spotify:show:a")];
        let mut library = Library::default();
        library.apply(&Update::FollowedShows(shows.clone()));
        assert_eq!(library.followed, Load::Ready(shows.clone()));

        library.open(show("spotify:show:b"));
        library.apply(&Update::FollowedShows(shows.clone()));
        assert_eq!(
            library.followed,
            Load::Ready(shows),
            "a show page does not gate the list"
        );

        library.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(library.followed, Load::Ready(Vec::new()));
    }

    #[test]
    fn a_followed_failure_is_recorded() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::FollowedShowsFailed(Failure::Followed));

        assert_eq!(library.followed, Load::Failed(Failure::Followed));
        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist("a", "A")]),
            "the playlists beside it still draw"
        );
    }

    #[test]
    fn reload_resets_followed_so_it_refetches() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::FollowedShows(vec![followed_show(
            "spotify:show:a",
        )]));

        assert_eq!(library.reload(), Command::ReloadLibrary);
        assert_eq!(library.followed, Load::Idle, "a playlist retry re-arms it");
        assert_eq!(
            library.followed_request(),
            None,
            "the playlists are loading again, so the list waits"
        );

        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    }

    #[test]
    fn reload_followed_marks_loading_and_returns_the_command() {
        let mut library = Library::default();
        library.apply(&Update::FollowedShowsFailed(Failure::Followed));

        assert_eq!(library.reload_followed(), Command::LoadFollowedShows);
        assert_eq!(library.followed, Load::Loading);

        let mut ready = Library::default();
        ready.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(ready.reload_followed(), Command::LoadFollowedShows);
        assert_eq!(ready.followed, Load::Loading);
    }

    #[test]
    fn reload_while_followed_is_loading_accepts_either_reply() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(library.followed, Load::Loading);

        // A playlist retry while the first load is still in flight re-arms the
        // list, so a second load is issued and two replies are in the air.
        library.reload();
        assert_eq!(library.followed, Load::Idle);
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(
            library.followed_request(),
            Some(Command::LoadFollowedShows),
            "the second load is issued while the first is still in flight"
        );

        let first = vec![followed_show("spotify:show:a")];
        let second = vec![followed_show("spotify:show:b")];
        library.apply(&Update::FollowedShows(first.clone()));
        assert_eq!(library.followed, Load::Ready(first));
        library.apply(&Update::FollowedShows(second.clone()));
        assert_eq!(
            library.followed,
            Load::Ready(second),
            "the later whole-set reply wins and neither corrupts the state"
        );
    }

    #[test]
    fn back_to_a_ready_playlist_returns_no_command() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });
        library.open(album("album"));

        assert_eq!(library.back(), None);
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
    }

    #[test]
    fn back_to_a_failed_playlist_loads_it_again() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        library.open(album("album"));

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.tracks, Load::Loading);
        // The reload must not push the page it lands on.
        assert!(library.back().is_none(), "the root is the only entry left");
        assert!(!library.can_back());
    }

    #[test]
    fn back_to_an_idle_playlist_loads_it() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("album"));
        library.tracks = Load::Idle;

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn back_to_a_loading_playlist_returns_no_command() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("album"));

        assert_eq!(library.back(), None);
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn back_from_the_root_loads_the_playlist() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });
        library.to_playlists();

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_reply_for_the_wanted_playlist_is_applied_on_an_album_page() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("album"));

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
    }

    #[test]
    fn can_back_and_can_forward_track_the_moves() {
        let mut library = Library::default();
        assert!(!library.can_back());
        assert!(!library.can_forward());

        library.select("a");
        assert!(library.can_back());
        assert!(!library.can_forward());

        assert_eq!(
            library.open(album("spotify:album:rec")),
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        assert_eq!(
            library.back(),
            None,
            "back onto the playlist already wanted needs no load"
        );
        assert!(library.can_forward());

        assert_eq!(
            library.forward(),
            None,
            "forward onto the album already wanted needs no load"
        );
        assert!(!library.can_forward());
        assert_eq!(library.page(), Some(&album("spotify:album:rec")));
    }

    #[test]
    fn a_prune_keeps_an_album_page_and_the_album_and_artist_entries() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });
        library.open(album("album"));
        library.open(artist("artist"));
        library.back();

        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
        assert_eq!(library.back(), None);
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
        assert_eq!(library.forward(), None);
        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(library.forward(), None);
        assert_eq!(library.page(), Some(&artist("artist")));
    }

    #[test]
    fn a_prune_of_the_current_playlist_returns_to_the_root_without_duplicates() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.page(), None);
        assert_eq!(library.selected(), None);
        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.wanted, None);
        assert!(
            !library.can_back(),
            "the root must not be duplicated behind the root"
        );
    }

    #[test]
    fn back_with_nowhere_to_go_does_not_reload() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.back(), None, "the root needs no command");
        assert_eq!(library.page(), None);
        assert_eq!(
            library.back(),
            None,
            "no page left behind the root, and no reload"
        );
        assert!(!library.can_back());
    }

    #[test]
    fn forward_with_nowhere_to_go_does_not_reload_a_failed_playlist() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert!(!library.can_forward());
        assert_eq!(
            library.forward(),
            None,
            "a no-op Forward must not retry the page it is on"
        );
        assert_eq!(library.tracks, Load::Failed(Failure::Load));
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
    }

    #[test]
    fn a_prune_with_no_wanted_load_clears_ready_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });
        library.to_playlists();
        assert_eq!(library.wanted, None);
        assert!(matches!(library.tracks, Load::Ready(_)));

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(
            library.tracks,
            Load::Idle,
            "the deleted playlist's ready tracks must be freed"
        );
    }

    #[test]
    fn to_playlists_while_loading_leaves_no_spinner_at_the_root() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        assert_eq!(library.tracks, Load::Loading);

        library.to_playlists();

        assert_eq!(
            library.tracks,
            Load::Idle,
            "the root must not sit on a spinner nothing can settle"
        );
        assert_eq!(library.wanted, None);
    }

    #[test]
    fn to_playlists_drops_a_failed_load() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        library.to_playlists();

        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.wanted, None);
    }

    #[test]
    fn tracks_keep_their_added_at_through_apply() {
        let mut library = Library::default();
        library.select("a");
        let tracks = vec![
            dated("t0", "1700000000000"),
            track("t1"),
            dated("t2", "1792357200000"),
        ];
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: tracks.clone(),
            cover: None,
        });

        assert_eq!(library.tracks, Load::Ready(tracks_for("a", tracks)));
    }

    #[test]
    fn a_reply_for_the_selected_playlist_is_applied() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
    }

    #[test]
    fn a_reply_for_a_stale_playlist_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.select("b");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        assert_eq!(library.tracks, Load::Loading);
        assert_eq!(library.selected(), Some("b"));
    }

    #[test]
    fn a_failure_for_a_stale_playlist_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.select("b");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_failure_for_the_selected_playlist_is_recorded() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.tracks, Load::Failed(Failure::Load));
    }

    #[test]
    fn tracks_before_any_selection_are_discarded() {
        let mut library = Library::default();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });
        assert_eq!(library.tracks, Load::Idle);
    }

    #[test]
    fn reload_marks_the_playlists_loading_and_returns_the_command() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.reload(), Command::ReloadLibrary);
        assert_eq!(library.playlists, Load::Loading);
    }

    #[test]
    fn playlists_keep_the_tracks_when_the_selection_survives() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        library.apply(&Update::Playlists(vec![
            playlist("a", "A"),
            playlist("b", "B"),
        ]));

        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
        assert_eq!(library.selected(), Some("a"));
    }

    #[test]
    fn playlists_clear_the_tracks_when_the_selection_is_gone() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.selected(), None);
    }

    #[test]
    fn to_playlists_clears_the_selection_but_keeps_the_tracks() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        library.to_playlists();

        assert_eq!(library.selected(), None);
        assert_eq!(library.page(), None);
        assert!(library.can_back());
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
        assert_eq!(library.wanted, None, "the stale-reply guard is cleared");
    }

    #[test]
    fn a_reply_after_to_playlists_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0")],
            cover: None,
        });
        library.to_playlists();
        let settled = Load::Ready(tracks_for("a", vec![track("t0")]));

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t1")],
            cover: None,
        });
        assert_eq!(library.tracks, settled, "the late tracks are discarded");

        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.tracks, settled, "the late failure is discarded");

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "going back into the playlist reloads it"
        );
    }

    #[test]
    fn the_selected_name_comes_from_a_ready_list() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        assert_eq!(library.selected_name(), Some("A"));
    }

    #[test]
    fn the_selected_name_is_none_while_the_list_reloads() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.reload();
        assert_eq!(library.selected(), Some("a"));
        assert_eq!(library.selected_name(), None);
    }

    #[test]
    fn transport_updates_are_ignored() {
        let mut library = Library::default();
        library.apply(&Update::Transport(TransportUpdate::Ended));
        assert_eq!(library.playlists, Load::Idle);
        assert_eq!(library.tracks, Load::Idle);
    }

    #[test]
    fn a_tracks_cover_upgrades_an_entry_that_already_had_a_cover() {
        let first = cover_url("first");
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a", "A", first,
        )]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context.clone()),
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)])
        );
    }

    #[test]
    fn a_tracks_without_a_cover_leaves_the_entry_alone() {
        let first = cover_url("first");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "A",
            first.clone(),
        )]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", first)])
        );
    }

    #[test]
    fn a_tracks_cover_upgrades_the_update_playlist_not_the_selected_one() {
        let a_first = cover_url("a-first");
        let a_context = cover_url("a-context");
        let b_cover = cover_url("b");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            playlist_with_cover("a", "A", a_first),
            playlist_with_cover("b", "B", b_cover.clone()),
        ]));
        library.select("b");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(a_context.clone()),
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![
                playlist_with_cover("a", "A", a_context),
                playlist_with_cover("b", "B", b_cover),
            ])
        );
        // A's reply must not fill B's tracks.
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_cover_arriving_before_playlists_is_applied_after_them() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context.clone()),
        });
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)])
        );
    }

    #[test]
    fn a_playlists_reload_keeps_a_recorded_upgrade() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context.clone()),
        });
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)])
        );
    }

    #[test]
    fn a_removed_playlists_cover_is_pruned() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context),
        });
        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert!(library.context_covers.is_empty(), "the key must be pruned");
        assert_eq!(library.playlists, Load::Ready(vec![playlist("b", "B")]));
    }

    #[test]
    fn open_an_artist_starts_its_load_and_keeps_the_album() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a"))
        );
        assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
        assert_eq!(library.artist, Load::Loading);
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "the album behind the artist stays ready for Back"
        );
    }

    #[test]
    fn an_artist_reply_lands_only_on_its_own_want_while_loading() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:a"))
        );

        library.apply(&Update::Artist(artist_detail("spotify:artist:b")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:a")),
            "a reply nobody asked for is discarded"
        );
    }

    #[test]
    fn a_stale_artist_reply_is_discarded() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.open(artist("spotify:artist:b"));

        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        assert_eq!(library.artist, Load::Loading, "A's reply is not B's");

        library.apply(&Update::Artist(artist_detail("spotify:artist:b")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:b"))
        );
    }

    #[test]
    fn a_stale_artist_failure_is_discarded() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.open(artist("spotify:artist:b"));

        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.artist, Load::Loading, "A's failure is not B's");

        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:b".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.artist, Load::Failed(Failure::Load));
    }

    #[test]
    fn the_first_artist_reply_wins() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        let first = ArtistDetail {
            name: "First".to_owned(),
            ..artist_detail("spotify:artist:a")
        };
        let second = ArtistDetail {
            name: "Second".to_owned(),
            ..artist_detail("spotify:artist:a")
        };

        library.apply(&Update::Artist(first.clone()));
        library.apply(&Update::Artist(second));

        assert_eq!(library.artist, Load::Ready(first));
    }

    #[test]
    fn back_onto_a_ready_artist_needs_no_command() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.to_playlists();

        assert_eq!(library.back(), Some(artist_command("spotify:artist:a")));
        assert_eq!(library.artist, Load::Loading);
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        library.select("p");
        assert_eq!(library.back(), None, "a ready artist is already settled");
        assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
    }

    #[test]
    fn back_onto_a_failed_artist_reloads_it() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        library.select("p");

        assert_eq!(
            library.back(),
            Some(artist_command("spotify:artist:a")),
            "a failed artist has nothing to draw, so Back asks again"
        );
        assert_eq!(library.artist, Load::Loading);
    }

    #[test]
    fn reopening_a_failed_artist_retries_it() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the page's Retry is this call, so it must re-issue the load"
        );
        assert_eq!(library.artist, Load::Loading);
    }

    #[test]
    fn the_filter_starts_on_all() {
        assert_eq!(Library::default().filter.chosen(), Chip::All);
        assert_eq!(Library::default().filter.artist(), None);
    }

    #[test]
    fn the_sidebar_kind_starts_on_playlists() {
        let mut library = Library::default();
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);

        library.open(show("spotify:show:a"));
        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "opening a page keeps the sidebar's choice"
        );

        library.back();
        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "a Back keeps the sidebar's choice"
        );

        library.to_playlists();
        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "going home keeps the sidebar's choice"
        );
    }

    #[test]
    fn the_sidebar_kind_survives_a_trip_to_a_show_page_and_back() {
        let mut library = Library::default();
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));

        library.open(show("spotify:show:a"));
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);

        library.back();
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);

        library.to_playlists();
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);
    }

    #[test]
    fn the_playlist_highlight_clears_on_a_show_page_and_returns() {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.open(show("spotify:show:a"));
        assert_eq!(
            library.selected(),
            None,
            "the main panel is not showing a playlist"
        );

        library.back();
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
    }

    #[test]
    fn the_chosen_chip_survives_a_trip_to_a_show_page_and_back() {
        let mut library = Library::default();
        library.root_filter.choose(RootKind::Podcasts);
        assert_eq!(
            library.root_filter.chosen(),
            RootKind::Podcasts,
            "a live chip is pressed"
        );

        library.open(show("spotify:show:a"));
        assert_eq!(
            library.root_filter.chosen(),
            RootKind::Podcasts,
            "opening a page keeps the root's choice"
        );

        library.back();
        assert_eq!(
            library.root_filter.chosen(),
            RootKind::Podcasts,
            "a Back to the root redraws the chip the user left"
        );
    }

    #[test]
    fn opening_another_artist_resets_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));

        library.open(artist("spotify:artist:b"));

        assert_eq!(
            library.filter.chosen(),
            Chip::All,
            "another artist's discs are not the ones that were filtered"
        );
        assert_eq!(library.filter.artist(), Some("spotify:artist:b"));
    }

    #[test]
    fn back_onto_the_same_artist_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));

        library.to_playlists();
        let _ = library.back();

        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Ep),
            "Back onto the same artist draws what the user left"
        );
    }

    #[test]
    fn retrying_the_same_artist_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        library.filter.choose(Chip::Kind(ReleaseKind::Single));

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "Retry re-issues the same load"
        );
        assert_eq!(library.filter.chosen(), Chip::Kind(ReleaseKind::Single));
    }

    #[test]
    fn an_artist_reply_does_not_touch_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.filter.choose(Chip::Kind(ReleaseKind::Compilation));

        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Compilation),
            "a reply is not an artist change"
        );
    }

    #[test]
    fn to_playlists_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Single));

        library.to_playlists();
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Single),
            "home then Back reloads the same artist, so the choice is kept"
        );
        assert_eq!(library.filter.artist(), Some("spotify:artist:a"));

        let _ = library.back();
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Single),
            "home then Back is still the same artist"
        );
    }

    #[test]
    fn reopening_the_same_artist_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        library.to_playlists();

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "home cleared the want, so the page is loaded fresh rather than recalled"
        );
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Ep),
            "opening the artist the choice was made for keeps it"
        );
    }

    #[test]
    fn another_artists_ep_chip_does_not_keep_the_choice() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));

        library.open(artist("spotify:artist:b"));

        let releases = vec![takofy_model::Release {
            kind: ReleaseKind::Ep,
            ..release("spotify:album:b-ep")
        }];
        let row = crate::discography::chips(&releases);
        assert!(
            row.contains(&Chip::Kind(ReleaseKind::Ep)),
            "B has an EP, so its row offers the chip the choice named: {row:?}"
        );
        assert_eq!(
            library.filter.effective(&row),
            Chip::All,
            "B still draws All pressed, because the choice belonged to A"
        );
    }

    #[test]
    fn artist_page_hands_out_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.filter.choose(Chip::Kind(ReleaseKind::Single));
        {
            let page = library.artist_page();
            assert_eq!(page.filter.chosen(), Chip::Kind(ReleaseKind::Single));
            page.filter.choose(Chip::All);
        }
        assert_eq!(
            library.filter.chosen(),
            Chip::All,
            "the page borrows the library's own filter, not a copy"
        );
    }

    #[test]
    fn to_playlists_clears_the_artist_want_and_drops_an_unsettled_load() {
        let mut loading = Library::default();
        loading.open(artist("spotify:artist:a"));
        loading.to_playlists();
        assert_eq!(loading.artist, Load::Idle, "a load nobody wants is dropped");
        assert_eq!(
            loading.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the want was cleared, so the artist is asked for again"
        );

        let mut failed = Library::default();
        failed.open(artist("spotify:artist:a"));
        failed.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        failed.to_playlists();
        assert_eq!(failed.artist, Load::Idle);

        let mut ready = Library::default();
        ready.open(artist("spotify:artist:a"));
        ready.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        ready.to_playlists();
        assert_eq!(
            ready.artist,
            Load::Ready(artist_detail("spotify:artist:a")),
            "a settled artist is kept for the page Back returns to"
        );
        assert_eq!(
            ready.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the want is cleared even when the artist stays ready"
        );
    }

    #[test]
    fn to_playlists_resets_the_follow_state() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.follow.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        });
        assert_eq!(library.follow.label(), "Following");

        library.to_playlists();

        assert_eq!(library.follow.label(), "Follow");
        assert_eq!(library.follow.artist(), None);
    }

    #[test]
    fn current_artist_is_none_while_loading_or_for_another_uri() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        assert_eq!(library.current_artist(), None);

        library.artist = Load::Ready(artist_detail("spotify:artist:b"));
        assert_eq!(
            library.current_artist(),
            None,
            "B's detail must not name A's page"
        );

        library.artist = Load::Ready(artist_detail("spotify:artist:a"));
        assert_eq!(
            library.current_artist(),
            Some(&artist_detail("spotify:artist:a"))
        );
    }

    #[test]
    fn a_playlist_prune_leaves_the_artist_alone() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("p", "P")]));
        library.select("p");
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        library.apply(&Update::Playlists(vec![playlist("q", "Q")]));

        assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:a")),
            "a playlist prune never touches the artist"
        );
    }

    #[test]
    fn an_artist_reply_retains_only_its_own_releases() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        // R1 is asked for, so its entry is in flight rather than idle: that is
        // what makes a dropped entry distinguishable from a retained one below.
        library.releases.look("spotify:album:r1");
        assert_eq!(
            library.releases.requests(),
            vec![Command::LoadReleaseTracks {
                release: "spotify:album:r1".to_owned(),
                tracks: vec!["spotify:album:r1".to_owned()],
            }]
        );

        // Coming back to the artist reloads it; the reply no longer names R1.
        library.open(artist("spotify:artist:b"));
        library.open(artist("spotify:artist:a"));
        let mut other = artist_detail("spotify:artist:a");
        other.releases = vec![release("spotify:album:r2")];
        library.apply(&Update::Artist(other));

        assert_eq!(
            library.releases.look("spotify:album:r1"),
            crate::releases::Labels::Idle,
            "a release the artist no longer names is forgotten"
        );
        library.releases.look("spotify:album:r2");
        assert_eq!(
            library.releases.requests(),
            vec![Command::LoadReleaseTracks {
                release: "spotify:album:r2".to_owned(),
                tracks: vec!["spotify:album:r2".to_owned()],
            }]
        );
    }

    #[test]
    fn a_release_reply_reaches_the_release_store() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.releases.look("spotify:album:r1");
        let _ = library.releases.requests();

        library.apply(&Update::ReleaseTracks {
            release: "spotify:album:r1".to_owned(),
            tracks: vec![track("t")],
        });

        assert!(matches!(
            library.releases.look("spotify:album:r1"),
            crate::releases::Labels::Ready(_)
        ));
    }

    #[test]
    fn band_cover_waits_until_the_tracks_settle() {
        let cover = cover_url("context");
        let entry = playlist_with_cover("a", "A", cover.clone());
        assert_eq!(band_cover(&entry, false), BandCover::Pending);
        assert_eq!(band_cover(&entry, true), BandCover::Entry(Some(&cover)));
    }

    #[test]
    fn band_cover_is_missing_when_the_entry_names_none() {
        let entry = playlist("a", "A");
        assert_eq!(band_cover(&entry, true), BandCover::Entry(None));
    }

    #[test]
    fn a_failed_context_still_settles_the_band_on_the_entry() {
        // The band's rule is the load's settle state, not the context's success:
        // a failed context settles onto whatever the entry already holds.
        let cover = cover_url("first-track");
        let entry = playlist_with_cover("a", "A", cover.clone());
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![entry]));
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        let settled = !matches!(library.tracks, Load::Loading | Load::Idle);
        let entry = library.selected_entry().expect("the selected entry");
        assert_eq!(band_cover(entry, settled), BandCover::Entry(Some(&cover)));
    }

    #[test]
    fn band_cover_handles_an_empty_playlist() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
        });
        assert_eq!(library.tracks, Load::Ready(tracks_for("a", Vec::new())));
        let settled = !matches!(library.tracks, Load::Loading | Load::Idle);
        // With no list loaded there is no entry to read; the band's caller draws
        // nothing. This pins the settled rule for the empty list itself.
        assert!(settled);
        assert!(library.selected_entry().is_none());
    }
}
