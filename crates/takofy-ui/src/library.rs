//! The load state for the playlists and their tracks.
//!
//! The library is a reducer over worker updates: it decides what is on screen
//! and which load a reply belongs to, and it hands the view a command when the
//! user asks for one.

use takofy_model::{PlaylistEntry, TrackEntry};

use crate::message::{Command, Failure, Update};

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

/// The sidebar's playlists and the selected playlist's tracks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Library {
    pub playlists: Load<Vec<PlaylistEntry>>,
    pub tracks: Load<TracksFor>,
    /// The playlist a reply must name to be drawn; keeps a stale reply out.
    selected: Option<String>,
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
                self.playlists = Load::Ready(playlists.clone());
                let gone = self
                    .selected
                    .as_deref()
                    .is_some_and(|selected| !playlists.iter().any(|entry| entry.uri == selected));
                if gone {
                    self.selected = None;
                    self.tracks = Load::Idle;
                }
            }
            Update::PlaylistFailed(failure) => {
                self.playlists = Load::Failed(*failure);
            }
            Update::Tracks { playlist, tracks } => {
                if self.selected.as_deref() == Some(playlist.as_str()) {
                    self.tracks = Load::Ready(TracksFor {
                        playlist: playlist.clone(),
                        tracks: tracks.clone(),
                    });
                }
            }
            Update::TracksFailed { playlist, failure } => {
                if self.selected.as_deref() == Some(playlist.as_str()) {
                    self.tracks = Load::Failed(*failure);
                }
            }
            Update::NeedsAuthorization | Update::SignInFailed(_) | Update::Transport(_) => {}
        }
    }

    /// Which playlist's tracks are on screen, if any.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Marks a playlist as selected and returns the command that loads it.
    pub fn select(&mut self, playlist: &str) -> Command {
        self.selected = Some(playlist.to_owned());
        self.tracks = Load::Loading;
        Command::LoadTracks {
            playlist: playlist.to_owned(),
        }
    }

    /// Marks the playlists loading again and returns the command that fetches
    /// them.
    pub fn reload(&mut self) -> Command {
        self.playlists = Load::Loading;
        Command::ReloadLibrary
    }

    /// Clears the selection and the tracks that belonged to it.
    pub fn deselect(&mut self) {
        self.selected = None;
        self.tracks = Load::Idle;
    }

    /// The selected playlist's name, once the list that names it is ready.
    pub fn selected_name(&self) -> Option<&str> {
        let selected = self.selected.as_deref()?;
        match &self.playlists {
            Load::Ready(playlists) => playlists
                .iter()
                .find(|entry| entry.uri == selected)
                .map(|entry| entry.name.as_str()),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use takofy_model::{PlaylistEntry, TrackEntry};

    use super::*;
    use crate::message::TransportUpdate;

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
        }
    }

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            named: true,
        }
    }

    fn tracks_for(playlist: &str, tracks: Vec<TrackEntry>) -> TracksFor {
        TracksFor {
            playlist: playlist.to_owned(),
            tracks,
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
    }

    #[test]
    fn a_reply_for_the_selected_playlist_is_applied() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
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
        });

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.selected(), None);
    }

    #[test]
    fn deselect_clears_the_selection_and_the_tracks() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
        });

        library.deselect();

        assert_eq!(library.selected(), None);
        assert_eq!(library.tracks, Load::Idle);
    }

    #[test]
    fn a_reply_for_a_deselected_playlist_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.deselect();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
        });

        assert_eq!(library.tracks, Load::Idle);
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
}
