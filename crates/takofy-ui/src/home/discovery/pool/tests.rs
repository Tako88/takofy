use takofy_model::home::{Day, DiscoveryKind};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use super::*;

fn today() -> Day {
    Day::from_days(19_000)
}

fn playlist(index: usize) -> PlaylistEntry {
    PlaylistEntry {
        uri: format!("spotify:playlist:{index}"),
        name: format!("{index}"),
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

fn playlists(count: usize) -> Vec<PlaylistEntry> {
    (0..count).map(playlist).collect()
}

fn artist(index: usize) -> FollowedArtist {
    FollowedArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("{index}"),
        named: true,
        portrait: None,
        releases: None,
        followed_at: None,
    }
}

fn artists(count: usize) -> Vec<FollowedArtist> {
    (0..count).map(artist).collect()
}

fn album(index: usize, credited: &[&str]) -> SavedAlbum {
    SavedAlbum {
        uri: format!("spotify:album:{index}"),
        name: format!("{index}"),
        named: true,
        artists: credited.join(", "),
        artist_uris: credited
            .iter()
            .map(|name| format!("spotify:artist:{name}"))
            .collect(),
        artist_names: credited.iter().map(|name| (*name).to_owned()).collect(),
        year: None,
        cover: None,
        tracks: Some(10),
        saved_at: None,
    }
}

fn albums(count: usize) -> Vec<SavedAlbum> {
    (0..count)
        .map(|index| album(index, &[&format!("{index}")]))
        .collect()
}

fn order(kind: usize) -> Vec<String> {
    day_pool(&playlists(8), &artists(8), &albums(8), today())[kind]
        .iter()
        .map(|seed| seed.name.clone())
        .collect()
}

#[test]
fn the_same_library_on_the_same_day_draws_the_same_pool() {
    let once = day_pool(&playlists(30), &artists(30), &albums(30), today());
    let twice = day_pool(&playlists(30), &artists(30), &albums(30), today());

    assert_eq!(
        once, twice,
        "the pool is a function of the day and the lists, with nothing else to vary"
    );
}

#[test]
fn a_rich_library_gives_twelve_candidates() {
    let pool = day_pool(&playlists(30), &artists(30), &albums(30), today());

    assert_eq!(
        pool.iter().map(Vec::len).collect::<Vec<_>>(),
        [PER_KIND, PER_KIND, PER_KIND],
        "four candidates per kind, for three kinds"
    );
    for (kind, seeds) in pool.iter().enumerate() {
        assert!(
            seeds.iter().all(|seed| seed.kind.index() == kind),
            "each row's candidates carry that row's kind"
        );
    }
}

#[test]
fn a_playlist_seed_names_no_anchor_and_the_others_name_theirs() {
    let pool = day_pool(&playlists(4), &artists(4), &albums(4), today());

    assert!(
        pool[0].iter().all(|seed| seed.anchor.is_none()),
        "a playlist's artist is the lookup's to find, not the pool's to guess"
    );
    for seed in &pool[1] {
        assert_eq!(
            seed.anchor.as_ref().map(|anchor| anchor.uri.as_str()),
            Some(seed.uri.as_str()),
            "an artist seed is reached from itself"
        );
    }
    let albums = [album(0, &["a"]), album(1, &["b", "c"])];
    for seed in &day_pool(&[], &[], &albums, today())[2] {
        let anchor = seed
            .anchor
            .as_ref()
            .expect("an album names its first artist");
        let expected = match seed.uri.as_str() {
            "spotify:album:0" => "a",
            "spotify:album:1" => "b",
            other => panic!("unexpected album seed {other}"),
        };
        assert_eq!(
            anchor.name, expected,
            "an album is reached from its first credited artist"
        );
    }
}

#[test]
fn a_different_day_draws_a_different_order() {
    let now = day_pool(&playlists(8), &artists(8), &albums(8), today());
    let next = day_pool(
        &playlists(8),
        &artists(8),
        &albums(8),
        Day::from_days(19_001),
    );

    assert_ne!(
        now, next,
        "the day is what the shuffle is mixed from, so tomorrow offers other seeds"
    );
}

#[test]
fn each_kind_is_mixed_on_its_own() {
    assert_ne!(
        mix(today(), 0),
        mix(today(), 1),
        "two rows never share a mix"
    );
    assert_ne!(
        mix(today(), 1),
        mix(today(), 2),
        "two rows never share a mix"
    );
    assert_ne!(
        mix(today(), 0),
        mix(today(), 2),
        "two rows never share a mix"
    );

    let drawn = [order(0), order(1), order(2)];

    assert_ne!(
        drawn[0], drawn[1],
        "the same eight names in two kinds are not drawn into one order"
    );
    assert_ne!(
        drawn[1], drawn[2],
        "the same eight names in two kinds are not drawn into one order"
    );
}

#[test]
fn the_whole_list_is_the_edge_set_up_to_twice_the_edge() {
    let pool = day_pool(&playlists(2 * EDGE), &[], &[], today());

    assert_eq!(pool[0].len(), PER_KIND, "a full pool is still its size");
    assert_eq!(pool[1], Vec::new(), "a kind with nothing usable is empty");
    assert_eq!(pool[2], Vec::new(), "a kind with nothing usable is empty");
}

#[test]
fn a_list_longer_than_twice_the_edge_draws_from_its_ends_only() {
    let long = 2 * EDGE + 1;
    let pool = day_pool(&playlists(long), &[], &[], today());

    assert_eq!(
        pool[0].len(),
        PER_KIND,
        "only the ends compete, so the middle never appears"
    );
    for seed in &pool[0] {
        let index: usize = seed.name.parse().expect("a name is its index");
        assert!(
            index < EDGE || index >= long - EDGE,
            "seed {index} is in the middle the edge set never reaches"
        );
    }
}

#[test]
fn a_list_of_twice_the_edge_competes_whole() {
    let middle = |day: Day| {
        day_pool(&playlists(2 * EDGE), &[], &[], day)[0]
            .iter()
            .any(|seed| seed.name.parse::<usize>().is_ok_and(|index| index >= EDGE))
    };

    assert!(
        (19_000..19_010).any(|days| middle(Day::from_days(days))),
        "at twice the edge the whole list competes, so the middle can be drawn"
    );
}

#[test]
fn a_list_of_one_draws_that_one() {
    let pool = day_pool(&playlists(1), &artists(1), &albums(1), today());

    for seeds in &pool {
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].name, "0");
    }
}

