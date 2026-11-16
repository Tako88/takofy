use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::home::{
    ArtistReleases, Day, DigAnswer, DigOutcome, DiscoveryAnswer, DiscoveryArtist, DiscoveryCard,
    DiscoveryKind, DiscoveryOutcome, DiscoverySeed, HomeAnswer, HomeAsk, NewRelease, OutsideId,
    ReleaseDate, Station,
};
use takofy_model::{
    Episode, FollowedArtist, FollowedShow, PlaylistEntry, PublishedAt, SavedAlbum, SavedAt, Show,
    ShowDetail,
};

use super::*;
use crate::cover::Art;
use crate::format::{
    HOME_BECAUSE_TITLE, HOME_CHANCE_TITLE, HOME_COUNTED_TITLE, HOME_DISCOVERY_MORE,
    HOME_DISCOVERY_NOTE_UNREACHABLE, HOME_DISCOVERY_SWITCH_OFF, HOME_DISCOVERY_SWITCH_ON,
    HOME_DISCOVERY_TITLE, HOME_EPISODES_TITLE, HOME_QUIET, HOME_RELEASES_TITLE, HOME_SHELF_TITLE,
    HOME_STATIONS_TITLE,
};
use crate::home::{Fit, Presence, Slot};
use crate::library::Library;
use crate::message::{Command, Failure, Update};
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

/// A window wide enough that every card shelf draws in one row.
pub(super) const PROBE: Vec2 = Vec2::new(900.0, 1200.0);
pub(super) const NOW_MS: u64 = 1_700_000_000_000;
const LOCAL_HOUR: u8 = 9;
const DAY_MS: u64 = 86_400_000;

pub(super) fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

/// One pass through the page, and the command it asked for.
fn run(
    ctx: &Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
) -> (Option<Command>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut command = None;
    let mut output = ctx.run_ui(input(events), |ui| {
        if let Some(next) = home_view(
            ui,
            crate::layout::Table::Full,
            library,
            art,
            Instant::now(),
            NOW_MS,
            LOCAL_HOUR,
            &tokens,
        ) {
            command = Some(next);
        }
    });
    output.textures_delta.clear();
    (command, output)
}

/// One click, in the passes egui needs to report it, and the command the page
/// asked for on the frame that reported it.
fn click(ctx: &Context, pos: Pos2, library: &mut Library, art: &mut Art) -> Option<Command> {
    let mut command = None;
    for event in click_events(pos) {
        let (next, _) = run(ctx, vec![event], library, art);
        command = command.or(next);
    }
    command
}

/// Every text one frame painted, with the box it filled, in paint order.
pub(super) fn painted(output: &egui::FullOutput) -> Vec<(&str, Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some((
                text.galley.text(),
                Rect::from_min_size(text.pos, text.galley.size()),
            )),
            _ => None,
        })
        .collect()
}

/// Where the galley whose text is exactly `label` was painted.
pub(super) fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

fn date(ms: u64) -> Option<SavedAt> {
    SavedAt::from_millis(i64::try_from(ms).ok()?)
}

fn playlist(uri: &str, name: &str, ms: u64) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: date(ms),
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn album(uri: &str, artist: &str, ms: u64) -> SavedAlbum {
    SavedAlbum {
        saved_at: date(ms),
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: artist.to_owned(),
        artist_uris: vec![artist.to_owned()],
        artist_names: vec![artist.to_owned()],
        year: None,
        cover: None,
        tracks: None,
    }
}

fn artist(uri: &str, name: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        portrait: None,
        releases: None,
        followed_at: None,
    }
}

fn show(uri: &str, name: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        publisher: None,
        episodes: None,
        cover: None,
    }
}

fn detail(uri: &str) -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            publisher: String::new(),
            description: String::new(),
            cover: None,
        },
        episodes: (0..6)
            .map(|at| Episode {
                uri: format!("{uri}:episode:{at}"),
                title: format!("Episode {at}"),
                show: uri.to_owned(),
                show_uri: Some(uri.to_owned()),
                description: None,
                published: PublishedAt::from_millis((NOW_MS - at as u64 * DAY_MS) as i64),
                duration_ms: Some(1_800_000),
                cover: None,
                named: true,
            })
            .collect(),
    }
}

