use takofy_model::home::{DiscoveryArtist, DiscoveryCard};
use takofy_model::{FollowedArtist, SavedAlbum};

use super::*;

fn artist(index: usize) -> DiscoveryArtist {
    DiscoveryArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("Artist {index}"),
    }
}

fn card(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        artist: artist(index),
        portrait: None,
        top_track: format!("spotify:track:{index}"),
        top_title: format!("Track {index}"),
        latest_year: None,
    }
}

fn followed(index: usize) -> FollowedArtist {
    FollowedArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("Artist {index}"),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn saved(artists: &[usize]) -> SavedAlbum {
    SavedAlbum {
        uri: "spotify:album:{index}".replace("{index}", "0"),
        name: "Album".to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: artists
            .iter()
            .map(|index| format!("spotify:artist:{index}"))
            .collect(),
        artist_names: artists
            .iter()
            .map(|index| format!("Artist {index}"))
            .collect(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

fn uris<'a>(cards: &[&'a DiscoveryCard]) -> Vec<&'a str> {
    cards.iter().map(|card| card.artist.uri.as_str()).collect()
}

fn hidden(artists: &[usize]) -> Hidden {
    let mut hidden = Hidden::default();
    for index in artists {
        assert!(hidden.hide(&format!("spotify:artist:{index}")));
    }
    hidden
}

#[test]
fn a_followed_artist_is_dropped() {
    let cards = [card(0), card(1)];
    let owned = Owned::new(&[followed(1)], &[]);

    assert_eq!(
        uris(&fresh(&cards, None, &owned, &Hidden::default())),
        ["spotify:artist:0"],
        "the library already has the artist, so the row does not offer it back"
    );
}

#[test]
fn a_saved_albums_secondary_artist_is_dropped() {
    let cards = [card(0), card(1), card(2)];
    let owned = Owned::new(&[], &[saved(&[1, 2])]);

    assert_eq!(
        uris(&fresh(&cards, None, &owned, &Hidden::default())),
        ["spotify:artist:0"],
        "every artist credited on a saved album is one the library has"
    );
}

#[test]
fn a_liked_song_or_playlist_artist_is_not_owned() {
    let cards = [card(0)];
    let owned = Owned::new(&[], &[]);

    assert_eq!(
        uris(&fresh(&cards, None, &owned, &Hidden::default())),
        ["spotify:artist:0"],
        "a like names no artist and a playlist names no artist, so neither makes one owned"
    );
}

#[test]
fn a_hidden_artist_is_dropped() {
    let cards = [card(0), card(1)];

    assert_eq!(
        uris(&fresh(&cards, None, &Owned::default(), &hidden(&[1]))),
        ["spotify:artist:0"],
        "the permanent set keeps an artist out of every row"
    );
}

#[test]
fn the_anchor_is_dropped() {
    let cards = [card(0), card(1)];

    assert_eq!(
        uris(&fresh(
            &cards,
            Some(&artist(1)),
            &Owned::default(),
            &Hidden::default()
        )),
        ["spotify:artist:0"],
        "a row never recommends the artist it was reached from"
    );
}

#[test]
fn an_unfollowed_artist_becomes_eligible_again() {
    let cards = [card(0)];

    assert!(
        fresh(
            &cards,
            None,
            &Owned::new(&[followed(0)], &[]),
            &Hidden::default()
        )
        .is_empty()
    );
    assert_eq!(
        uris(&fresh(
            &cards,
            None,
            &Owned::new(&[], &[]),
            &Hidden::default()
        )),
        ["spotify:artist:0"],
        "following is a filter over today's library, not a record of what was shown"
    );
}

#[test]
fn an_unhidden_artist_returns() {
    let cards = [card(0), card(1)];
    let mut hidden = hidden(&[0]);

    assert_eq!(
        uris(&fresh(&cards, None, &Owned::default(), &hidden)),
        ["spotify:artist:1"],
        "hidden artists wait until the user unhides them"
    );
    assert!(hidden.unhide("spotify:artist:0"));
    assert_eq!(
        uris(&fresh(&cards, None, &Owned::default(), &hidden)),
        ["spotify:artist:0", "spotify:artist:1"],
        "an unhidden artist is offered again on the next frame"
    );
}

#[test]
fn the_order_of_the_cards_is_kept() {
    let cards = [card(3), card(1), card(4), card(0)];

    assert_eq!(
        uris(&fresh(
            &cards,
            None,
            &Owned::new(&[followed(1)], &[]),
            &hidden(&[3])
        )),
        ["spotify:artist:4", "spotify:artist:0"],
        "the lookup's order is the row's order"
    );
}
