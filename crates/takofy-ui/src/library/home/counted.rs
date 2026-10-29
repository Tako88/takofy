//! The counted shelf's read, and the section the page draws it from.

use std::collections::HashSet;

use takofy_model::home::HomeAsk;
use takofy_model::{LikedTrack, SavedAlbum};

use crate::home::counted::{self, Holding, Tile, Tiles};
use crate::home::{Section, Slot, settle};
use crate::message::Command;

use super::super::{Library, Load};

impl Library {
    /// What the counted shelf draws: the artists the library holds most of.
    ///
    /// The shelf waits whole: the reads it draws from have not settled, or the
    /// labels of the liked songs are still owed, so a partial ranking could
    /// never be published as the library's own. A library with nothing liked
    /// waits for no label, so its saved albums rank as soon as they answer.
    pub(super) fn counted_section(&self) -> Section<Tiles<'_>> {
        let intake = self.home_intake();
        let feeds = [intake.albums, intake.likes];
        // A failed rootlist, a pending feed, or a held answer decides first.
        let decided = settle(intake.playlists, &feeds, None::<Tiles<'_>>);
        if !matches!(decided, Section::Absent) {
            return decided;
        }
        if let Some(failure) = self.home.counted.failed() {
            return Section::Failed(failure);
        }
        if !self.home.counted.settled() && self.any_liked() {
            return Section::Waiting;
        }
        settle(intake.playlists, &feeds, self.counted_tiles())
    }

    /// The one read the counted shelf owes this open: the labels of every liked
    /// song, so the ranking counts all of them rather than a recent sample.
    pub(super) fn counted_ask(&mut self) -> Option<Command> {
        if !self.home.counted.owed() || !self.home_intake().settled() {
            return None;
        }
        let tracks = self.liked_uris();
        if tracks.is_empty() {
            // Nothing is liked, so there are no labels to wait for: the shelf
            // settles on what the saved albums name instead of holding the page
            // for a read that would ask for nothing.
            self.home.counted.nothing_to_ask();
            return None;
        }
        if !self.home.budget.take(Slot::Counted) {
            return None;
        }
        self.home.counted.mark_asked(tracks.clone());
        Some(Command::LoadHome(HomeAsk::Labels { tracks }))
    }

    /// Whether the library holds a liked song the ranking can label.
    fn any_liked(&self) -> bool {
        self.likes.recent().into_iter().flatten().next().is_some()
    }

    /// Every liked song once, in the library's saved order.
    ///
    /// A song the read lists twice is one song: it is asked for once and it
    /// names its artists once, so the ask and the ranking see the same library.
    fn liked_once(&self) -> impl Iterator<Item = &LikedTrack> {
        let mut seen: HashSet<&str> = HashSet::new();
        self.likes
            .recent()
            .into_iter()
            .flatten()
            .filter(move |track| seen.insert(track.uri.as_str()))
    }

    /// Every liked song's URI, in the library's saved order, each once.
    fn liked_uris(&self) -> Vec<String> {
        self.liked_once().map(|track| track.uri.clone()).collect()
    }

    /// The tiles this open draws, when the library holds any artist at all.
    fn counted_tiles(&self) -> Option<Tiles<'_>> {
        let holdings = self.counted_holdings();
        let ranked = counted::rank(&holdings);
        if ranked.is_empty() {
            return None;
        }
        let followed = match &self.followed_artists {
            Load::Ready(artists) => Some(artists.as_slice()),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        };
        let artists = counted::pick(ranked.len(), self.home.counted.seed())
            .into_iter()
            .filter_map(|at| ranked.get(at))
            .map(|artist| Tile {
                uri: artist.uri,
                name: artist.name,
                portrait: followed
                    .and_then(|list| list.iter().find(|card| card.uri == artist.uri))
                    .and_then(|card| card.portrait.as_ref()),
            })
            .collect();
        Some(Tiles { artists })
    }

    /// Every artist the library holds, once per item that names it.
    ///
    /// A saved album names its artists once each, and a liked song does the same
    /// through the label the answer carried. Albums come first so a tie in the
    /// ranking reads the way the library is ordered.
    fn counted_holdings<'a>(&'a self) -> Vec<Holding<'a>> {
        let mut holdings = Vec::new();
        if let Load::Ready(albums) = &self.saved_albums {
            for album in albums {
                holdings.extend(counted::one_each(album_pairs(album)));
            }
        }
        for track in self.liked_once() {
            let Some(artists) = self.home.counted.artists(&track.uri) else {
                continue;
            };
            holdings.extend(counted::one_each(
                artists
                    .iter()
                    .map(|(uri, name)| (uri.as_str(), name.as_str())),
            ));
        }
        holdings
    }
}

/// One album's artists, its URIs and its names read in step.
fn album_pairs(album: &SavedAlbum) -> impl Iterator<Item = (&str, &str)> {
    album
        .artist_uris
        .iter()
        .map(String::as_str)
        .zip(album.artist_names.iter().map(String::as_str))
}

#[cfg(test)]
mod tests;
