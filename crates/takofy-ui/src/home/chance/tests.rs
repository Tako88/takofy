use std::collections::HashSet;

use takofy_model::TrackEntry;
use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, Station, StationSeed};
use takofy_model::{FollowedArtist, LikedTrack, PlaylistEntry, SavedAlbum};

use crate::message::Failure;

use super::*;

fn date(days: u64) -> Option<SavedAt> {
    SavedAt::from_millis(1_000_000_000_000 - (days as i64) * 86_400_000)
}

fn seed() -> Seed {
    Seed::for_open(1_700_000_000_000, 1)
}

fn another() -> Seed {
    Seed::for_open(1_700_000_000_000, 2)
}

fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner: None,
        access: None,
        saved_at: date(1),
    }
}

fn followed(uri: &str, name: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        portrait: None,
        releases: None,
        followed_at: date(2),
    }
}

fn saved_album(uri: &str, name: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: date(3),
    }
}

fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: date(4),
    }
}

fn row(uri: &str, album: Option<&str>) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: "An artist".to_owned(),
        album: None,
        album_uri: album.map(str::to_owned),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// Rows that can all be cards: each names a different album.
fn clean_rows(count: usize) -> Vec<TrackEntry> {
    (0..count)
        .map(|index| {
            let album = format!("spotify:album:{index}");
            row(&format!("spotify:track:{index}"), Some(&album))
        })
        .collect()
}

fn seed_of(kind: SeedKind, uri: &str) -> StationSeed {
    StationSeed {
        kind,
        uri: uri.to_owned(),
    }
}

/// The library's four lists, each `count` long.
#[derive(Default)]
struct Library {
    playlists: Vec<PlaylistEntry>,
    artists: Vec<FollowedArtist>,
    albums: Vec<SavedAlbum>,
    songs: Vec<LikedTrack>,
}

impl Library {
    fn new(count: usize) -> Self {
        Self {
            playlists: (0..count)
                .map(|index| playlist(&format!("spotify:playlist:{index}"), "A list"))
                .collect(),
            artists: (0..count)
                .map(|index| followed(&format!("spotify:artist:{index}"), "An artist"))
                .collect(),
            albums: (0..count)
                .map(|index| saved_album(&format!("spotify:album:{index}"), "An album"))
                .collect(),
            songs: (0..count)
                .map(|index| liked(&format!("spotify:track:{index}")))
                .collect(),
        }
    }

    fn candidates(&self) -> Candidates<'_> {
        Candidates {
            playlists: &self.playlists,
            artists: &self.artists,
            albums: &self.albums,
            songs: &self.songs,
        }
    }
}

fn index(kind: SeedKind) -> usize {
    match kind {
        SeedKind::Playlist => 0,
        SeedKind::Artist => 1,
        SeedKind::Album => 2,
        SeedKind::Song => 3,
    }
}

fn drawn(deck: &mut Deck, presses: usize) -> Vec<Candidate> {
    (0..presses)
        .map(|_| deck.draw().expect("the library holds a candidate"))
        .collect()
}

#[test]
fn a_held_seed_gives_the_same_deck_twice() {
    let library = Library::new(5);
    let mut first = Deck::default();
    first.fill(&library.candidates(), seed());
    let mut second = Deck::default();
    second.fill(&library.candidates(), seed());

    assert_eq!(drawn(&mut first, 9), drawn(&mut second, 9));
}

#[test]
fn a_different_seed_draws_a_different_deck() {
    let library = Library::new(5);
    let mut first = Deck::default();
    first.fill(&library.candidates(), seed());
    let mut second = Deck::default();
    second.fill(&library.candidates(), another());

    assert_ne!(drawn(&mut first, 4), drawn(&mut second, 4));
}

#[test]
fn a_uri_listed_twice_is_one_candidate() {
    let playlists = vec![
        playlist("spotify:playlist:1", "One"),
        playlist("spotify:playlist:1", "One again"),
        playlist("spotify:playlist:2", "Two"),
    ];
    let candidates = Candidates {
        playlists: &playlists,
        ..Default::default()
    };
    let mut deck = Deck::default();
    deck.fill(&candidates, seed());

    let first = deck.draw().expect("a candidate").uri;
    let second = deck.draw().expect("a candidate").uri;
    assert_ne!(first, second, "a list with three entries drew one twice");
}

