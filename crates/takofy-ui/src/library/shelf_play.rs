//! The play pressed on a shelf, and the rows it waits on.

use takofy_model::TrackEntry;

use crate::message::{Command, Failure};
use crate::sidebar_kind::SidebarKind;

use super::{Library, Load};

/// A play pressed on a card or a row of the library root.
///
/// The rows are held here rather than in `tracks`, which belongs to the page: the
/// press is not a navigation, and a page the user opens while the rows are in
/// flight must keep its own list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingPlay {
    pub(super) uri: String,
    pub(super) kind: SidebarKind,
    stage: Stage,
}

/// How far a pending play has got.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Pressed, and the rows have not been asked for yet.
    Wanted,
    /// The rows are in flight.
    Asked,
    /// The rows landed and wait for a frame that can start them.
    Landed(Vec<TrackEntry>),
}

/// What a landed shelf play hands the queue: the object's own rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The page the rows came from, so a later move can tell its target what to
    /// play.
    pub origin: String,
    pub tracks: Vec<TrackEntry>,
}

impl Library {
    /// Whether pressing play on a library item of `kind` starts that object.
    ///
    /// A show needs episode ordering and an artist needs top-tracks semantics,
    /// and both have their own loaders, so neither draws a play.
    pub fn playable(kind: SidebarKind) -> bool {
        matches!(kind, SidebarKind::Playlists | SidebarKind::Albums)
    }

    /// Holds a play pressed on the shelf until its rows can start the queue.
    ///
    /// Rows the window already holds are taken as they stand, so a press on the
    /// page's own playlist sends nothing. A second press replaces the first, and
    /// the same object pressed again is the same play. Returns whether the press
    /// was taken: a kind with no play refuses it.
    pub fn want_play(&mut self, uri: &str, kind: SidebarKind) -> bool {
        if !Self::playable(kind) {
            return false;
        }
        self.play_notice = None;
        if self.play.as_ref().is_some_and(|play| play.uri == uri) {
            return true;
        }
        let held = match (kind, &self.tracks, &self.album) {
            (SidebarKind::Playlists, Load::Ready(tracks), _) if tracks.playlist == uri => {
                Some(tracks.tracks.clone())
            }
            (SidebarKind::Albums, _, Load::Ready(detail)) if detail.uri == uri => {
                Some(detail.tracks.clone())
            }
            _ => None,
        };
        let stage = match held {
            Some(rows) => Stage::Landed(rows),
            None => Stage::Wanted,
        };
        self.play = Some(PendingPlay {
            uri: uri.to_owned(),
            kind,
            stage,
        });
        true
    }

    /// The command that fetches the pending play's rows, if one is due.
    ///
    /// It is want-driven like the followed lists: one ask per press, and never
    /// through [`Self::load`], so the page's own slot and want stay as they are.
    pub fn play_request(&mut self) -> Option<Command> {
        let play = self.play.as_mut()?;
        if play.stage != Stage::Wanted {
            return None;
        }
        let command = match play.kind {
            SidebarKind::Playlists => Command::LoadTracks {
                playlist: play.uri.clone(),
            },
            SidebarKind::Albums => Command::LoadAlbum {
                album: play.uri.clone(),
            },
            SidebarKind::Artists | SidebarKind::Podcasts => return None,
        };
        play.stage = Stage::Asked;
        Some(command)
    }

    /// Takes the rows a landed shelf play holds, clearing it.
    ///
    /// An empty list clears the play and starts nothing: there is no first track
    /// to start on.
    pub fn take_play(&mut self) -> Option<Start> {
        match self.play.take() {
            Some(PendingPlay {
                uri,
                stage: Stage::Landed(tracks),
                ..
            }) if !tracks.is_empty() => Some(Start {
                origin: uri,
                tracks,
            }),
            Some(PendingPlay {
                stage: Stage::Landed(_),
                ..
            }) => None,
            Some(play) => {
                self.play = Some(play);
                None
            }
            None => None,
        }
    }

    /// Whether a shelf play's rows have landed and no frame has started them.
    pub fn has_landed_play(&self) -> bool {
        self.play
            .as_ref()
            .is_some_and(|play| matches!(play.stage, Stage::Landed(_)))
    }

    /// Whether a play is waiting on `uri`'s rows.
    ///
    /// A reply is copied out of the update only when this says so, because every
    /// page read would otherwise pay for a play nobody pressed.
    pub(super) fn wants(&self, uri: &str) -> bool {
        self.play.as_ref().is_some_and(|play| {
            play.uri == uri && matches!(play.stage, Stage::Wanted | Stage::Asked)
        })
    }

    /// Lands `rows` on the pending play for `uri`, while it waits on them.
    pub(super) fn land(&mut self, uri: &str, rows: Vec<TrackEntry>) {
        let Some(play) = self.play.as_mut() else {
            return;
        };
        if play.uri != uri || !matches!(play.stage, Stage::Wanted | Stage::Asked) {
            return;
        }
        play.stage = Stage::Landed(rows);
    }

    /// Drops the pending play for `uri` and says why its rows did not arrive.
    pub(super) fn fail_play(&mut self, uri: &str, failure: Failure) {
        if !self.wants(uri) {
            return;
        }
        self.play = None;
        self.play_notice = Some(failure);
    }
}
