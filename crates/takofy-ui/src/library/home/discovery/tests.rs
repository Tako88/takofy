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
        followed_at: None,
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
        tracks: Vec::new(),
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
fn an_off_lookup_lands_idle_and_turning_the_setting_on_asks_it_again() {
    let day = today();
    let mut library = ready();
    let asked = asks_on(&mut library, day);
    let seed = asked.first().cloned().expect("arrange: a row is asked");
    library.set_discovery_enabled(false);

    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed: seed.clone(),
        outcome: DiscoveryOutcome::Off,
    })));
    assert!(
        asks(&mut library).is_empty(),
        "an off lookup is not asked again while the setting is off"
    );

    library.set_discovery_enabled(true);
    assert!(
        asks(&mut library).contains(&seed),
        "the row the gate turned away is asked again once it opens"
    );
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
    // A genuine re-open: leaving Home and coming back.
    library.open(Page::Artist("spotify:artist:elsewhere".to_owned()));
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
    library.stamp_discovery(5_000);

    assert!(library.reroll_discovery(DiscoveryKind::Playlist, 7_000));
    assert!(
        !library.reroll_discovery(DiscoveryKind::Playlist, 7_500),
        "a second press before the new answer lands moves nothing"
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

/// A follow keeps its card, and the tile that undoes it, until the page is
/// opened again.
#[test]
fn a_followed_card_stays_so_the_click_can_be_taken_back() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    let uri = "spotify:artist:n1";
    land(&mut library, &seeds[0], vec![card(uri)]);

    assert_eq!(
        library.ask_follow(uri, true),
        Some(Command::SetFollow {
            artist: uri.to_owned(),
            followed: true,
        })
    );
    assert!(!library.followed_here(uri), "the write has not landed yet");

    library.apply(&Update::Followed {
        artist: uri.to_owned(),
        followed: true,
    });
    assert!(
        matches!(library.followed_artists, Load::Ready(_)),
        "the rows stay while the refresh is in flight, so no shelf loses its place"
    );
    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists),
        "the artist's name and portrait come from a read"
    );
    assert!(
        library.followed_here(uri),
        "the tile now offers the undo of that follow"
    );
    assert_eq!(
        drawn(&library.discovery_band(), DiscoveryKind::Playlist),
        [uri],
        "the card keeps its place through the reload"
    );

    let mut artists = lists().1;
    artists.push(artist(uri));
    library.apply(&Update::FollowedArtists(artists));
    assert_eq!(
        drawn(&library.discovery_band(), DiscoveryKind::Playlist),
        [uri],
        "the list holds the artist, and the card is still there on this page"
    );

    library.apply(&Update::Followed {
        artist: uri.to_owned(),
        followed: false,
    });
    assert!(!library.followed_here(uri), "the undo took it back");
    assert_eq!(
        drawn(&library.discovery_band(), DiscoveryKind::Playlist),
        [uri],
        "an unfollowed artist is fresh again"
    );
}

/// The next page does not carry it: an artist you follow is not outside your
/// library.
#[test]
fn the_next_open_drops_a_card_followed_here() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    let uri = "spotify:artist:n1";
    land(&mut library, &seeds[0], vec![card(uri)]);

    library.ask_follow(uri, true);
    library.apply(&Update::Followed {
        artist: uri.to_owned(),
        followed: true,
    });
    let mut artists = lists().1;
    artists.push(artist(uri));
    library.apply(&Update::FollowedArtists(artists));

    // A genuine re-open: leaving Home and coming back.
    library.open(Page::Artist("spotify:artist:elsewhere".to_owned()));
    library.go_home(NOW_MS + 1_000);
    // A new page re-reads the lists, so the band is rebuilt from answers that
    // have landed again rather than from the last page's.
    let mut artists = lists().1;
    artists.push(artist(uri));
    library.apply(&Update::FollowedArtists(artists));
    let _ = asks_on(&mut library, today());
    let asked = asks(&mut library);
    if let Some(seed) = asked.first() {
        land(&mut library, seed, vec![card(uri)]);
    }
    let band = library.discovery_band();
    let shown = drawn(&band, DiscoveryKind::Playlist);
    assert!(
        !shown.contains(&uri),
        "a followed artist is no longer outside the library: {shown:?}"
    );
}