/// The one related read the shelf owes, as the artist it is for.
///
/// The draw shelf is asked first, and its asks have the same shape as the
/// band's, so they are recognised by the shelf's own state and answered with
/// nothing. The stations shelf's reads are answered empty too: this fixture
/// draws no band, and the station asks do not stand in for the one this
/// returns.
fn related_ask(library: &mut Library) -> String {
    loop {
        let owed = library.home.chance.owed().is_some();
        let out = library.home.chance.in_flight().is_some();
        let request = library.home_request();
        // The draw shelf leads, so the ask is its own unless a read it sent was
        // already out when the call was made (#96).
        let drawn = owed || (!out && library.home.chance.in_flight().is_some());
        match request {
            Some(Command::LoadHome(ask)) if drawn => answer_draw(library, &ask),
            Some(Command::LoadHome(HomeAsk::Related { artist })) => return artist,
            Some(Command::LoadHome(HomeAsk::Station { seed })) => {
                library.apply(&Update::Home(HomeAnswer::Station(Station {
                    seed: seed.clone(),
                    uri: format!("spotify:station:{}", seed.uri),
                    title: seed.uri.clone(),
                    subtitle: None,
                    art: None,
                    rows: Vec::new(),
                })));
            }
            other => panic!("expected one related read, got {other:?}"),
        }
    }
}

/// Answers one ask the draw shelf made with nothing, so the shelf moves on.
fn answer_draw(library: &mut Library, ask: &HomeAsk) {
    match ask {
        HomeAsk::Station { seed } => {
            library.apply(&Update::Home(HomeAnswer::Station(Station {
                seed: seed.clone(),
                uri: format!("spotify:station:{}", seed.uri),
                title: seed.uri.clone(),
                subtitle: None,
                art: None,
                rows: Vec::new(),
            })));
        }
        HomeAsk::Labels { tracks } => {
            library.apply(&Update::Home(HomeAnswer::Labelled {
                asked: tracks.clone(),
                named: Vec::new(),
            }));
        }
        other => panic!("the draw shelf asked for {other:?}"),
    }
}

/// The shows the one pooled episodes read names.
fn show_ask(library: &mut Library) -> Vec<String> {
    match library.home_request() {
        Some(Command::LoadHome(HomeAsk::Episodes { shows })) => shows,
        other => panic!("expected one episodes read, got {other:?}"),
    }
}

/// What `slot` is doing, as the page reads it.
fn presence(library: &Library, slot: Slot) -> Presence {
    library.home_sections(Fit { columns: 4 }).presence(slot)
}

/// A library whose three Home sources have answered with these lists.
fn settled(playlists: Vec<PlaylistEntry>, albums: Vec<SavedAlbum>) -> Library {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(playlists));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(Vec::new()));
    library.apply(&Update::SavedAlbums(albums));
    // These tests are about the shelves under the discovery band; a band left
    // waiting for a day would hold a skeleton over every one of them.
    library.set_discovery_enabled(false);
    library
}

/// How long the page asked the host to wait before the next frame.
fn repaint_delay(output: &egui::FullOutput) -> std::time::Duration {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay)
}

#[test]
fn pressing_the_discovery_switch_turns_the_setting_and_asks_for_a_frame() {
    let ctx = context();
    let mut library = settled(Vec::new(), Vec::new());
    let mut art = Art::default();
    assert!(!library.discovery_enabled(), "arrange: the page starts off");

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let at = text_rect(&output, HOME_DISCOVERY_SWITCH_OFF)
        .expect("the off row draws over the page")
        .center();
    let mut pressed = None;
    for event in click_events(at) {
        let (command, output) = run(&ctx, vec![event], &mut library, &mut art);
        assert_eq!(command, None, "the switch is the page's own setting");
        pressed = Some(output);
    }
    let pressed = pressed.expect("the press drew a frame");
    assert!(library.discovery_enabled(), "the press turned discovery on");
    assert_eq!(
        repaint_delay(&pressed),
        std::time::Duration::ZERO,
        "the press asks for the frame that redraws the band"
    );

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let at = text_rect(&output, HOME_DISCOVERY_SWITCH_ON)
        .expect("the band at work draws its switch")
        .center();
    for event in click_events(at) {
        let (command, _) = run(&ctx, vec![event], &mut library, &mut art);
        assert_eq!(command, None);
    }
    assert!(
        !library.discovery_enabled(),
        "the same row turned discovery back off"
    );
}

#[test]
fn the_draw_shelf_sits_between_the_band_and_the_releases() {
    // The chance shelf is still naming its first seed, which is the one state
    // that draws the shelf's own title: once a draw is nameable the seed is the
    // title instead.
    let mut library = draw_naming();
    assert_eq!(
        library.home_sections(Fit { columns: 4 }).chance.presence(),
        Presence::Waiting,
        "arrange: the draw shelf waits for its read"
    );

    let ctx = context();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);

    let positions = head_positions(
        &output,
        &[HOME_DISCOVERY_TITLE, HOME_CHANCE_TITLE, HOME_RELEASES_TITLE],
    );
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the draw shelf left its place on the page: {positions:?}"
    );
}

