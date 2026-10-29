//! Home's view of the library: what the three reads are doing, and the shelf
//! that follows from them.
//!
//! Both methods read state only: no page and no clock, so a constructed
//! `Library` can reach every state at once.

use takofy_model::{LikedTrack, SavedAt};

use crate::home::{Card, Intake, Outlook, Phase, Saved, Shelf, Tally, outlook, pick, pool};

use super::{Library, Load};

/// One thing a card can be drawn from, before the shelf knows whether it can be.
enum Pooled<'a> {
    /// A playlist or an album, which draw without a label.
    Saved(Saved<'a>),
    /// A liked song, which needs its label to name it and to find its album.
    Liked(&'a LikedTrack),
}

/// What one load is doing, as a source of the shelf.
fn phase_of<T>(load: &Load<T>) -> Phase {
    match load {
        Load::Idle | Load::Loading => Phase::Pending,
        Load::Ready(_) => Phase::Arrived,
        Load::Failed(failure) => Phase::Failed(*failure),
    }
}

impl Library {
    /// What Home's three sources are doing.
    ///
    /// A source that was never asked counts as the state of the thing it waits
    /// on: the albums and likes reads do not run without the rootlist, so behind
    /// a failed rootlist they are failed too, and the page says so instead of
    /// waiting for answers no read will ever ask for.
    pub(super) fn home_intake(&self) -> Intake {
        let unasked = match self.playlists {
            Load::Failed(failure) => Some(failure),
            Load::Idle | Load::Loading | Load::Ready(_) => None,
        };
        Intake {
            playlists: phase_of(&self.playlists),
            albums: unasked.map_or_else(|| phase_of(&self.saved_albums), Phase::Failed),
            likes: unasked.map_or_else(|| self.likes.phase(), Phase::Failed),
        }
    }

    /// What Home draws under the greeting, with at most `cards` cards.
    pub fn home_shelf(&self, cards: usize) -> Shelf<'_> {
        let any = matches!(&self.playlists, Load::Ready(items) if !items.is_empty())
            || matches!(&self.saved_albums, Load::Ready(items) if !items.is_empty())
            || self.likes.any();

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

        let dates: Vec<Option<SavedAt>> = all.iter().map(|(_, at)| *at).collect();
        let mut unanswered = 0;
        let mut usable: Vec<usize> = Vec::new();
        for index in pool(&dates) {
            let draws = match all.get(index).map(|(item, _)| item) {
                Some(Pooled::Saved(_)) => true,
                Some(Pooled::Liked(track)) => {
                    if self.home.answered(&track.uri) {
                        // An answered miss leaves the pool rather than holding
                        // it: the label will not answer twice.
                        self.home.label(&track.uri).is_some()
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
        };
        match outlook(self.home_intake(), tally) {
            Outlook::Waiting => Shelf::Waiting,
            Outlook::Unreadable(failure) => Shelf::Unreadable(failure),
            Outlook::Incomplete(failure) => Shelf::Incomplete(failure),
            Outlook::DayOne => Shelf::DayOne,
            Outlook::Quiet => Shelf::Quiet,
            Outlook::Cards => Shelf::Cards(self.home_cards(&all, &dates, &usable, cards)),
        }
    }

    /// The drawn cards, in date order.
    fn home_cards<'a>(
        &'a self,
        all: &[(Pooled<'a>, Option<SavedAt>)],
        dates: &[Option<SavedAt>],
        usable: &[usize],
        cards: usize,
    ) -> Vec<Card<'a>> {
        pick(dates, usable, self.home.seed(), cards)
            .into_iter()
            .filter_map(|index| {
                let (item, at) = all.get(index)?;
                let at = (*at)?;
                let saved = match item {
                    Pooled::Saved(saved) => *saved,
                    Pooled::Liked(track) => Saved::Track(self.home.label(&track.uri)?),
                };
                Some(Card { saved, at })
            })
            .collect()
    }
}
