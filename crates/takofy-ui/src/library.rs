//! The load state for the playlists and their tracks.
//!
//! The library is a reducer over worker updates: it decides what is on screen
//! and which load a reply belongs to, and it hands the view a command when the
//! user asks for one.

use std::collections::HashMap;

use takofy_model::{CoverUrl, PlaylistEntry, TrackEntry};

use crate::message::{Command, Failure, Update};
use crate::route::{History, Page};

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

/// The sidebar's playlists and the tracks the current page has loaded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Library {
    pub playlists: Load<Vec<PlaylistEntry>>,
    pub tracks: Load<TracksFor>,
    /// The playlist `tracks` currently concerns; keeps a stale reply out.
    ///
    /// It is not the page: a reply that lands while the user is on an album
    /// page is kept, so Back finds it ready.
    wanted: Option<String>,
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
        if matches!(self.tracks, Load::Loading | Load::Failed(_)) {
            self.tracks = Load::Idle;
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

    /// Returns the command the current page needs, if any.
    ///
    /// A playlist page whose wanted load is in flight or ready is left alone;
    /// any other playlist page is reloaded through `load`.
    fn settle(&mut self) -> Option<Command> {
        let playlist = match self.history.current() {
            Some(Page::Playlist(playlist)) => playlist.clone(),
            _ => return None,
        };
        if self.wanted.as_deref() == Some(playlist.as_str())
            && matches!(self.tracks, Load::Loading | Load::Ready(_))
        {
            return None;
        }
        Some(self.load(&playlist))
    }

    /// Marks the playlists loading again and returns the command that fetches
    /// them.
    pub fn reload(&mut self) -> Command {
        self.playlists = Load::Loading;
        Command::ReloadLibrary
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
    use takofy_model::{AddedAt, CoverUrl, PlaylistEntry, TrackEntry};

    use super::*;
    use crate::message::TransportUpdate;

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
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

    fn artist(uri: &str) -> Page {
        Page::Artist(uri.to_owned())
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
    fn open_an_album_returns_no_command_and_keeps_the_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
        });

        assert_eq!(library.open(album("album")), None);
        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
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

        library.open(album("album"));
        assert_eq!(library.back(), None);
        assert!(library.can_forward());

        assert_eq!(library.forward(), None);
        assert!(!library.can_forward());
        assert_eq!(library.page(), Some(&album("album")));
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