#[test]
fn a_playlist_with_no_name_of_its_own_is_never_a_candidate() {
    let mut unnamed = playlist("spotify:playlist:1", "Untitled");
    unnamed.named = false;
    let blank = playlist("spotify:playlist:2", "");
    let playlists = vec![unnamed, blank, playlist("spotify:playlist:3", "Three")];
    let candidates = Candidates {
        playlists: &playlists,
        ..Default::default()
    };
    let mut deck = Deck::default();
    deck.fill(&candidates, seed());

    for candidate in drawn(&mut deck, 3) {
        assert_eq!(candidate.uri, "spotify:playlist:3");
    }
}

#[test]
fn every_named_drawer_carries_the_date_its_list_gave_it() {
    let library = Library::new(2);
    let mut deck = Deck::default();
    deck.fill(&library.candidates(), seed());

    for candidate in drawn(&mut deck, 4) {
        let at = match candidate.kind {
            SeedKind::Playlist => library
                .playlists
                .iter()
                .find(|entry| entry.uri == candidate.uri)
                .and_then(|entry| entry.saved_at),
            SeedKind::Artist => library
                .artists
                .iter()
                .find(|entry| entry.uri == candidate.uri)
                .and_then(|entry| entry.followed_at),
            SeedKind::Album => library
                .albums
                .iter()
                .find(|entry| entry.uri == candidate.uri)
                .and_then(|entry| entry.saved_at),
            SeedKind::Song => library
                .songs
                .iter()
                .find(|entry| entry.uri == candidate.uri)
                .and_then(|entry| entry.saved_at),
        };
        assert_eq!(candidate.at, at);
        assert!(at.is_some(), "the drawer dropped its date");
    }
}

#[test]
fn four_presses_draw_the_four_kinds() {
    let library = Library::new(3);
    let mut deck = Deck::default();
    deck.fill(&library.candidates(), seed());

    let mut kinds: Vec<usize> = drawn(&mut deck, 4)
        .iter()
        .map(|candidate| index(candidate.kind))
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), 4, "a kind was drawn twice before the fourth");
}

#[test]
fn a_library_of_albums_and_likes_alternates_without_idling() {
    let library = Library::new(0);
    let library = Library {
        albums: (0..3)
            .map(|index| saved_album(&format!("spotify:album:{index}"), "An album"))
            .collect(),
        songs: (0..3)
            .map(|index| liked(&format!("spotify:track:{index}")))
            .collect(),
        ..library
    };
    let mut deck = Deck::default();
    deck.fill(&library.candidates(), seed());

    let drawn = drawn(&mut deck, 6);
    for pair in drawn.windows(2) {
        assert_ne!(pair[0].kind, pair[1].kind, "the same kind twice running");
    }
}

#[test]
fn a_bag_never_repeats_before_it_is_spent() {
    let library = Library::new(5);
    let mut deck = Deck::default();
    deck.fill(&library.candidates(), seed());

    let uris: HashSet<String> = drawn(&mut deck, 5).into_iter().map(|c| c.uri).collect();
    assert_eq!(
        uris.len(),
        5,
        "a bag repeated a candidate before it was spent"
    );
}

#[test]
fn a_spent_bag_is_reshuffled_and_may_repeat() {
    let albums: Vec<SavedAlbum> = (0..2)
        .map(|index| saved_album(&format!("spotify:album:{index}"), "An album"))
        .collect();
    let candidates = Candidates {
        albums: &albums,
        ..Default::default()
    };
    let mut deck = Deck::default();
    deck.fill(&candidates, seed());

    let uris: HashSet<String> = drawn(&mut deck, 6).into_iter().map(|c| c.uri).collect();
    assert_eq!(uris.len(), 2, "a bag of two gave six different candidates");
}