/// A library whose draw shelf is still naming its first seed.
///
/// The kind order a page's seed draws puts the song bag before the playlists',
/// and no album or named artist is held here, so the first candidate is a song
/// whose label read has not landed yet.
fn draw_naming() -> Library {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(
        (0..4)
            .map(|at| {
                playlist(
                    &format!("spotify:playlist:{at}"),
                    &format!("Mix {at}"),
                    5_000,
                )
            })
            .collect(),
    ));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(vec![takofy_model::LikedTrack {
        uri: "spotify:track:liked".to_owned(),
        saved_at: None,
    }]));
    library.apply(&Update::SavedAlbums(Vec::new()));
    // No list named this artist, so the release shelf follows it and the deck
    // cannot seed from it.
    library.apply(&Update::FollowedArtists(vec![FollowedArtist {
        uri: "spotify:artist:seed".to_owned(),
        name: String::new(),
        named: false,
        portrait: None,
        releases: None,
        followed_at: None,
    }]));
    library.set_discovery_enabled(true);
    library.set_local_day(Day::from_days(SHELF_DAY));
    let seed = discovery_seed(&mut library);
    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed,
        outcome: DiscoveryOutcome::Neighbours {
            cards: (0..4).map(outside_card).collect(),
            genre: None,
        },
    })));
    land_release(&mut library);
    library
}

/// A library whose built shelves all have something to draw.
fn all_ready() -> Library {
    let mut library = settled(
        vec![playlist("spotify:playlist:a", "Mix", 5_000)],
        vec![album("spotify:album:a", "spotify:artist:held", 5_000)],
    );

    library.apply(&Update::FollowedArtists(vec![artist(
        "spotify:artist:seed",
        "Seed",
    )]));
    let seed = related_ask(&mut library);
    library.apply(&Update::Home(HomeAnswer::Related {
        artist: seed,
        cards: (0..4)
            .map(|at| artist(&format!("spotify:artist:{at}"), &format!("A{at}")))
            .collect(),
    }));

    library.apply(&Update::FollowedShows(vec![show(
        "spotify:show:a",
        "Show A",
    )]));
    let shows = show_ask(&mut library);
    library.apply(&Update::Home(HomeAnswer::Episodes(
        shows.iter().map(|uri| detail(uri)).collect(),
    )));
    library
}

/// The head each shelf draws, in the page's own order, for the shelves that
/// draw one at all.
fn heads(library: &Library) -> Vec<String> {
    let sections = library.home_sections(crate::home::Fit { columns: 4 });
    Slot::ORDER
        .into_iter()
        .filter(|slot| sections.presence(*slot) != Presence::Absent)
        .map(|slot| match slot {
            Slot::Dated => HOME_SHELF_TITLE.to_owned(),
            Slot::Stations => HOME_STATIONS_TITLE.to_owned(),
            // The related shelf names the artist it draws from.
            Slot::Because => format!("{HOME_BECAUSE_TITLE} Seed"),
            Slot::Counted => HOME_COUNTED_TITLE.to_owned(),
            Slot::Episodes => HOME_EPISODES_TITLE.to_owned(),
        })
        .collect()
}

#[test]
fn the_page_draws_its_heads_in_the_slots_own_order() {
    let mut library = all_ready();
    let expected = heads(&library);
    assert_eq!(
        expected,
        vec![
            HOME_SHELF_TITLE.to_owned(),
            format!("{HOME_BECAUSE_TITLE} Seed"),
            HOME_COUNTED_TITLE.to_owned(),
            HOME_EPISODES_TITLE.to_owned(),
        ],
        "arrange: four shelves draw a head"
    );

    let ctx = context();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let drawn = painted(&output);
    let positions: Vec<usize> = expected
        .iter()
        .map(|head| {
            drawn
                .iter()
                .position(|(text, _)| text == head)
                .unwrap_or_else(|| panic!("{head:?} was not drawn"))
        })
        .collect();

    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the heads left the page's order: {positions:?}"
    );
}

/// The day the release shelf's window is latched on, and the day its one
/// release is dated.
const SHELF_DAY: i32 = 20_253;

/// One release the shelf can draw, for the artist `all_ready` follows.
fn new_release() -> NewRelease {
    NewRelease {
        artist: "Seed".to_owned(),
        uri: "spotify:album:new".to_owned(),
        name: "New One".to_owned(),
        date: ReleaseDate {
            year: 2025,
            month: Some(10),
            day: Some(14),
        },
        day: Day::from_days(SHELF_DAY),
        cover: None,
    }
}

