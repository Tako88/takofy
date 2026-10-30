//! The stations shelf's read, and the state the page draws it from.

use takofy_model::home::HomeAsk;

use crate::home::stations::{Band, Candidates, pick_seeds};
use crate::home::{Phase, Section, Slot};
use crate::message::Command;

use super::super::{Library, Load};
use super::phase_of;

impl Library {
    /// What the stations shelf draws.
    ///
    /// One station failing leaves the others: the band is whatever landed, and
    /// the failure is named only when nothing did.
    pub(super) fn stations_section(&self) -> Section<Band<'_>> {
        // The rootlist gates every read behind it, and the page says so once
        // above the bands.
        if let Phase::Failed(failure) = phase_of(&self.playlists) {
            return Section::Failed(failure);
        }
        if let Some(band) = self.home.stations.band() {
            return Section::Ready(band);
        }
        // Nothing drew. The shelf is waiting only while a read of its own is out
        // or the library its seeds come from has not arrived: the seeds are
        // chosen once every feed settles, so a settled library the shelf holds no
        // station from has nothing to show.
        if self.home.stations.pending() || phase_of(&self.playlists) == Phase::Pending {
            return Section::Waiting;
        }
        match self.home.stations.failure() {
            Some(failure) => Section::Failed(failure),
            None => Section::Absent,
        }
    }

    /// The read the stations shelf owes, if one is due.
    ///
    /// One station per call, and never one of this open's seeds twice: the four
    /// asks are the band's whole budget, so a station that failed is not asked
    /// again until the shelf is retried.
    pub(super) fn stations_ask(&mut self) -> Option<Command> {
        if !self.home.stations.chosen() {
            // The seed list is kept for the whole open, so it is chosen once the
            // feeds behind it have settled: seeding it from a half-read library
            // would drop the rest of a feed's candidates.
            if !self.stations_settled() {
                return None;
            }
            let seed = self.home.seed();
            let seeds = {
                let playlists: Vec<String> = match &self.playlists {
                    Load::Ready(items) => items.iter().map(|entry| entry.uri.clone()).collect(),
                    Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
                };
                let artists: Vec<String> = match &self.followed_artists {
                    Load::Ready(items) => items.iter().map(|artist| artist.uri.clone()).collect(),
                    Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
                };
                let albums: Vec<String> = match &self.saved_albums {
                    Load::Ready(items) => items.iter().map(|album| album.uri.clone()).collect(),
                    Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
                };
                let songs: Vec<String> = self
                    .likes
                    .recent()
                    .map(|recent| recent.map(|track| track.uri.clone()).collect())
                    .unwrap_or_default();
                pick_seeds(
                    &Candidates {
                        playlists: &playlists,
                        artists: &artists,
                        albums: &albums,
                        songs: &songs,
                    },
                    seed,
                )
            };
            self.home.stations.choose(seeds);
        }
        let seed = self.home.stations.next().cloned()?;
        if !self.home.budget.take(Slot::Stations) {
            return None;
        }
        self.home.stations.asked(&seed);
        Some(Command::LoadHome(HomeAsk::Station { seed }))
    }

    /// The label read the band owes, for the lengths its rows end in.
    ///
    /// A station read names its tracks but not how long they run, so the lengths
    /// are one batched read over the rows the band shows. Rows already answered
    /// are not asked for again, so the read runs at most once an open.
    pub(super) fn stations_lengths_ask(&mut self) -> Option<Command> {
        let mut tracks: Vec<String> = Vec::new();
        if let Some(band) = self.home.stations.band() {
            for station in &band.stations {
                for row in station.shown.iter().filter_map(|at| station.whole.get(*at)) {
                    if self.home.duration(&row.uri).is_some()
                        || self.home.asked(&row.uri)
                        || tracks.iter().any(|asked| asked == &row.uri)
                    {
                        continue;
                    }
                    tracks.push(row.uri.clone());
                }
            }
        }
        if tracks.is_empty() || !self.home.budget.take(Slot::Stations) {
            return None;
        }
        self.home.mark_asked(&tracks);
        Some(Command::LoadHome(HomeAsk::Labels { tracks }))
    }

    /// Whether every feed the seeds are chosen from has settled.
    ///
    /// A failed rootlist is the page's own failure, so it never seeds a read
    /// the gated page would not draw.
    fn stations_settled(&self) -> bool {
        if matches!(phase_of(&self.playlists), Phase::Failed(_)) {
            return false;
        }
        [
            phase_of(&self.saved_albums),
            phase_of(&self.followed_artists),
            self.likes.phase(),
        ]
        .into_iter()
        .all(|phase| phase != Phase::Pending)
    }
}

#[cfg(test)]
mod tests;