#[test]
fn an_empty_library_draws_nothing() {
    let mut deck = Deck::default();
    deck.fill(&Candidates::default(), seed());

    assert_eq!(deck.draw(), None);
}

#[test]
fn the_seed_is_never_among_its_cards() {
    let mut rows = clean_rows(CARDS + 1);
    let album = "spotify:album:seed".to_owned();
    rows.insert(3, row("spotify:track:seed", Some(&album)));
    let drew = seed_of(SeedKind::Song, "spotify:track:seed");

    let cards = draw_cards(&rows, &drew, seed());

    assert_eq!(cards.len(), CARDS);
    assert!(
        cards.iter().all(|card| rows[card.row].uri != drew.uri),
        "the shelf drew the seed it was built from"
    );
}

#[test]
fn a_track_and_its_own_album_never_sit_together() {
    let mut rows = clean_rows(CARDS);
    // One row repeats the first one's album, so the pair rule has something to refuse.
    rows.push(row("spotify:track:twin", Some("spotify:album:0")));

    let cards = draw_cards(&rows, &seed_of(SeedKind::Album, "x"), seed());

    assert_eq!(
        cards.len(),
        CARDS,
        "the pair rule cost a card it did not have to"
    );
    let albums: Vec<&str> = cards
        .iter()
        .filter_map(|card| rows[card.row].album_uri.as_deref())
        .collect();
    assert_eq!(
        albums.iter().collect::<HashSet<_>>().len(),
        CARDS,
        "two cards are one album"
    );
    let twins = cards
        .iter()
        .filter(|card| rows[card.row].album_uri.as_deref() == Some("spotify:album:0"))
        .count();
    assert!(twins <= 1, "two rows of one album were drawn as cards");
}

#[test]
fn no_uri_is_drawn_twice() {
    let mut rows = clean_rows(CARDS);
    rows.push(row("spotify:track:0", Some("spotify:album:twin")));

    let cards = draw_cards(&rows, &seed_of(SeedKind::Artist, "x"), seed());

    assert_eq!(cards.len(), CARDS);
    let uris: HashSet<&str> = cards
        .iter()
        .map(|card| card.row)
        .map(|row| rows[row].uri.as_str())
        .collect();
    assert_eq!(uris.len(), CARDS, "a URI was drawn twice");
}

#[test]
fn an_unnamed_row_or_one_with_no_album_is_never_a_card() {
    let mut unnamed = row("spotify:track:unnamed", Some("spotify:album:unnamed"));
    unnamed.named = false;
    let albumless = row("spotify:track:albumless", None);
    let mut rows = vec![unnamed, albumless];
    rows.extend(clean_rows(CARDS));

    let cards = draw_cards(&rows, &seed_of(SeedKind::Artist, "x"), seed());

    assert_eq!(cards.len(), CARDS);
    assert!(cards.iter().all(|card| card.row >= 2));
}

#[test]
fn a_station_with_enough_rows_fills_the_shelf() {
    let cards = draw_cards(
        &clean_rows(CARDS * 3),
        &seed_of(SeedKind::Artist, "x"),
        seed(),
    );

    assert_eq!(cards.len(), CARDS);
}

#[test]
fn the_rows_drawn_are_sampled_rather_than_taken_from_the_head() {
    // Two draws over one answer must not always open with the same rows: the pick is
    // a sample, or two seeds that share a neighbourhood draw the same shelf (#96).
    let rows = clean_rows(CARDS * 5);
    let station = seed_of(SeedKind::Artist, "x");
    let first: Vec<usize> = draw_cards(&rows, &station, seed())
        .iter()
        .map(|card| card.row)
        .collect();
    let mut elsewhere = false;
    for press in 1..8u64 {
        let mut state = seed();
        for _ in 0..press {
            state = state.split(press);
        }
        let rows_drawn: Vec<usize> = draw_cards(&rows, &station, state)
            .iter()
            .map(|card| card.row)
            .collect();
        if rows_drawn != first {
            elsewhere = true;
            break;
        }
        if let Some(&last) = rows_drawn.last()
            && last >= CARDS
        {
            elsewhere = true;
            break;
        }
    }
    assert!(elsewhere, "every draw took the same rows from the answer");
    assert!(
        first.iter().any(|&row| row >= CARDS) || first.len() == CARDS,
        "the first draw took nothing past the head"
    );
}

