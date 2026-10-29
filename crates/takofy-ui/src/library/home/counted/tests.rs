use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_model::{CoverSet, CoverUrl, FollowedArtist, LikedTrack, SavedAlbum, TrackEntry};

use super::*;
use crate::home::{Presence, Section};
use crate::message::{Failure, Update};

const NOW_MS: u64 = 1_700_000_000_000;

/// One cover, from the CDN's own path for it.
fn cover(tail: &str) -> CoverSet {
    let url = CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url");
    CoverSet::single(url)
}

/// A saved album that names `artists`.
fn album(uri: &str, artists: &[(&str, &str)]) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: artists
            .iter()
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(", "),
        artist_uris: artists
            .iter()
            .map(|(artist, _)| (*artist).to_owned())
            .collect(),
        artist_names: artists.iter().map(|(_, name)| (*name).to_owned()).collect(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

/// A liked song, as the liked read names it.
fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: None,
    }
}

/// A track the label read named, credited to `artists`.
fn label(uri: &str, artists: &[(&str, &str)]) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: artists
            .iter()
            .map(|(artist, _)| (*artist).to_owned())
            .collect(),
        artist_names: artists.iter().map(|(_, name)| (*name).to_owned()).collect(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// A track the read could not name: it fell back to its own URI.
fn missed(uri: &str) -> TrackEntry {
    TrackEntry {
        title: uri.to_owned(),
        named: false,
        ..label(uri, &[])
    }
}

/// A followed artist, with `portrait` when the read gave one.
fn followed(uri: &str, portrait: Option<CoverSet>) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait,
        releases: None,
    }
}

/// A library on Home whose three sources have all answered with these lists.
fn settled(albums: Vec<SavedAlbum>, likes: Vec<LikedTrack>) -> Library {
    let mut library = Library::default();
    library.home.open(NOW_MS);
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::SavedAlbums(albums));
    assert!(
        library.likes_request().is_some(),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(likes));
    library
}

/// The URIs the ask names, from the one read it sends.
fn asked(library: &mut Library) -> Vec<String> {
    match library.counted_ask() {
        Some(Command::LoadHome(HomeAsk::Labels { tracks })) => tracks,
        other => panic!("expected one labels read, got {other:?}"),
    }
}

/// Lands the answer to the ask `tracks`, naming each with `named`.
fn land(library: &mut Library, tracks: Vec<String>, named: Vec<TrackEntry>) {
    library.apply(&Update::Home(HomeAnswer::Labelled {
        asked: tracks,
        named,
    }));
}

/// The URIs the section's tiles draw, in ranking order.
fn tile_uris(section: &Section<Tiles<'_>>) -> Vec<String> {
    match section {
        Section::Ready(tiles) => tiles
            .artists
            .iter()
            .map(|artist| artist.uri.to_owned())
            .collect(),
        section => panic!("expected tiles, got {section:?}"),
    }
}

#[test]
fn the_ask_waits_for_the_reads_the_ranking_is_built_from() {
    let mut library = Library::default();
    library.home.open(NOW_MS);
    assert_eq!(library.counted_ask(), None, "nothing is known yet");

    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.counted_ask(), None, "the albums have not answered");

    library.apply(&Update::SavedAlbums(Vec::new()));
    assert_eq!(
        library.counted_ask(),
        None,
        "the liked songs have not answered"
    );

    assert!(library.likes_request().is_some());
    library.apply(&Update::LikedTracks(vec![liked("spotify:track:a")]));
    assert!(library.counted_ask().is_some(), "every source has answered");
}

#[test]
fn a_library_with_nothing_liked_asks_for_nothing() {
    let mut library = settled(
        vec![album("spotify:album:a", &[("spotify:artist:a", "A")])],
        Vec::new(),
    );
    assert_eq!(
        library.counted_ask(),
        None,
        "there are no labels to wait for"
    );
    assert_eq!(
        tile_uris(&library.counted_section()),
        vec!["spotify:artist:a"],
        "the saved albums rank without a read"
    );
}

