use std::time::Instant;

use takofy_model::home::{
    Day, DiscoveryAnswer, DiscoveryArtist, DiscoveryCard, DiscoveryKind, DiscoveryOutcome,
    DiscoverySeed, HomeAnswer,
};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use super::*;
use crate::home::discovery::RowRead;
use crate::home::discovery::pool;
use crate::message::{Failure, Update};
use crate::route::Page;

const NOW_MS: u64 = 1_700_000_000_000;

/// The day the page is open on in these tests.
fn today() -> Day {
    Day::from_days(20_253)
}

fn playlist(uri: &str) -> PlaylistEntry {
    PlaylistEntry {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner: None,
        access: None,
        saved_at: None,
    }
}

fn artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

/// An album saved from `artist`, which is what its row is seeded from.
fn album(uri: &str, artist: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: artist.to_owned(),
        artist_uris: vec![artist.to_owned()],
        artist_names: vec![artist.to_owned()],
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

/// The three lists the day's pools are drawn from, four deep each.
fn lists() -> (Vec<PlaylistEntry>, Vec<FollowedArtist>, Vec<SavedAlbum>) {
    (
        ["a", "b", "c", "d"]
            .map(|tail| playlist(&format!("spotify:playlist:{tail}")))
            .to_vec(),
        ["a", "b", "c", "d"]
            .map(|tail| artist(&format!("spotify:artist:{tail}")))
            .to_vec(),
        ["a", "b", "c", "d"]
            .map(|tail| {
                album(
                    &format!("spotify:album:{tail}"),
                    &format!("spotify:artist:saved-{tail}"),
                )
            })
            .to_vec(),
    )
}

/// The day's pools, as the row machine draws them.
fn pools(day: Day) -> [Vec<DiscoverySeed>; 3] {
    let (playlists, artists, albums) = lists();
    pool::day_pool(&playlists, &artists, &albums, day)
}

/// A Home library whose three pools' lists have all answered.
fn ready() -> Library {
    let (playlists, artists, albums) = lists();
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(playlists));
    library.apply(&Update::SavedAlbums(albums));
    library.apply(&Update::FollowedArtists(artists));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(Vec::new()));
    library
}

/// One frame's discovery asks, in the order it made them.
fn asks(library: &mut Library) -> Vec<DiscoverySeed> {
    let commands = library.home_asks(Instant::now());
    commands
        .into_iter()
        .filter_map(|command| match command {
            Command::LoadHome(HomeAsk::Discovery { seed }) => Some(seed),
            _ => None,
        })
        .collect()
}

/// The day named for the frame and that frame's discovery asks.
fn asks_on(library: &mut Library, day: Day) -> Vec<DiscoverySeed> {
    library.set_local_day(day);
    asks(library)
}

/// A card for `artist`, as a lookup would find it.
fn card(artist: &str) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: artist.to_owned(),
            name: artist.to_owned(),
        },
        portrait: None,
        top_track: "spotify:track:top".to_owned(),
        top_title: "Top".to_owned(),
        latest_year: None,
    }
}

/// Lands `cards` on the row `seed` was asked for.
fn land(library: &mut Library, seed: &DiscoverySeed, cards: Vec<DiscoveryCard>) {
    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed: seed.clone(),
        outcome: DiscoveryOutcome::Neighbours { cards, genre: None },
    })));
}

/// Answers the row `seed` was asked for with nothing it can identify.
fn reject(library: &mut Library, seed: &DiscoverySeed) {
    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed: seed.clone(),
        outcome: DiscoveryOutcome::Rejected,
    })));
}

/// The artist URIs the row `kind` draws this frame.
fn drawn<'a>(band: &'a Band<'a>, kind: DiscoveryKind) -> Vec<&'a str> {
    match band {
        Band::Rows(rows) => rows
            .iter()
            .filter(|row| row.seed.kind == kind)
            .flat_map(|row| row.cards.iter().map(|card| card.artist.uri.as_str()))
            .collect(),
        Band::Off | Band::Waiting | Band::Absent | Band::Unreachable => Vec::new(),
    }
}

