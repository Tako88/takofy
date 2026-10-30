//! The dated shelf: what the three reads behind it are doing, and the cards that
//! follow from them.
//!
//! It reads state only: no page and no clock, so a constructed `Library` can
//! reach every state at once.

use takofy_model::{LikedTrack, SavedAt};

use crate::home::{Card, Intake, Outlook, Saved, Shelf, Tally, inherit, outlook, pick, pool};

use super::super::{Library, Load};
use super::phase_of;

/// One thing a card can be drawn from, before the shelf knows whether it can be.
enum Pooled<'a> {
    /// A playlist or an album, which draw without a label.
    Saved(Saved<'a>),
    /// A liked song, which needs its label to name it and to find its album.
    Liked(&'a LikedTrack),
}

/// What Home draws under the greeting, with at most `cards` cards.
pub(super) fn home_shelf(library: &Library, cards: usize) -> Shelf<'_> {
    let any = matches!(&library.playlists, Load::Ready(items) if !items.is_empty())
        || matches!(&library.saved_albums, Load::Ready(items) if !items.is_empty())
        || library.likes.any();

    let all = library.home_entries();
    let dates: Vec<Option<SavedAt>> = all.iter().map(|(_, at)| *at).collect();
    let mut unanswered = 0;
    let mut usable: Vec<usize> = Vec::new();
    for index in pool(&dates) {
        let draws = match all.get(index).map(|(item, _)| item) {
            Some(Pooled::Saved(_)) => true,
            Some(Pooled::Liked(track)) => {
                if library.home.answered(&track.uri) {
                    // An answered miss leaves the pool rather than holding
                    // it: the label will not answer twice.
                    library.home.label(&track.uri).is_some()
                } else {
                    unanswered += 1;
                    false
                }
            }
            None => false,
        };
        if draws {
            usable.push(index);
        }
    }

    let tally = Tally {
        any,
        unanswered,
        usable: usable.len(),
        label_failed: library.home.failed(),
    };
    match outlook(library.home_intake(), tally) {
        Outlook::Waiting => Shelf::Waiting,
        Outlook::Unreadable(failure) => Shelf::Unreadable(failure),
        Outlook::Incomplete(failure) => Shelf::Incomplete(failure),
        Outlook::DayOne => Shelf::DayOne,
        Outlook::Quiet => Shelf::Quiet,
        Outlook::Cards => Shelf::Cards(home_cards(library, &all, &dates, &usable, cards)),
    }
}

impl Library {
    /// What Home's three sources are doing.
    ///
    /// A source that was never asked counts as the state of the thing it waits
    /// on: the albums and likes reads do not run without the rootlist, so behind
    /// a failed rootlist they are failed too, and the page says so instead of
    /// waiting for answers no read will ever ask for.
    pub(in crate::library) fn home_intake(&self) -> Intake {
        let playlists = phase_of(&self.playlists);
        Intake {
            playlists,
            albums: inherit(playlists, phase_of(&self.saved_albums)),
            likes: inherit(playlists, self.likes.phase()),
        }
    }

    /// The pooled liked songs whose label no read has asked for yet.
    ///
    /// Only the pool counts: a liked song older than the newest `POOL` entries
    /// is never drawn, so its label would be a read nobody uses.
    pub(in crate::library) fn home_unlabelled(&self) -> Vec<String> {
        let all = self.home_entries();
        let dates: Vec<Option<SavedAt>> = all.iter().map(|(_, at)| *at).collect();
        pool(&dates)
            .into_iter()
            .filter_map(|index| match all.get(index)?.0 {
                Pooled::Liked(track) if !self.home.asked(&track.uri) => Some(track.uri.clone()),
                Pooled::Saved(_) | Pooled::Liked(_) => None,
            })
            .collect()
    }

    /// Everything the shelf can draw from, with the date it was saved.
    fn home_entries(&self) -> Vec<(Pooled<'_>, Option<SavedAt>)> {
        let mut all: Vec<(Pooled<'_>, Option<SavedAt>)> = Vec::new();
        if let Load::Ready(playlists) = &self.playlists {
            all.extend(
                playlists
                    .iter()
                    .map(|entry| (Pooled::Saved(Saved::Playlist(entry)), entry.saved_at)),
            );
        }
        if let Load::Ready(albums) = &self.saved_albums {
            all.extend(
                albums
                    .iter()
                    .map(|album| (Pooled::Saved(Saved::Album(album)), album.saved_at)),
            );
        }
        if let Some(recent) = self.likes.recent() {
            all.extend(recent.map(|track| (Pooled::Liked(track), track.saved_at)));
        }
        all
    }
}

/// The drawn cards, in date order.
fn home_cards<'a>(
    library: &'a Library,
    all: &[(Pooled<'a>, Option<SavedAt>)],
    dates: &[Option<SavedAt>],
    usable: &[usize],
    cards: usize,
) -> Vec<Card<'a>> {
    pick(dates, usable, library.home.seed(), cards)
        .into_iter()
        .filter_map(|index| {
            let (item, at) = all.get(index)?;
            let at = (*at)?;
            let saved = match item {
                Pooled::Saved(saved) => *saved,
                Pooled::Liked(track) => Saved::Track(library.home.label(&track.uri)?),
            };
            Some(Card { saved, at })
        })
        .collect()
}