#[test]
fn the_ask_names_every_liked_song_once_in_saved_order() {
    let likes: Vec<LikedTrack> = (0..600)
        .map(|index| liked(&format!("spotify:track:{index}")))
        .collect();
    let mut library = settled(Vec::new(), likes);

    let tracks = asked(&mut library);
    assert_eq!(tracks.len(), 600, "every like, not a recent sample");
    assert_eq!(tracks.first().map(String::as_str), Some("spotify:track:0"));
    assert_eq!(
        tracks.last().map(String::as_str),
        Some("spotify:track:599"),
        "the oldest like is asked for too"
    );
    assert_eq!(library.counted_ask(), None, "the ask is sent once");
}

#[test]
fn one_song_listed_twice_is_asked_for_once() {
    let mut library = settled(
        Vec::new(),
        vec![
            liked("spotify:track:a"),
            liked("spotify:track:a"),
            liked("spotify:track:b"),
        ],
    );
    assert_eq!(
        asked(&mut library),
        vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()]
    );
}

#[test]
fn the_ask_spends_the_one_slot_and_a_retry_gives_it_back() {
    let mut library = settled(Vec::new(), vec![liked("spotify:track:a")]);
    assert_eq!(library.home.budget.left(Slot::Counted), 1);
    assert!(library.counted_ask().is_some());
    assert_eq!(library.home.budget.left(Slot::Counted), 0);

    assert_eq!(library.retry_home(Slot::Counted), None);
    assert_eq!(library.home.budget.left(Slot::Counted), 1);
    assert!(
        library.counted_ask().is_some(),
        "a retry asks for the labels again"
    );
}

#[test]
fn the_shelf_waits_whole_until_the_answer_lands() {
    let mut library = settled(
        vec![album("spotify:album:a", &[("spotify:artist:a", "A")])],
        vec![liked("spotify:track:a")],
    );
    asked(&mut library);
    assert_eq!(
        library.counted_section().presence(),
        Presence::Waiting,
        "a ranking drawn before the answer would reshuffle"
    );
}

#[test]
fn an_answer_to_another_ask_is_ignored() {
    let mut library = settled(Vec::new(), vec![liked("spotify:track:a")]);
    asked(&mut library);
    land(
        &mut library,
        vec!["spotify:track:stale".to_owned()],
        vec![label("spotify:track:stale", &[("spotify:artist:x", "X")])],
    );
    assert_eq!(library.counted_section().presence(), Presence::Waiting);

    land(
        &mut library,
        vec!["spotify:track:a".to_owned()],
        vec![label("spotify:track:a", &[("spotify:artist:x", "X")])],
    );
    assert_eq!(library.counted_section().presence(), Presence::Ready);
}

#[test]
fn the_ranking_counts_the_albums_and_the_liked_songs_together() {
    let mut library = settled(
        vec![album(
            "spotify:album:a",
            &[("spotify:artist:a", "A"), ("spotify:artist:b", "B")],
        )],
        vec![liked("spotify:track:1")],
    );
    let tracks = asked(&mut library);
    land(
        &mut library,
        tracks,
        vec![label("spotify:track:1", &[("spotify:artist:a", "A")])],
    );
    assert_eq!(
        tile_uris(&library.counted_section()),
        vec!["spotify:artist:a", "spotify:artist:b"],
        "the artist two items name ranks above the one item that names another"
    );
}

#[test]
fn an_artist_named_twice_on_one_item_holds_it_once() {
    // The first album names A, the second names B twice, and the liked song
    // names C twice. Were a repeat two holdings, B would outrank A and C would
    // outrank both; held once per item the three tie and keep the library's
    // order.
    let mut library = settled(
        vec![
            album("spotify:album:a", &[("spotify:artist:a", "A")]),
            album(
                "spotify:album:b",
                &[("spotify:artist:b", "B"), ("spotify:artist:b", "B")],
            ),
        ],
        vec![liked("spotify:track:1")],
    );
    let tracks = asked(&mut library);
    land(
        &mut library,
        tracks,
        vec![label(
            "spotify:track:1",
            &[("spotify:artist:c", "C"), ("spotify:artist:c", "C")],
        )],
    );
    assert_eq!(
        tile_uris(&library.counted_section()),
        vec!["spotify:artist:a", "spotify:artist:b", "spotify:artist:c"]
    );
}

