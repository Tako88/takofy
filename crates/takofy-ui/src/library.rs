//! The load state for the playlists and their tracks.
//!
//! The library is a reducer over worker updates: it decides what is on screen
//! and which load a reply belongs to, and it hands the view a command when the
//! user asks for one.

use std::collections::HashMap;

use takofy_model::{CoverUrl, PlaylistEntry, TrackEntry};

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
                let gone = self
                    .selected
                    .as_deref()
                    .is_some_and(|selected| !playlists.iter().any(|entry| entry.uri == selected));
                self.playlists = Load::Ready(playlists);
                if gone {
                    self.selected = None;
                    self.tracks = Load::Idle;
                }
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
            Update::NeedsAuthorization
            | Update::SignInFailed(_)
            | Update::Transport(_)
            | Update::Cover { .. }
            | Update::CoverFailed { .. } => {}
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
    use takofy_model::{CoverUrl, PlaylistEntry, TrackEntry};

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
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn tracks_for(playlist: &str, tracks: Vec<TrackEntry>) -> TracksFor {
        TracksFor {
            playlist: playlist.to_owned(),
            tracks,
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
    fn deselect_clears_the_selection_and_the_tracks() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
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
            cover: None,
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
}