#[test]
fn a_failed_follow_keeps_the_card_and_lets_the_tile_act_again() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    let uri = "spotify:artist:n1";
    land(&mut library, &seeds[0], vec![card(uri)]);
    assert!(library.ask_follow(uri, true).is_some());

    library.apply(&Update::FollowFailed {
        artist: uri.to_owned(),
        failure: Failure::Save,
    });

    assert!(!library.follow_pending(uri), "the tile retries");
    assert_eq!(
        drawn(&library.discovery_band(), DiscoveryKind::Playlist),
        [uri],
        "a write that did not stick changes nothing"
    );
}

/// A follow that lands while the list is out keeps the card: `settle_follow`
/// has no list to remove it from. The guard still clears, and pressing again is
/// an idempotent write.
#[test]
fn a_follow_that_lands_while_the_list_is_out_keeps_the_card_and_clears_the_guard() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    let uri = "spotify:artist:n1";
    land(&mut library, &seeds[0], vec![card(uri)]);

    library.followed_artists = Load::Idle;
    assert!(library.ask_follow(uri, true).is_some());
    library.apply(&Update::Followed {
        artist: uri.to_owned(),
        followed: true,
    });
    assert!(!library.follow_pending(uri), "the answer clears the guard");

    library.apply(&Update::FollowedArtists(lists().1));
    assert_eq!(
        drawn(&library.discovery_band(), DiscoveryKind::Playlist),
        [uri],
        "the stale list never held the artist, so the card stays"
    );
    assert!(
        library.ask_follow(uri, true).is_some(),
        "a second press is an idempotent write"
    );
}

#[test]
fn a_hide_drops_the_artist_from_every_row_it_is_drawn_in() {
    let mut library = ready();
    let seeds = asks_on(&mut library, today());
    land(
        &mut library,
        &seeds[0],
        vec![card("spotify:artist:n1"), card("spotify:artist:n2")],
    );
    land(
        &mut library,
        &seeds[1],
        vec![card("spotify:artist:n3"), card("spotify:artist:n4")],
    );

    library.hide_artist("spotify:artist:n1");
    library.hide_artist("spotify:artist:n3");

    let band = library.discovery_band();
    assert_eq!(drawn(&band, DiscoveryKind::Playlist), ["spotify:artist:n2"]);
    assert_eq!(drawn(&band, DiscoveryKind::Artist), ["spotify:artist:n4"]);
}

#[test]
fn a_hide_keeps_the_artist_out_of_a_day_built_after_a_restart() {
    let day = today();
    let mut library = ready();
    let seeds = asks_on(&mut library, day);
    land(
        &mut library,
        &seeds[0],
        vec![card("spotify:artist:n1"), card("spotify:artist:n2")],
    );
    let before = library.hidden().revision();
    library.hide_artist("spotify:artist:n1");
    assert!(
        library.hidden().revision() > before,
        "the hide is a change the file records"
    );

    let mut restarted = ready();
    restarted.restore_hidden(library.hidden().clone());
    let seeds = asks_on(&mut restarted, day);
    land(
        &mut restarted,
        &seeds[0],
        vec![card("spotify:artist:n1"), card("spotify:artist:n2")],
    );

    assert_eq!(
        drawn(&restarted.discovery_band(), DiscoveryKind::Playlist),
        ["spotify:artist:n2"],
        "a day built from the stored set does not offer the artist again"
    );
}

/// One build of the owned set serves every ask in a frame and the draw after it.
///
/// The set walks every saved album's credits, and the band asks for it on both
/// the ask path and the draw path.
#[test]
fn one_owned_set_serves_the_asks_and_the_draw() {
    let mut library = ready();
    let _ = asks_on(&mut library, today());

    super::reset_owned_builds();
    let _ = library.home_asks(Instant::now());
    let _ = library.discovery_band();

    assert_eq!(
        super::owned_builds(),
        1,
        "the frame built the owned set more than once"
    );
}
