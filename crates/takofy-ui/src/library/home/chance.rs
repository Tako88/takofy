//! The shelf at chance's read: the deck filled once the lists settle, and the
//! one ask a press owes.

use takofy_model::home::HomeAsk;
use takofy_model::{FollowedArtist, LikedTrack, PlaylistEntry, SavedAlbum};

use crate::home::chance::{self, Ask, Candidates};
use crate::home::{Phase, Section};
use crate::message::Command;

use super::super::{Library, Load};
use super::phase_of;

impl Library {
    /// What the shelf at chance draws.
    ///
    /// A failed rootlist is the page's own failure, said once above the bands,
    /// so this shelf stays out of the way. It waits only on a read of its own:
    /// the lists its deck is filled from are the band above's wait, and a shelf
    /// with nothing to draw is absent rather than waiting for them (#96).
    pub(super) fn chance_section(&self) -> Section<chance::Draw<'_>> {
        if matches!(phase_of(&self.playlists), Phase::Failed(_)) {
            return Section::Absent;
        }
        if let Some(draw) = self.home.chance.draw() {
            return Section::Ready(draw);
        }
        if self.home.chance.filled()
            && (self.home.chance.pending() || self.home.chance.owed().is_some())
        {
            return Section::Waiting;
        }
        Section::Absent
    }

    /// The read the shelf at chance owes, if one is due.
    ///
    /// The deck is filled once and kept for the whole open, so it is built only
    /// after every list it draws from has settled. One ask per call: the shelf
    /// claims the ask it returns, so no frame sends the same read twice and the
    /// page's own ask loop ends.
    pub(super) fn chance_ask(&mut self) -> Option<Command> {
        if !self.library_settled() {
            return None;
        }
        if !self.home.chance.filled() {
            let playlists: &[PlaylistEntry] = match &self.playlists {
                Load::Ready(items) => items,
                Load::Idle | Load::Loading | Load::Failed(_) => &[],
            };
            let artists: &[FollowedArtist] = match &self.followed_artists {
                Load::Ready(items) => items,
                Load::Idle | Load::Loading | Load::Failed(_) => &[],
            };
            let albums: &[SavedAlbum] = match &self.saved_albums {
                Load::Ready(items) => items,
                Load::Idle | Load::Loading | Load::Failed(_) => &[],
            };
            let songs: Vec<LikedTrack> = self
                .likes
                .recent()
                .map(|recent| recent.cloned().collect())
                .unwrap_or_default();
            self.home.chance.fill(&Candidates {
                playlists,
                artists,
                albums,
                songs: &songs,
            });
        }
        let ask = self.home.chance.owed()?;
        self.home.chance.asked(ask.clone());
        Some(match ask {
            Ask::Name(uri) => Command::LoadHome(HomeAsk::Labels { tracks: vec![uri] }),
            Ask::Station(seed) => Command::LoadHome(HomeAsk::Station { seed }),
        })
    }
}

#[cfg(test)]
mod tests;