#[test]
fn a_read_that_falls_back_to_its_uris_fails_the_shelf() {
    let likes: Vec<LikedTrack> = (0..4)
        .map(|index| liked(&format!("spotify:track:{index}")))
        .collect();
    let mut library = settled(Vec::new(), likes);
    let tracks = asked(&mut library);
    let named: Vec<TrackEntry> = tracks.iter().map(|uri| missed(uri)).collect();
    land(&mut library, tracks, named);

    assert!(
        matches!(library.counted_section(), Section::Failed(Failure::Load)),
        "a read that named nothing is a failure, not an empty library"
    );
    assert_eq!(library.retry_home(Slot::Counted), None);
    assert!(library.counted_ask().is_some(), "the retry reads again");
}

#[test]
fn a_read_at_the_tolerance_is_ranked_from_what_it_named() {
    let likes: Vec<LikedTrack> = (0..200)
        .map(|index| liked(&format!("spotify:track:{index}")))
        .collect();
    let mut library = settled(Vec::new(), likes);
    let tracks = asked(&mut library);
    let mut named: Vec<TrackEntry> = tracks
        .iter()
        .take(198)
        .map(|uri| label(uri, &[("spotify:artist:a", "A")]))
        .collect();
    named.extend(tracks.iter().skip(198).map(|uri| missed(uri)));
    land(&mut library, tracks, named);

    assert_eq!(
        tile_uris(&library.counted_section()),
        vec!["spotify:artist:a"],
        "one percent unnamed is still the library's own ranking"
    );
}

#[test]
fn a_wire_failure_is_the_shelf_s_own() {
    let mut library = settled(Vec::new(), vec![liked("spotify:track:a")]);
    let tracks = asked(&mut library);
    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Labels { tracks },
        failure: Failure::Load,
    });
    assert!(matches!(
        library.counted_section(),
        Section::Failed(Failure::Load)
    ));
}

#[test]
fn another_shelf_s_failure_is_not_this_shelf_s() {
    let mut library = settled(Vec::new(), vec![liked("spotify:track:a")]);
    asked(&mut library);
    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Related {
            artist: "spotify:artist:a".to_owned(),
        },
        failure: Failure::Related,
    });
    assert_eq!(
        library.counted_section().presence(),
        Presence::Waiting,
        "the labels are still in flight"
    );
}

#[test]
fn nothing_held_is_an_absent_shelf() {
    let mut library = settled(Vec::new(), Vec::new());
    assert_eq!(library.counted_ask(), None);
    assert!(matches!(library.counted_section(), Section::Absent));
}

#[test]
fn an_unreadable_rootlist_takes_the_shelf_with_it() {
    let mut library = Library::default();
    library.home.open(NOW_MS);
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert!(matches!(
        library.counted_section(),
        Section::Failed(Failure::Load)
    ));
}

#[test]
fn a_portrait_comes_from_the_followed_artists() {
    let mut library = settled(
        vec![album(
            "spotify:album:a",
            &[("spotify:artist:a", "A"), ("spotify:artist:b", "B")],
        )],
        Vec::new(),
    );
    assert_eq!(library.counted_ask(), None, "nothing is liked");
    library.apply(&Update::FollowedArtists(vec![
        followed("spotify:artist:a", Some(cover("aaaaaaaaaaaaaaaaaaaa"))),
        followed("spotify:artist:b", None),
    ]));

    match library.counted_section() {
        Section::Ready(tiles) => {
            let portraits: Vec<bool> = tiles
                .artists
                .iter()
                .map(|artist| artist.portrait.is_some())
                .collect();
            assert_eq!(portraits, vec![true, false]);
        }
        section => panic!("expected tiles, got {section:?}"),
    }
}

#[test]
fn a_portrait_is_missing_until_the_followed_artists_answer() {
    let mut library = settled(
        vec![album("spotify:album:a", &[("spotify:artist:a", "A")])],
        Vec::new(),
    );
    assert_eq!(library.counted_ask(), None, "nothing is liked");
    match library.counted_section() {
        Section::Ready(tiles) => assert_eq!(tiles.artists[0].portrait, None),
        section => panic!("expected tiles, got {section:?}"),
    }
}