/// Lands one release for the followed artist, once `library` has a day.
fn land_release(library: &mut Library) {
    library.set_local_day(Day::from_days(SHELF_DAY));
    library.apply(&Update::Home(HomeAnswer::Releases {
        asked: vec!["spotify:artist:seed".to_owned()],
        found: vec![ArtistReleases {
            artist: "spotify:artist:seed".to_owned(),
            releases: vec![new_release()],
        }],
    }));
}

/// Where each of `heads` was painted, in paint order.
fn head_positions(output: &egui::FullOutput, heads: &[&str]) -> Vec<usize> {
    let drawn = painted(output);
    heads
        .iter()
        .map(|head| {
            drawn
                .iter()
                .position(|(text, _)| text == head)
                .unwrap_or_else(|| panic!("{head:?} was not drawn"))
        })
        .collect()
}

#[test]
fn a_ready_release_shelf_sits_above_every_other_shelf() {
    let mut library = all_ready();
    land_release(&mut library);

    let ctx = context();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);

    let positions = head_positions(
        &output,
        &[
            HOME_RELEASES_TITLE,
            HOME_SHELF_TITLE,
            HOME_COUNTED_TITLE,
            HOME_EPISODES_TITLE,
        ],
    );
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the release shelf left its place on the page: {positions:?}"
    );
    let drawn = painted(&output);
    assert!(
        drawn
            .iter()
            .any(|(text, _)| *text == crate::format::release_count_label(1).to_uppercase()),
        "the shelf drew no card: the aside counts what it drew"
    );
}

/// A library whose dated shelf is quiet: one saved thing, with no date.
fn quiet_with(artists: Vec<FollowedArtist>) -> Library {
    let mut library = settled(vec![playlist("spotify:playlist:a", "Mix", 0)], Vec::new());
    library.apply(&Update::FollowedArtists(artists));
    library
}

#[test]
fn a_ready_release_shelf_takes_the_quiet_lines_place() {
    let ctx = context();
    let mut art = Art::default();

    // With nothing followed the shelf is absent, so the dated shelf says its
    // quiet line itself.
    let mut quiet = quiet_with(Vec::new());
    quiet.set_local_day(Day::from_days(SHELF_DAY));
    let (_, output) = run(&ctx, Vec::new(), &mut quiet, &mut art);
    let texts: Vec<&str> = painted(&output).iter().map(|(text, _)| *text).collect();
    assert!(
        texts.contains(&HOME_QUIET),
        "arrange: a quiet dated shelf says so: {texts:?}"
    );

    let mut ready = quiet_with(vec![artist("spotify:artist:seed", "Seed")]);
    land_release(&mut ready);
    let (_, output) = run(&ctx, Vec::new(), &mut ready, &mut art);
    let texts: Vec<&str> = painted(&output).iter().map(|(text, _)| *text).collect();
    assert!(
        !texts.contains(&HOME_QUIET),
        "the quiet line was drawn under a shelf with content: {texts:?}"
    );
    assert!(
        texts.contains(&HOME_RELEASES_TITLE),
        "the shelf did not draw"
    );
}

#[test]
fn an_unreadable_rootlist_draws_only_the_dated_failure() {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::PlaylistFailed(Failure::Load));

    let ctx = context();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let drawn = painted(&output);
    let texts: Vec<&str> = drawn.iter().map(|(text, _)| *text).collect();

    assert!(
        texts.contains(&crate::signin::failure_message(Failure::Load)),
        "the page says why it could not read the library"
    );
    for head in [HOME_STATIONS_TITLE, HOME_COUNTED_TITLE, HOME_EPISODES_TITLE] {
        assert!(
            !texts.contains(&head),
            "a shelf repeated the rootlist's failure"
        );
    }
    assert!(
        !texts
            .iter()
            .any(|text| text.starts_with(HOME_BECAUSE_TITLE)),
        "a shelf repeated the rootlist's failure"
    );
    assert_eq!(
        texts.iter().filter(|text| **text == "Retry").count(),
        1,
        "one failure, not five"
    );
}

/// A library whose episodes read landed and failed for the one show it asked.
fn episodes_failed() -> Library {
    let mut library = settled(Vec::new(), Vec::new());
    library.apply(&Update::FollowedShows(vec![show(
        "spotify:show:a",
        "Show A",
    )]));
    let shows = show_ask(&mut library);
    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Episodes { shows },
        failure: Failure::Load,
    });
    library
}