/// What the row `kind` is doing this frame, or `None` when it draws nothing.
fn read_of(band: &Band<'_>, kind: DiscoveryKind) -> Option<RowRead> {
    match band {
        Band::Rows(rows) => rows
            .iter()
            .find(|row| row.seed.kind == kind)
            .map(|row| row.read),
        Band::Off | Band::Waiting | Band::Absent | Band::Unreachable => None,
    }
}

#[test]
fn a_row_the_setting_turned_off_is_never_asked() {
    let mut library = ready();
    library.set_discovery_enabled(false);

    let mut asked: Vec<DiscoverySeed> = Vec::new();
    for _ in 0..1_000 {
        asked.extend(asks_on(&mut library, today()));
    }

    assert!(asked.is_empty(), "an off row is not read: {asked:?}");
    assert_eq!(library.discovery_band(), Band::Off);
}

#[test]
fn nothing_is_asked_before_the_day_is_named() {
    let mut library = ready();

    assert!(asks(&mut library).is_empty());
    assert_eq!(library.discovery_band(), Band::Waiting);
}

#[test]
fn a_failed_list_leaves_no_ask_and_no_band() {
    let mut rootlist = ready();
    rootlist.set_local_day(today());
    rootlist.playlists = Load::Failed(Failure::Load);
    assert!(
        asks(&mut rootlist).is_empty(),
        "a lost list has no pool to seed a row from"
    );
    assert_eq!(rootlist.discovery_band(), Band::Absent);

    let mut albums = ready();
    albums.set_local_day(today());
    albums.saved_albums = Load::Failed(Failure::SavedAlbums);
    assert!(asks(&mut albums).is_empty());
    assert_eq!(albums.discovery_band(), Band::Absent);

    let mut artists = ready();
    artists.set_local_day(today());
    artists.followed_artists = Load::Failed(Failure::FollowedArtists);
    assert!(asks(&mut artists).is_empty());
    assert_eq!(artists.discovery_band(), Band::Absent);
}

#[test]
fn a_list_still_out_waits() {
    let mut library = ready();
    library.set_local_day(today());
    library.saved_albums = Load::Loading;

    assert!(asks(&mut library).is_empty());
    assert_eq!(library.discovery_band(), Band::Waiting);
}

#[test]
fn each_row_asks_once_and_a_new_open_asks_again() {
    let day = today();
    let pools = pools(day);
    let mut library = ready();
    let first = vec![
        pools[0][0].clone(),
        pools[1][0].clone(),
        pools[2][0].clone(),
    ];

    let frame = asks_on(&mut library, day);
    assert_eq!(frame, first);
    assert!(
        asks(&mut library).is_empty(),
        "a row whose answer has not come is not asked again"
    );
    assert!(asks(&mut library).is_empty(), "nor on a later frame");

    library.open(Page::Album("spotify:album:x".to_owned()));
    library.go_home(NOW_MS + 1_000);

    assert_eq!(
        asks_on(&mut library, day),
        first,
        "a second open on the same day asks the same seeds again"
    );
}

#[test]
fn hiding_an_artist_takes_it_out_of_the_band_and_unhiding_puts_it_back() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    land(
        &mut library,
        &seeds[0],
        vec![card("spotify:artist:n1"), card("spotify:artist:n2")],
    );

    let band = library.discovery_band();
    assert_eq!(
        drawn(&band, DiscoveryKind::Playlist),
        ["spotify:artist:n1", "spotify:artist:n2"]
    );

    library.hide_artist("spotify:artist:n1");
    let band = library.discovery_band();
    assert_eq!(
        drawn(&band, DiscoveryKind::Playlist),
        ["spotify:artist:n2"],
        "a hidden artist is not drawn again"
    );
    assert!(
        asks(&mut library).is_empty(),
        "hiding one of a row's cards is no reason to read again"
    );

    library.unhide_artist("spotify:artist:n1");
    let band = library.discovery_band();
    assert_eq!(
        drawn(&band, DiscoveryKind::Playlist),
        ["spotify:artist:n1", "spotify:artist:n2"]
    );
}