#[test]
fn a_refused_row_does_not_cost_the_shelf_a_card() {
    // One row of five shares an album with another, so four can be cards. The sample
    // must reach past the refusal rather than stopping short of it (#96).
    let rows = vec![
        row("spotify:track:0", Some("spotify:album:0")),
        row("spotify:track:1", Some("spotify:album:1")),
        row("spotify:track:2", Some("spotify:album:2")),
        row("spotify:track:3", Some("spotify:album:0")),
        row("spotify:track:4", Some("spotify:album:4")),
    ];

    for open in 0..8 {
        let cards = draw_cards(
            &rows,
            &seed_of(SeedKind::Album, "x"),
            Seed::for_open(1_700_000_000_000, open),
        );
        let albums: HashSet<&str> = cards
            .iter()
            .filter_map(|card| rows[card.row].album_uri.as_deref())
            .collect();
        assert_eq!(cards.len(), 4, "the refusal cost the shelf a card");
        assert_eq!(albums.len(), 4, "two cards are one album");
    }
}

#[test]
fn the_same_rows_and_seed_yield_the_same_four() {
    let rows = clean_rows(8);
    let drew = seed_of(SeedKind::Artist, "x");

    assert_eq!(
        draw_cards(&rows, &drew, seed()),
        draw_cards(&rows, &drew, seed())
    );
}

fn opened(library: &Library) -> State {
    let mut state = State::default();
    state.open(seed());
    state.fill(&library.candidates());
    state
}

/// The shelf's ask, claimed so a frame cannot send it twice.
fn claim(state: &mut State) -> Ask {
    let ask = state.owed().expect("the shelf owes a read");
    state.asked(ask.clone());
    ask
}

fn station_answer(seed: &StationSeed, rows: Vec<TrackEntry>) -> HomeAnswer {
    HomeAnswer::Station(Station {
        seed: seed.clone(),
        uri: "spotify:station:1".to_owned(),
        title: "Radio".to_owned(),
        subtitle: None,
        art: None,
        rows,
    })
}

fn labelled(uri: &str, name: &str, album: &str) -> HomeAnswer {
    let mut track = row(name, Some(album));
    track.uri = uri.to_owned();
    HomeAnswer::Labelled {
        asked: vec![uri.to_owned()],
        named: vec![track],
    }
}

/// Drives the shelf's reads to a landing, answering each with `rows`.
fn land_draw(state: &mut State, rows: Vec<TrackEntry>) {
    for _ in 0..READS_PER_PRESS {
        let Some(ask) = state.owed() else { return };
        state.asked(ask.clone());
        match ask {
            Ask::Name(uri) => state.land(&labelled(&uri, &uri, "spotify:album:seed")),
            Ask::Station(seed) => state.land(&station_answer(&seed, rows.clone())),
        }
    }
}

#[test]
fn a_press_while_a_read_is_out_is_dropped() {
    let mut state = opened(&Library::new(2));
    let ask = claim(&mut state);

    assert!(!state.press(1_000), "a press while a read is out moved");
    assert_eq!(state.in_flight(), Some(ask));
}

#[test]
fn a_late_station_answer_for_the_previous_seed_is_dropped() {
    let mut state = opened(&Library::new(2));
    land_draw(&mut state, clean_rows(CARDS + 1));
    let landed = state.draw().expect("a draw").seed.clone();

    assert!(state.press(1_000), "a press after a landing draws");
    let late = seed_of(landed.kind, &landed.uri);
    state.land(&station_answer(&late, clean_rows(6)));

    let drawn = state.draw().expect("the landed draw stays").seed.clone();
    assert_eq!(drawn, landed, "a late answer repainted the shelf");
    assert!(state.pending(), "the shelf stopped waiting on its own read");
}