#[test]
fn a_failed_band_draws_its_head_and_a_retry_that_routes_to_its_slot() {
    let mut library = episodes_failed();
    assert!(
        presence(&library, Slot::Episodes) == Presence::Failed,
        "arrange: the shelf failed"
    );

    let ctx = context();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let drawn = painted(&output);
    let texts: Vec<&str> = drawn.iter().map(|(text, _)| *text).collect();
    assert!(
        texts.contains(&HOME_EPISODES_TITLE),
        "a failed shelf still names itself"
    );

    let pos = text_rect(&output, "Retry")
        .expect("a failure the user can act on offers a retry")
        .center();
    assert_eq!(
        click(&ctx, pos, &mut library, &mut art),
        None,
        "the shelf's retry is not the whole page's reload"
    );
    assert!(
        library.home.episodes.asked().is_empty(),
        "the retry was routed to another shelf"
    );
    assert_eq!(
        library.home.budget.left(Slot::Episodes),
        crate::home::Budget::per_open(Slot::Episodes),
        "the retried shelf kept a spent allowance"
    );
    assert_eq!(
        presence(&library, Slot::Episodes),
        Presence::Waiting,
        "the retry did not put the shelf back to work"
    );
}

/// A library whose related shelf is waiting on the answer to its own read.
fn because_waiting() -> Library {
    let mut library = settled(
        Vec::new(),
        vec![album("spotify:album:a", "spotify:artist:held", 5_000)],
    );
    library.apply(&Update::FollowedArtists(vec![artist(
        "spotify:artist:seed",
        "Seed",
    )]));
    let _ = related_ask(&mut library);
    assert_eq!(
        presence(&library, Slot::Because),
        Presence::Waiting,
        "arrange: the shelf waits on its read"
    );
    library
}

/// The same library, with the related answer landed.
fn because_ready() -> Library {
    let mut library = because_waiting();
    library.apply(&Update::Home(HomeAnswer::Related {
        artist: "spotify:artist:seed".to_owned(),
        cards: (0..4)
            .map(|at| artist(&format!("spotify:artist:{at}"), &format!("A{at}")))
            .collect(),
    }));
    assert_eq!(
        presence(&library, Slot::Because),
        Presence::Ready,
        "arrange: the shelf has its cards"
    );
    library
}

/// The height the counted shelf's head is drawn at, on a page of its own.
fn counted_head_top(library: &mut Library) -> f32 {
    let ctx = context();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), library, &mut art);
    text_rect(&output, HOME_COUNTED_TITLE)
        .expect("the shelf below the waiting one is drawn")
        .top()
}

#[test]
fn a_waiting_band_holds_its_height_so_the_shelves_below_it_do_not_move() {
    let waiting = counted_head_top(&mut because_waiting());
    let ready = counted_head_top(&mut because_ready());

    assert_eq!(
        waiting, ready,
        "the shelf below moved when the waiting band's content landed"
    );
}

#[test]
fn a_ready_shelf_ends_in_a_door_to_its_rest() {
    let ctx = context();
    let mut library = because_ready();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let painted: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();

    // The shelves this library has content for end in their own doors; the rest
    // have nothing to draw, so no foot either.
    assert!(
        painted.contains(&crate::format::HOME_DOOR_SAVED),
        "the dated shelf's foot is missing"
    );
    let because = crate::format::home_door_because("Seed");
    assert!(
        painted.contains(&because.as_str()),
        "the because shelf's foot is missing"
    );
}

#[test]
fn a_shelfs_door_takes_the_page_it_names() {
    let ctx = context();
    let mut library = because_ready();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let label = crate::format::home_door_because("Seed");
    let at = text_rect(&output, &label)
        .expect("the because shelf has a foot")
        .center();

    let _ = click(&ctx, at, &mut library, &mut art);

    assert_eq!(
        library.page(),
        Some(&Page::Artist("spotify:artist:seed".to_owned())),
        "the door opens the artist whose neighbours it drew"
    );
}

#[test]
fn a_library_door_lands_on_the_root() {
    let ctx = context();
    let mut library = because_ready();
    let mut art = Art::default();
    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let at = text_rect(&output, crate::format::HOME_DOOR_SAVED)
        .expect("the dated shelf has a foot")
        .center();

    let _ = click(&ctx, at, &mut library, &mut art);

    assert_eq!(
        library.page(),
        None,
        "a door into the library lands on its root rather than on a page"
    );
}

/// One pass of the page at a window of `size`, and what it painted.
fn run_at(size: Vec2, library: &mut Library) -> egui::FullOutput {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = home_view(
                ui,
                crate::layout::regions_for(size.x).table,
                library,
                &mut art,
                Instant::now(),
                NOW_MS,
                LOCAL_HOUR,
                &tokens,
            );
        },
    );
    output.textures_delta.clear();
    output
}

