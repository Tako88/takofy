//! What every file here builds: a day, a library, a card and the answers a
//! row can get.

use takofy_model::home::{
    Day, DigAnswer, DigOutcome, DiscoveryAnswer, DiscoveryCard, DiscoveryKind, DiscoveryOutcome,
    DiscoverySeed, HomeAnswer, OutsideId,
};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use super::super::*;

pub(super) fn today() -> Day {
    Day::from_days(19_000)
}

pub(super) fn tomorrow() -> Day {
    Day::from_days(19_001)
}
pub(super) const NOW: u64 = 1_700_000_000_000;

pub(super) fn playlist(index: usize) -> PlaylistEntry {
    PlaylistEntry {
        uri: format!("spotify:playlist:{index}"),
        name: format!("Playlist {index}"),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: Some(20),
        owner: None,
        access: None,
        saved_at: None,
    }
}

pub(super) fn playlists(count: usize) -> Vec<PlaylistEntry> {
    (0..count).map(playlist).collect()
}

pub(super) fn artist(index: usize) -> FollowedArtist {
    FollowedArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("Artist {index}"),
        named: true,
        portrait: None,
        releases: None,
        followed_at: None,
    }
}

pub(super) fn artists(count: usize) -> Vec<FollowedArtist> {
    (0..count).map(artist).collect()
}

pub(super) fn album(index: usize) -> SavedAlbum {
    SavedAlbum {
        uri: format!("spotify:album:{index}"),
        name: format!("Album {index}"),
        named: true,
        artists: format!("Artist {index}"),
        artist_uris: vec![format!("spotify:artist:{index}")],
        artist_names: vec![format!("Artist {index}")],
        year: None,
        cover: None,
        tracks: Some(10),
        saved_at: None,
    }
}

pub(super) fn albums(count: usize) -> Vec<SavedAlbum> {
    (0..count).map(album).collect()
}

pub(super) fn card(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        artist: takofy_model::home::DiscoveryArtist {
            uri: format!("spotify:artist:card{index}"),
            name: format!("Card {index}"),
        },
        outside: None,
        portrait: None,
        tracks: Vec::new(),
        latest_year: None,
    }
}

pub(super) fn cards(indexes: &[usize]) -> Vec<DiscoveryCard> {
    indexes.iter().map(|index| card(*index)).collect()
}

pub(super) fn id(index: usize) -> OutsideId {
    OutsideId::from_id(index as i64)
}

pub(super) fn stamped(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        outside: Some(id(index)),
        ..card(index)
    }
}

pub(super) fn stamped_cards(indexes: &[usize]) -> Vec<DiscoveryCard> {
    indexes.iter().map(|index| stamped(*index)).collect()
}

pub(super) fn ring(indexes: &[usize], passed: &[usize], exhausted: bool) -> DigOutcome {
    DigOutcome::Ring {
        cards: stamped_cards(indexes),
        passed: passed.iter().copied().map(id).collect(),
        exhausted,
    }
}

pub(super) fn dig(seed: &DiscoverySeed, hop: u8, outcome: DigOutcome) -> HomeAnswer {
    HomeAnswer::Dig(DigAnswer {
        seed: seed.clone(),
        hop,
        outcome,
    })
}

pub(super) fn land_ring(state: &mut State, seed: &DiscoverySeed, indexes: &[usize]) {
    state.land(&answer(
        seed.clone(),
        DiscoveryOutcome::Neighbours {
            cards: stamped_cards(indexes),
            genre: None,
        },
    ));
}

pub(super) fn owned_cards(indexes: &[usize]) -> fresh::Owned {
    let followed: Vec<FollowedArtist> = indexes
        .iter()
        .map(|index| FollowedArtist {
            uri: card(*index).artist.uri,
            name: card(*index).artist.name,
            named: true,
            portrait: None,
            releases: None,
            followed_at: None,
        })
        .collect();
    fresh::Owned::new(&followed, &[] as &[SavedAlbum])
}

pub(super) fn owned(indexes: &[usize]) -> fresh::Owned {
    let followed: Vec<FollowedArtist> = indexes.iter().map(|index| artist(*index)).collect();
    fresh::Owned::new(&followed, &[] as &[SavedAlbum])
}

pub(super) fn landed_cards(state: &State, kind: DiscoveryKind) -> Vec<String> {
    match &state.rows[kind.index()].read {
        Read::Landed { cards, .. } => cards.iter().map(|card| card.artist.uri.clone()).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn answer(seed: DiscoverySeed, outcome: DiscoveryOutcome) -> HomeAnswer {
    HomeAnswer::Discovery(DiscoveryAnswer { seed, outcome })
}

pub(super) fn neighbours(indexes: &[usize]) -> DiscoveryOutcome {
    DiscoveryOutcome::Neighbours {
        cards: cards(indexes),
        genre: None,
    }
}

/// A state that has opened on `day` with `count` usable items of every kind.
pub(super) fn chosen(day: Day, count: usize) -> State {
    let mut state = State::default();
    state.latch(day);
    state.open();
    state.choose(
        day,
        &playlists(count),
        &artists(count),
        &albums(count),
        None,
    );
    state
}

/// A state that has opened on `day` with only playlists to seed from, so one
/// row is the only one that can ask.
pub(super) fn playlists_only(day: Day, count: usize) -> State {
    let mut state = State::default();
    state.latch(day);
    state.open();
    state.choose(day, &playlists(count), &[], &[], None);
    state
}

/// Lands the playlist row's first answer at `now`, which is when its wait starts.
pub(super) fn land_playlist(state: &mut State, now: u64) {
    let seed = state.next_ask().expect("the playlist row asks");
    state.land(&answer(seed, neighbours(&[0])));
    state.stamp_landed(now);
}

/// The seed the row of `kind` is on.
pub(super) fn current(state: &State, kind: DiscoveryKind) -> DiscoverySeed {
    let row = &state.rows[kind.index()];
    row.pool[row.at].clone()
}