#[test]
fn a_song_seed_asks_its_name_before_its_station() {
    let library = Library {
        songs: vec![liked("spotify:track:1"), liked("spotify:track:2")],
        ..Library::default()
    };
    let mut state = opened(&library);
    let Ask::Name(uri) = claim(&mut state) else {
        panic!("a song seed is asked for its name first");
    };

    state.land(&labelled(&uri, "A song", "spotify:album:1"));

    assert_eq!(
        state.owed(),
        Some(Ask::Station(seed_of(SeedKind::Song, &uri)))
    );
    claim(&mut state);
    state.land(&station_answer(
        &seed_of(SeedKind::Song, &uri),
        clean_rows(6),
    ));
    assert_eq!(
        state.draw().expect("the station landed").seed.name,
        "A song"
    );
}

#[test]
fn a_library_of_unnameable_songs_ends_in_nothing_within_skips() {
    let songs: Vec<LikedTrack> = (0..12)
        .map(|index| liked(&format!("spotify:track:{index}")))
        .collect();
    let candidates = Candidates {
        songs: &songs,
        ..Default::default()
    };
    let mut state = State::default();
    state.open(seed());
    state.fill(&candidates);

    let mut reads = 0;
    for _ in 0..=SKIPS {
        let Some(ask) = state.owed() else { break };
        reads += 1;
        state.asked(ask.clone());
        let Ask::Name(uri) = ask else {
            panic!("a song seed asks for its name first");
        };
        state.land(&HomeAnswer::Labelled {
            asked: vec![uri],
            named: Vec::new(),
        });
    }

    assert_eq!(reads, SKIPS, "the walk ran past its bound");
    assert!(state.nothing());
    assert_eq!(state.in_flight(), None);
    assert!(state.draw().is_none(), "a shelf that drew nothing drew");
}

#[test]
fn a_song_seeds_head_opens_the_album_its_name_named() {
    // A track has no page of its own, so the head's link needs the album the name
    // read carried — before the station answers as well as after (#96).
    let songs = vec![liked("spotify:track:9")];
    let candidates = Candidates {
        songs: &songs,
        ..Default::default()
    };
    let mut state = State::default();
    state.open(seed());
    state.fill(&candidates);

    let Ask::Name(uri) = claim(&mut state) else {
        panic!("a song seed asks for its name first");
    };
    let mut named = row(&uri, Some("spotify:album:9"));
    named.title = "A name".to_owned();
    state.land(&HomeAnswer::Labelled {
        asked: vec![uri],
        named: vec![named.clone()],
    });

    let Ask::Station(station) = claim(&mut state) else {
        panic!("a named song asks for its station next");
    };
    let unlading = state.draw().expect("a named seed paints before its answer");
    assert_eq!(unlading.album_uri.as_deref(), Some("spotify:album:9"));

    state.land(&station_answer(&station, vec![named]));
    let landed = state.draw().expect("a landed draw");
    assert_eq!(landed.album_uri.as_deref(), Some("spotify:album:9"));
}

#[test]
fn a_failed_name_read_on_the_first_draw_walks_on() {
    // A song's name is its head, so a name read that fails leaves nothing to paint;
    // the square is the shelf's only control, so the draw moves on instead of
    // stranding the deck on a seed with no head (#96).
    let songs: Vec<LikedTrack> = (0..4)
        .map(|index| liked(&format!("spotify:track:{index}")))
        .collect();
    let candidates = Candidates {
        songs: &songs,
        ..Default::default()
    };
    let mut state = State::default();
    state.open(seed());
    state.fill(&candidates);

    let Ask::Name(uri) = claim(&mut state) else {
        panic!("a song seed asks for its name first");
    };
    state.fail(
        &HomeAsk::Labels {
            tracks: vec![uri.clone()],
        },
        Failure::Load,
    );

    assert_ne!(state.owed(), None, "the failed name read stranded the deck");
    assert_eq!(state.failure(), Some(Failure::Load));
    assert!(state.draw().is_none(), "a nameless seed painted a head");
    assert!(!matches!(state.owed(), Some(Ask::Name(stuck)) if stuck == uri));
}