#[test]
fn a_card_shelf_draws_four_across_on_one_row_however_wide_the_window() {
    let mut library = because_ready();
    let output = run_at(Vec2::new(2544.0, 4000.0), &mut library);

    let cards: Vec<Rect> = (0..4)
        .map(|at| {
            text_rect(&output, &format!("A{at}"))
                .unwrap_or_else(|| panic!("card A{at} is drawn at a wide window"))
        })
        .collect();
    let row = cards[0].top();
    assert!(
        cards.iter().all(|card| (card.top() - row).abs() < 0.01),
        "the four cards share one row: {cards:?}"
    );
    assert!(
        cards.windows(2).all(|pair| pair[0].left() < pair[1].left()),
        "the cards run left to right"
    );
    assert!(
        cards[3].left() > 1500.0,
        "the fourth card reaches the far end of a 2544-point page rather than \
         stopping at a 176-point minimum: {cards:?}"
    );
}

#[test]
fn a_narrow_page_wraps_the_four_cards_instead_of_squeezing_them() {
    let mut library = because_ready();
    let output = run_at(Vec2::new(360.0, 4000.0), &mut library);

    let cards: Vec<Rect> = (0..4)
        .map(|at| {
            text_rect(&output, &format!("A{at}"))
                .unwrap_or_else(|| panic!("card A{at} is still drawn"))
        })
        .collect();
    let rows: usize = cards
        .iter()
        .map(|card| (card.top() * 10.0).round() as i64)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert_eq!(
        rows, 4,
        "a page this narrow holds one card across, so the four stack: {cards:?}"
    );
}

/// The card a discovery lookup would find.
fn discovery_card(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: format!("spotify:artist:outside{index}"),
            name: format!("Outside {index}"),
        },
        outside: None,
        portrait: None,
        tracks: vec![takofy_model::TrackEntry::unnamed(&format!(
            "spotify:track:outside{index}"
        ))],
        latest_year: None,
    }
}

/// A Home library whose discovery row has landed four fresh cards.
fn discovery_ready() -> Library {
    let mut library = settled(
        (0..4)
            .map(|at| {
                playlist(
                    &format!("spotify:playlist:{at}"),
                    &format!("Mix {at}"),
                    5_000,
                )
            })
            .collect(),
        vec![album("spotify:album:a", "spotify:artist:held", 5_000)],
    );
    library.set_discovery_enabled(true);
    library.apply(&Update::FollowedArtists(vec![artist(
        "spotify:artist:seed",
        "Seed",
    )]));
    library.set_local_day(Day::from_days(20_253));
    let seed = discovery_seed(&mut library);
    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed,
        outcome: DiscoveryOutcome::Neighbours {
            cards: (0..4).map(outside_card).collect(),
            genre: Some("Alternative".to_owned()),
        },
    })));
    library
}

/// The discovery seed the frame asks for.
fn discovery_seed(library: &mut Library) -> DiscoverySeed {
    library
        .home_asks(Instant::now())
        .into_iter()
        .find_map(|command| match command {
            Command::LoadHome(HomeAsk::Discovery { seed }) => Some(seed),
            _ => None,
        })
        .expect("a ready library asks for a discovery seed")
}

#[test]
fn the_discovery_band_sits_above_the_dated_shelf() {
    let mut library = discovery_ready();
    let output = run_at(PROBE, &mut library);

    let band = text_rect(&output, HOME_DISCOVERY_TITLE).expect("the band draws its title");
    let shelf = text_rect(&output, HOME_SHELF_TITLE).expect("the dated shelf draws its title");
    assert!(
        band.top() < shelf.top(),
        "the band is the page's first band: {band:?} and {shelf:?}"
    );
}

#[test]
fn an_absent_band_leaves_no_head_and_no_divider_over_a_day_one_page() {
    let mut absent = settled(Vec::new(), Vec::new());
    absent.apply(&Update::FollowedArtists(Vec::new()));
    absent.set_discovery_enabled(true);
    absent.set_local_day(Day::from_days(20_253));
    assert_eq!(
        absent.discovery_band(),
        crate::home::discovery::Band::Absent
    );
    let absent_output = run_at(PROBE, &mut absent);

    assert!(
        text_rect(&absent_output, HOME_DISCOVERY_TITLE).is_none(),
        "an absent band draws no head"
    );
    assert!(
        text_rect(&absent_output, crate::format::HOME_DAY_ONE_TITLE).is_some(),
        "the day-one page still draws"
    );

    let mut off = settled(Vec::new(), Vec::new());
    off.apply(&Update::FollowedArtists(Vec::new()));
    off.set_local_day(Day::from_days(20_253));
    off.set_discovery_enabled(false);
    let off_output = run_at(PROBE, &mut off);
    let off_row = text_rect(&off_output, HOME_DISCOVERY_SWITCH_OFF)
        .expect("off draws its own row above the page");
    let off_day_one = text_rect(&off_output, crate::format::HOME_DAY_ONE_TITLE)
        .expect("the day-one page still draws under the off row");

    assert!(
        off_row.top() < off_day_one.top(),
        "the off row sits above the day-one page: {off_row:?} and {off_day_one:?}"
    );
}