#[test]
fn a_hidden_artist_the_library_already_holds_is_never_offered() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    land(
        &mut library,
        &seeds[0],
        vec![card("spotify:artist:a"), card("spotify:artist:n1")],
    );

    let band = library.discovery_band();

    assert_eq!(
        drawn(&band, DiscoveryKind::Playlist),
        ["spotify:artist:n1"],
        "the followed artist the row was seeded beside is not a discovery"
    );
}

#[test]
fn a_hide_that_empties_a_landed_row_spends_its_one_re_seed() {
    let day = today();
    let pools = pools(day);
    let mut library = ready();
    let seeds = asks_on(&mut library, day);
    land(&mut library, &seeds[0], vec![card("spotify:artist:n1")]);
    library.hide_artist("spotify:artist:n1");

    assert_eq!(
        asks(&mut library),
        [pools[0][1].clone()],
        "an emptied row moves on to its next seed"
    );

    land(&mut library, &pools[0][1], vec![card("spotify:artist:n2")]);
    library.hide_artist("spotify:artist:n2");

    assert!(
        asks(&mut library).is_empty(),
        "a second emptiness has no re-seed left to spend"
    );
    let band = library.discovery_band();
    assert_eq!(
        read_of(&band, DiscoveryKind::Playlist),
        None,
        "the row is gone rather than drawn empty"
    );
}

#[test]
fn the_lookup_goes_after_every_shelf() {
    let mut library = ready();
    library.set_local_day(today());

    let commands = library.home_asks(Instant::now());
    let at = commands
        .iter()
        .position(|command| matches!(command, Command::LoadHome(HomeAsk::Discovery { .. })))
        .unwrap_or_else(|| panic!("arrange: the frame owes a lookup: {commands:?}"));

    assert!(
        commands[at..]
            .iter()
            .all(|command| matches!(command, Command::LoadHome(HomeAsk::Discovery { .. }))),
        "the lookup goes last, so nothing above it waits: {commands:?}"
    );
}

#[test]
fn a_reroll_moves_the_row_on_to_its_next_seed() {
    let day = today();
    let pools = pools(day);
    let mut library = ready();
    let seeds = asks_on(&mut library, day);
    land(&mut library, &seeds[0], vec![card("spotify:artist:n1")]);

    assert!(library.reroll_discovery(DiscoveryKind::Playlist, 1_000));
    assert!(
        !library.reroll_discovery(DiscoveryKind::Playlist, 1_500),
        "a second press inside the cooldown moves nothing"
    );

    assert_eq!(asks(&mut library), [pools[0][1].clone()]);
}

#[test]
fn positions_are_remembered_for_their_own_day_only() {
    let day = today();
    let other = Day::from_days(day.days() + 1);
    let mine = pools(day);
    let mut library = ready();
    let seeds = asks_on(&mut library, day);
    assert_eq!(library.hidden().positions_for(day), Some([0, 0, 0]));

    land(&mut library, &seeds[0], vec![card("spotify:artist:n1")]);
    library.hide_artist("spotify:artist:n1");
    assert_eq!(asks(&mut library).first(), Some(&mine[0][1]));
    assert_eq!(
        library.hidden().positions_for(day),
        Some([1, 0, 0]),
        "where the day's rows stand is recorded"
    );

    let mut restarted = ready();
    restarted.restore_hidden(library.hidden().clone());
    assert_eq!(
        asks_on(&mut restarted, day).first(),
        Some(&mine[0][1]),
        "a restart on the same day carries on from the stored position"
    );

    let mut tomorrow = ready();
    tomorrow.restore_hidden(library.hidden().clone());
    assert_eq!(
        asks_on(&mut tomorrow, other).first(),
        Some(&pools(other)[0][0]),
        "another day's pool starts from its own beginning"
    );
}

#[test]
fn a_library_change_mid_day_keeps_the_rows_it_drew() {
    let day = today();
    let pools = pools(day);
    let mut library = ready();
    let seeds = asks_on(&mut library, day);

    library.apply(&Update::Playlists(vec![playlist("spotify:playlist:new")]));
    reject(&mut library, &seeds[0]);

    assert_eq!(
        asks(&mut library).first(),
        Some(&pools[0][1]),
        "the day's pool is drawn once, not drawn again from a changed list"
    );
}