#[test]
fn an_empty_library_gives_no_pool() {
    assert_eq!(
        day_pool(&[], &[], &[], today()),
        [Vec::new(), Vec::new(), Vec::new()],
        "three empty rows, not three rows of nothing to draw"
    );
}

#[test]
fn unnamed_and_empty_items_are_skipped() {
    let mut unnamed_playlist = playlist(0);
    unnamed_playlist.named = false;
    let mut empty_playlist = playlist(1);
    empty_playlist.tracks = Some(0);
    let mut unnamed_artist = artist(0);
    unnamed_artist.named = false;
    let mut unnamed_album = album(0, &["a"]);
    unnamed_album.named = false;
    let mut uncredited_album = album(1, &[]);
    uncredited_album.artist_uris.clear();

    let pool = day_pool(
        &[unnamed_playlist, empty_playlist],
        &[unnamed_artist],
        &[unnamed_album, uncredited_album],
        today(),
    );

    assert_eq!(
        pool,
        [Vec::new(), Vec::new(), Vec::new()],
        "an item that draws no title, or a playlist with no rows, is no seed"
    );
}

#[test]
fn a_playlist_with_an_unread_length_is_still_a_seed() {
    let mut unknown = playlist(0);
    unknown.tracks = None;

    assert_eq!(
        day_pool(&[unknown], &[], &[], today())[0].len(),
        1,
        "a count the wire never named is not an empty playlist"
    );
}

#[test]
fn duplicates_collapse_to_the_first_in_list_order() {
    let mut second = playlist(0);
    second.name = "second".to_owned();
    let mut second_album = album(1, &["a"]);
    second_album.name = "second".to_owned();

    let pool = day_pool(
        &[playlist(0), second],
        &[artist(0), artist(0)],
        &[album(0, &["a"]), second_album],
        today(),
    );

    assert_eq!(
        pool[0].len(),
        1,
        "one playlist listed twice is one candidate"
    );
    assert_eq!(pool[0][0].name, "0", "the first in list order wins");
    assert_eq!(pool[1].len(), 1, "one artist listed twice is one candidate");
    assert_eq!(
        pool[2].len(),
        1,
        "two albums by one artist are one candidate for that artist"
    );
    assert_eq!(pool[2][0].name, "0", "the first in list order wins");
}

#[test]
fn an_album_credited_to_various_artists_is_skipped() {
    let pool = day_pool(&[], &[], &[album(0, &["Various Artists"])], today());

    assert_eq!(
        pool[2],
        Vec::new(),
        "a compilation's credited artist is not an artist to find neighbours of"
    );
}

#[test]
fn a_second_credit_can_anchor_an_album() {
    let pool = day_pool(&[], &[], &[album(0, &["first", "second"])], today());

    assert_eq!(
        pool[2][0]
            .anchor
            .as_ref()
            .map(|anchor| anchor.name.as_str()),
        Some("first"),
        "the album is reached from its first credited artist, not its second"
    );
}

#[test]
fn every_kind_keeps_its_own_order() {
    let pool = day_pool(&playlists(30), &artists(30), &albums(30), today());

    for seeds in &pool {
        assert_eq!(
            seeds.len(),
            PER_KIND,
            "each kind is drawn independently of the others' lengths"
        );
    }
    assert_eq!(
        pool[0][0].kind,
        DiscoveryKind::Playlist,
        "the first row is the playlists"
    );
    assert_eq!(pool[1][0].kind, DiscoveryKind::Artist);
    assert_eq!(pool[2][0].kind, DiscoveryKind::Album);
}