#[test]
fn a_day_one_page_with_discovery_on_draws_no_band_and_no_rule_above_it() {
    let mut library = settled(Vec::new(), Vec::new());
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.set_discovery_enabled(true);
    library.set_local_day(Day::from_days(20_253));
    assert_eq!(
        library.discovery_band(),
        crate::home::discovery::Band::Absent
    );
    let output = run_at(PROBE, &mut library);

    assert!(
        text_rect(&output, HOME_DISCOVERY_TITLE).is_none(),
        "an absent band draws no head"
    );
    let greeting = text_rect(&output, crate::home::greeting(LOCAL_HOUR))
        .expect("the greeting is always drawn");
    let day_one = text_rect(&output, crate::format::HOME_DAY_ONE_TITLE)
        .expect("the day-one page draws its heading");
    assert!(
        text_rect(&output, crate::format::HOME_DAY_ONE_BODY).is_some(),
        "the day-one page keeps its one sentence"
    );
    let rules = rule_count(&output, greeting.bottom(), day_one.top());
    assert_eq!(
        rules, 1,
        "only the page head's own rule sits above the day-one page"
    );
}

/// Hairlines in the page's border colour between two heights.
fn rule_count(output: &egui::FullOutput, top: f32, bottom: f32) -> usize {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) => {
                shape.fill == tokens.palette.border
                    && (shape.rect.height() - crate::theme::stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.top() >= top
                    && shape.rect.bottom() <= bottom
            }
            _ => false,
        })
        .count()
}

#[test]
fn a_reroll_and_a_play_in_one_frame_both_land_with_the_play_first() {
    let mut library = settled(
        (0..4)
            .map(|at| {
                playlist(
                    &format!("spotify:playlist:{at}"),
                    &format!("Mix {at}"),
                    5_000,
                )
            })
            .collect(),
        Vec::new(),
    );
    library.set_discovery_enabled(true);
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.set_local_day(Day::from_days(20_253));
    let seed = discovery_seed(&mut library);
    assert_eq!(seed.kind, DiscoveryKind::Playlist);
    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed: seed.clone(),
        outcome: DiscoveryOutcome::Neighbours {
            cards: (0..4).map(discovery_card).collect(),
            genre: Some("Alternative".to_owned()),
        },
    })));
    library.stamp_discovery(NOW_MS - crate::home::discovery::REROLL_COOLDOWN_MS);
    assert_eq!(
        library.reroll_state_discovery(seed.kind, NOW_MS),
        crate::home::discovery::Reroll::Ready,
        "arrange: the row has a seed left and an ask to spend"
    );

    let rows = vec![takofy_model::TrackEntry::unnamed("spotify:track:play")];
    // The page applies the play before the re-roll, because a re-roll turns the
    // row idle and must not eat a play made in the same frame.
    assert!(library.want_play_rows(None, rows.clone()));
    assert!(library.reroll_discovery(seed.kind, NOW_MS));
    assert_eq!(
        library.take_play().map(|start| start.tracks),
        Some(rows),
        "the play was not lost to the re-roll"
    );
}

/// One discovery card, with the outside id a lookup would have stamped it with.
fn outside_card(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        outside: Some(OutsideId::from_id(index as i64)),
        ..discovery_card(index)
    }
}

/// The dig the page owes once a row has been pressed, one per pressed row.
fn dig_asks(library: &mut Library) -> Vec<takofy_model::home::DigAsk> {
    library
        .home_asks(Instant::now())
        .into_iter()
        .filter_map(|command| match command {
            Command::LoadHome(HomeAsk::Dig(ask)) => Some(ask),
            _ => None,
        })
        .collect()
}

/// The one dig the page owes once a row has been pressed.
fn dig_ask(library: &mut Library) -> takofy_model::home::DigAsk {
    let mut asks = dig_asks(library);
    assert_eq!(asks.len(), 1, "one press is one ring: {asks:?}");
    asks.pop().expect("a pressed row asks for its ring")
}

/// One click on the words the page drew, in the passes egui needs to report it.
fn press_words(ctx: &Context, words: &str, library: &mut Library, art: &mut Art) {
    let (_, output) = run(ctx, Vec::new(), library, art);
    let at = text_rect(&output, words)
        .unwrap_or_else(|| panic!("the page draws {words:?}"))
        .center();
    for event in click_events(at) {
        let _ = run(ctx, vec![event], library, art);
    }
}