#[test]
fn a_press_is_a_retry_so_the_last_sentence_goes_with_it() {
    let library = Library {
        albums: vec![saved_album("spotify:album:1", "An album")],
        ..Library::default()
    };
    let mut state = opened(&library);
    let Ask::Station(seed) = claim(&mut state) else {
        panic!("an album seed is asked for its station");
    };
    state.fail(&HomeAsk::Station { seed }, Failure::Load);
    assert_eq!(state.failure(), Some(Failure::Load));

    assert!(state.press(2_000));
    assert_eq!(state.failure(), None, "the retry kept the old sentence");
}

#[test]
fn a_walk_that_runs_out_on_a_kept_draw_announces_nothing() {
    // Eight unnameable songs behind a draw that landed: the four stay, so the shelf
    // must not print "nothing beside this one" under them (#96).
    let songs: Vec<LikedTrack> = (0..12)
        .map(|index| liked(&format!("spotify:track:{index}")))
        .collect();
    let albums = vec![saved_album("spotify:album:1", "An album")];
    let candidates = Candidates {
        songs: &songs,
        albums: &albums,
        ..Default::default()
    };
    let mut state = State::default();
    state.open(seed());
    state.fill(&candidates);
    land_draw(&mut state, clean_rows(CARDS + 1));
    assert_eq!(state.draw().expect("a draw").cards.len(), CARDS);

    // Every press from here lands on the songs, whose names never come back.
    let mut presses = 0;
    while presses < 4 {
        presses += 1;
        if !state.press(2_000 + presses) {
            break;
        }
        while let Some(ask) = state.owed() {
            state.asked(ask.clone());
            if let Ask::Name(uri) = ask {
                state.land(&HomeAnswer::Labelled {
                    asked: vec![uri],
                    named: Vec::new(),
                });
            } else {
                break;
            }
        }
    }

    let drawn = state.draw().expect("the four stay");
    assert!(!drawn.cards.is_empty(), "the kept draw went missing");
    assert!(
        !drawn.nothing,
        "the shelf announced nothing under four cards"
    );
}

#[test]
fn a_song_seed_costs_two_reads() {
    let library = Library {
        songs: vec![liked("spotify:track:1")],
        ..Library::default()
    };
    let mut state = opened(&library);

    let mut reads = 0;
    while let Some(ask) = state.owed() {
        assert!(
            reads < READS_PER_PRESS,
            "one press sent more than its reads"
        );
        reads += 1;
        state.asked(ask.clone());
        match ask {
            Ask::Name(uri) => state.land(&labelled(&uri, "A song", "spotify:album:1")),
            Ask::Station(seed) => state.land(&station_answer(&seed, clean_rows(6))),
        }
    }

    assert_eq!(reads, READS_PER_PRESS);
}

#[test]
fn a_named_seed_costs_one_read() {
    let library = Library {
        albums: vec![saved_album("spotify:album:1", "An album")],
        ..Library::default()
    };
    let mut state = opened(&library);

    let mut reads = 0;
    while let Some(ask) = state.owed() {
        assert!(
            reads < READS_PER_PRESS,
            "one press sent more than its reads"
        );
        reads += 1;
        state.asked(ask.clone());
        let Ask::Station(seed) = ask else {
            panic!("a named seed is asked for its station alone");
        };
        state.land(&station_answer(&seed, clean_rows(6)));
    }

    assert_eq!(reads, 1);
    assert!(reads < READS_PER_PRESS);
}

#[test]
fn a_failed_station_read_keeps_the_last_four() {
    let library = Library {
        albums: vec![saved_album("spotify:album:1", "An album")],
        ..Library::default()
    };
    let mut state = opened(&library);
    land_draw(&mut state, clean_rows(CARDS + 1));
    assert_eq!(state.draw().expect("a draw").cards.len(), CARDS);

    assert!(state.press(1_000));
    let Ask::Station(seed) = claim(&mut state) else {
        panic!("an album seed is asked for its station");
    };
    state.fail(&HomeAsk::Station { seed }, Failure::Load);

    let drawn = state.draw().expect("the four stay");
    assert_eq!(drawn.cards.len(), CARDS, "a failure blanked the shelf");
    assert_eq!(drawn.failed, Some(Failure::Load));
    assert!(!drawn.pending);
}

