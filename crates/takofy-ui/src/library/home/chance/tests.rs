use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, Station, StationSeed};
use takofy_model::{FollowedArtist, LikedTrack, PlaylistEntry, SavedAlbum, TrackEntry};

use super::*;
use crate::home::chance::{READS_PER_PRESS, SKIPS};
use crate::home::{Fit, Presence};
use crate::message::{Failure, Update};

const NOW_MS: u64 = 1_700_000_000_000;

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

fn album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: None,
    }
}

fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: Some("spotify:album:1".to_owned()),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// A library Home was opened on, with every feed behind the deck answered.
fn settled(
    playlists: Vec<PlaylistEntry>,
    artists: Vec<FollowedArtist>,
    albums: Vec<SavedAlbum>,
    tracks: Vec<LikedTrack>,
) -> Library {
    let mut library = Library::default();
    library.go_home(NOW_MS);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(playlists));
    library.apply(&Update::FollowedArtists(artists));
    library.apply(&Update::SavedAlbums(albums));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(tracks));
    library
}

/// A station answer with no rows at all, for the seed `ask` named.
fn empty_station(seed: &StationSeed) -> Station {
    Station {
        seed: seed.clone(),
        uri: format!("spotify:station:{}", seed.uri),
        title: seed.uri.clone(),
        subtitle: None,
        art: None,
        rows: Vec::new(),
    }
}

/// Answers the one ask `library` made with nothing.
fn answer(library: &mut Library, ask: Command) {
    match ask {
        Command::LoadHome(HomeAsk::Station { seed }) => {
            library.apply(&Update::Home(HomeAnswer::Station(empty_station(&seed))))
        }
        Command::LoadHome(HomeAsk::Labels { tracks }) => {
            library.apply(&Update::Home(HomeAnswer::Labelled {
                asked: tracks,
                named: Vec::new(),
            }))
        }
        other => panic!("the draw shelf asked for {other:?}"),
    }
}

#[test]
fn nothing_is_asked_before_the_feeds_settle() {
    let mut library = Library::default();
    library.go_home(NOW_MS);

    assert_eq!(library.chance_ask(), None);
    assert_eq!(library.chance_section().presence(), Presence::Absent);

    // The rootlist alone is not enough: the deck is filled from four lists.
    library.apply(&Update::Playlists(vec![playlist("spotify:playlist:1")]));
    assert_eq!(library.chance_ask(), None);
}

#[test]
fn one_ask_per_draw_however_many_frames_elapse() {
    let mut library = settled(
        vec![playlist("spotify:playlist:1")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    assert!(
        matches!(
            library.chance_ask(),
            Some(Command::LoadHome(HomeAsk::Station { .. }))
        ),
        "the first draw asks for its station"
    );
    assert_eq!(
        library.chance_ask(),
        None,
        "the ask is claimed, not owed again"
    );
    assert_eq!(library.chance_ask(), None);
    assert_eq!(library.chance_section().presence(), Presence::Ready);
}

#[test]
fn a_song_seed_asks_its_name_before_its_station() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![liked("spotify:track:1")],
    );

    match library.chance_ask() {
        Some(Command::LoadHome(HomeAsk::Labels { tracks })) => {
            assert_eq!(tracks, vec!["spotify:track:1".to_owned()])
        }
        other => panic!("a song seed asks for its name first: {other:?}"),
    }
    assert_eq!(
        library.chance_section().presence(),
        Presence::Waiting,
        "the shelf has no name for its head yet"
    );

    library.apply(&Update::Home(HomeAnswer::Labelled {
        asked: vec!["spotify:track:1".to_owned()],
        named: vec![track("spotify:track:1")],
    }));

    match library.chance_ask() {
        Some(Command::LoadHome(HomeAsk::Station { seed })) => {
            assert_eq!(seed.kind, SeedKind::Song);
            assert_eq!(seed.uri, "spotify:track:1");
        }
        other => panic!("the named song asks for its station next: {other:?}"),
    }
}

#[test]
fn a_press_touches_no_budget() {
    let mut library = settled(
        vec![playlist("spotify:playlist:1")],
        vec![artist("spotify:artist:1")],
        vec![album("spotify:album:1")],
        vec![liked("spotify:track:1")],
    );
    let before = library.home.budget;

    library.home.chance.press(NOW_MS);
    let _ = library.chance_ask();

    assert_eq!(
        library.home.budget, before,
        "the shelf holds no allowance and spends none"
    );
    assert_eq!(library.home.budget.total(), before.total());
}

#[test]
fn a_press_sends_at_most_the_reads_one_seed_needs() {
    let mut library = settled(
        vec![playlist("spotify:playlist:1")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let first = library.chance_ask().expect("the first draw asks");
    answer(&mut library, first);
    assert!(!library.home.chance.pending(), "arrange: the draw landed");
    assert!(library.press_draw(NOW_MS), "arrange: the press walks");

    let mut asks = 0;
    while let Some(ask) = library.chance_ask() {
        asks += 1;
        answer(&mut library, ask);
        assert!(asks <= READS_PER_PRESS, "a press sent {asks} reads");
    }
    assert!(asks <= READS_PER_PRESS);
}

#[test]
fn an_unnameable_song_ends_the_draw_within_its_skips() {
    let mut library = settled(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![
            liked("spotify:track:1"),
            liked("spotify:track:2"),
            liked("spotify:track:3"),
        ],
    );

    let mut asks = 0;
    while let Some(ask) = library.chance_ask() {
        asks += 1;
        assert!(asks <= SKIPS + READS_PER_PRESS, "the walk never ended");
        answer(&mut library, ask);
    }

    assert!(library.home.chance.nothing(), "nothing was drawn");
    assert_eq!(library.chance_section().presence(), Presence::Absent);
}

#[test]
fn a_shelf_with_nothing_to_draw_is_absent() {
    let mut library = settled(Vec::new(), Vec::new(), Vec::new(), Vec::new());

    assert_eq!(library.chance_ask(), None, "an empty deck owes nothing");
    assert!(library.home.chance.filled(), "arrange: the deck was built");
    assert_eq!(library.chance_section().presence(), Presence::Absent);
}

#[test]
fn a_landed_draw_is_ready() {
    let mut library = settled(
        vec![playlist("spotify:playlist:1")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let ask = library.chance_ask().expect("the first draw asks");

    answer(&mut library, ask);

    assert_eq!(library.chance_section().presence(), Presence::Ready);
}

#[test]
fn a_failed_rootlist_leaves_the_shelf_absent() {
    let mut library = settled(
        vec![playlist("spotify:playlist:1")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.chance_section().presence(), Presence::Absent);
    assert_eq!(library.chance_ask(), None);
}

#[test]
fn a_quiet_dated_shelf_keeps_its_sentence_beside_an_empty_deck() {
    // An unnamed item is a saved thing the dated shelf sees and the deck
    // cannot seed from, so this is the one library with a quiet page and no
    // draw at all.
    let mut unnamed = playlist("spotify:playlist:q");
    unnamed.named = false;
    let mut library = settled(vec![unnamed], Vec::new(), Vec::new(), Vec::new());
    assert_eq!(library.chance_ask(), None, "arrange: the deck is empty");

    let sections = library.home_sections(Fit { columns: 4 });

    assert_eq!(sections.chance.presence(), Presence::Absent);
    assert_eq!(sections.dated, crate::home::Shelf::Quiet);
    assert!(
        crate::home::dated_draws(&sections.dated, sections.others()),
        "the quiet sentence is not suppressed by a shelf with nothing to draw"
    );
}