#[test]
fn a_pressed_load_more_line_asks_its_own_row_for_one_ring() {
    let ctx = context();
    let mut library = discovery_ready();
    let mut art = Art::default();
    let kind = DiscoveryKind::Playlist;

    let _ = run(&ctx, Vec::new(), &mut library, &mut art);
    assert!(
        dig_asks(&mut library).is_empty(),
        "arrange: a row asks for nothing until its line is pressed"
    );

    press_words(&ctx, HOME_DISCOVERY_MORE, &mut library, &mut art);

    let asks = dig_asks(&mut library);
    assert_eq!(asks.len(), 1, "one press is one ring: {asks:?}");
    assert_eq!(
        asks[0].seed.kind, kind,
        "the ring walks from the row whose line was pressed"
    );
}

#[test]
fn a_line_already_fetching_takes_no_second_press() {
    let ctx = context();
    let mut library = discovery_ready();
    let mut art = Art::default();
    let kind = DiscoveryKind::Playlist;

    press_words(&ctx, HOME_DISCOVERY_MORE, &mut library, &mut art);
    assert_eq!(dig_asks(&mut library).len(), 1, "the first press asks once");
    assert_eq!(
        library.more_discovery(kind, NOW_MS),
        crate::home::discovery::walk::More::Fetching,
        "arrange: the row's ring is already out"
    );

    press_words(&ctx, HOME_DISCOVERY_MORE, &mut library, &mut art);
    assert!(
        dig_asks(&mut library).is_empty(),
        "a line whose ring is out eats the next press"
    );
}

#[test]
fn a_line_saying_what_the_last_press_found_takes_no_press() {
    let ctx = context();
    let mut library = discovery_ready();
    let mut art = Art::default();
    let kind = DiscoveryKind::Playlist;

    press_words(&ctx, HOME_DISCOVERY_MORE, &mut library, &mut art);
    let ask = dig_ask(&mut library);
    library.apply(&Update::Home(HomeAnswer::Dig(DigAnswer {
        seed: ask.seed,
        hop: ask.hop,
        outcome: DigOutcome::Unreachable,
    })));
    assert!(
        matches!(
            library.more_discovery(kind, NOW_MS),
            crate::home::discovery::walk::More::Note(
                crate::home::discovery::walk::Note::Unreachable,
                _
            )
        ),
        "arrange: the row is showing what its press found"
    );

    press_words(
        &ctx,
        HOME_DISCOVERY_NOTE_UNREACHABLE,
        &mut library,
        &mut art,
    );
    assert!(
        dig_asks(&mut library).is_empty(),
        "a sentence takes no press"
    );
}

#[test]
fn a_grown_row_keeps_its_first_four_in_order_and_a_reroll_replaces_them() {
    let ctx = context();
    let mut library = discovery_ready();
    let mut art = Art::default();
    let kind = DiscoveryKind::Playlist;

    assert!(library.press_more_discovery(kind, NOW_MS));
    let ask = dig_ask(&mut library);
    library.apply(&Update::Home(HomeAnswer::Dig(DigAnswer {
        seed: ask.seed,
        hop: ask.hop,
        outcome: DigOutcome::Ring {
            cards: (4..8).map(outside_card).collect(),
            passed: Vec::new(),
            exhausted: true,
        },
    })));

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let drawn: Vec<&str> = painted(&output)
        .into_iter()
        .map(|(text, _)| text)
        .filter(|text| text.starts_with("Outside "))
        .collect();
    assert_eq!(
        drawn,
        (0..8)
            .map(|index| format!("Outside {index}"))
            .collect::<Vec<_>>(),
        "the ring appends after the first four, in order"
    );

    library.stamp_discovery(NOW_MS - crate::home::discovery::REROLL_COOLDOWN_MS);
    assert!(library.reroll_discovery(kind, NOW_MS));
    let seed = library
        .home_asks(Instant::now())
        .into_iter()
        .find_map(|command| match command {
            Command::LoadHome(HomeAsk::Discovery { seed }) => Some(seed),
            _ => None,
        })
        .expect("the re-rolled row asks for its next seed");
    library.apply(&Update::Home(HomeAnswer::Discovery(DiscoveryAnswer {
        seed,
        outcome: DiscoveryOutcome::Neighbours {
            cards: (8..12).map(outside_card).collect(),
            genre: None,
        },
    })));

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let drawn: Vec<&str> = painted(&output)
        .into_iter()
        .map(|(text, _)| text)
        .filter(|text| text.starts_with("Outside "))
        .collect();
    assert_eq!(
        drawn,
        (8..12)
            .map(|index| format!("Outside {index}"))
            .collect::<Vec<_>>(),
        "a re-roll replaces the whole row, grown cards and all"
    );
}