#[test]
fn a_failed_first_draw_still_paints_its_head_and_its_sentence() {
    // The square is the shelf's only control, and a failure must leave it pressable
    // rather than taking the whole shelf off the page (#96).
    let library = Library {
        albums: vec![saved_album("spotify:album:1", "An album")],
        ..Library::default()
    };
    let mut state = opened(&library);

    let Ask::Station(seed) = claim(&mut state) else {
        panic!("an album seed is asked for its station");
    };
    state.fail(&HomeAsk::Station { seed }, Failure::Load);

    let drawn = state
        .draw()
        .expect("a failure before the first answer still paints");
    assert!(drawn.cards.is_empty(), "a failed first draw drew cards");
    assert_eq!(
        drawn.seed.name, "An album",
        "the head could not name its seed"
    );
    assert_eq!(drawn.failed, Some(Failure::Load));
    assert!(!drawn.pending);
}

#[test]
fn a_station_with_no_usable_row_draws_nothing() {
    let library = Library {
        albums: vec![saved_album("spotify:album:1", "An album")],
        ..Library::default()
    };
    let mut state = opened(&library);

    land_draw(&mut state, vec![row("spotify:track:albumless", None)]);

    let drawn = state.draw().expect("the seed stays with its station");
    assert!(drawn.cards.is_empty());
    assert!(drawn.nothing);
}

#[test]
fn a_press_keeps_the_four_while_it_draws() {
    let library = Library {
        albums: (0..3)
            .map(|index| saved_album(&format!("spotify:album:{index}"), "An album"))
            .collect(),
        ..Library::default()
    };
    let mut state = opened(&library);
    land_draw(&mut state, clean_rows(CARDS + 1));
    let landed: Vec<String> = state
        .draw()
        .expect("a draw")
        .cards
        .iter()
        .map(|card| card.row.uri.clone())
        .collect();

    assert!(state.press(1_000));

    let drawing: Vec<String> = state
        .draw()
        .expect("the four stay while it draws")
        .cards
        .iter()
        .map(|card| card.row.uri.clone())
        .collect();
    assert_eq!(drawing, landed, "a press blanked the shelf");
    assert!(state.draw().expect("a draw").pending);
}

#[test]
fn the_shelf_counts_what_it_drew() {
    let mut state = opened(&Library::new(2));
    land_draw(&mut state, clean_rows(CARDS + 1));

    let drawn = state.draw().expect("a draw");
    assert_eq!(drawn.cards.len(), CARDS);
    assert_eq!(drawn.tracks + drawn.albums, drawn.cards.len());
    assert_eq!(
        drawn
            .cards
            .iter()
            .filter(|card| card.kind == CardKind::Album)
            .count(),
        drawn.albums
    );
}

#[test]
fn forget_drops_the_deck_and_the_four() {
    let mut state = opened(&Library::new(2));
    land_draw(&mut state, clean_rows(CARDS + 1));
    assert!(state.filled());
    assert!(state.draw().is_some());

    state.forget();

    assert!(!state.filled(), "the deck survived a forget");
    assert!(state.draw().is_none(), "the four survived a forget");
    assert!(!state.pending());
    assert_eq!(state.failure(), None);
}

#[test]
fn an_open_starts_the_shelf_again() {
    let mut state = opened(&Library::new(2));
    land_draw(&mut state, clean_rows(CARDS + 1));

    state.open(another());

    assert!(!state.filled());
    assert!(state.draw().is_none());
}

#[test]
fn waited_runs_from_nought_to_one() {
    assert_eq!(waited(None, 1_000, 900), None);
    assert_eq!(waited(Some(1_000), 1_000, 900), Some(0.0));
    assert_eq!(waited(Some(1_000), 1_450, 900), Some(0.5));
    assert_eq!(waited(Some(1_000), 5_000, 900), Some(1.0));
}
