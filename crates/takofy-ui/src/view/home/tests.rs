use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::home::{HomeAnswer, HomeAsk, Station};
use takofy_model::{
    Episode, FollowedArtist, FollowedShow, PlaylistEntry, PublishedAt, SavedAlbum, SavedAt, Show,
    ShowDetail,
};

use super::*;
use crate::cover::Art;
use crate::format::{
    HOME_BECAUSE_TITLE, HOME_COUNTED_TITLE, HOME_EPISODES_TITLE, HOME_SHELF_TITLE,
    HOME_STATIONS_TITLE,
};
use crate::home::{Fit, Presence, Slot};
use crate::library::Library;
use crate::message::{Command, Failure, Update};
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

/// A window wide enough that every card shelf draws in one row.
const PROBE: Vec2 = Vec2::new(900.0, 900.0);
const NOW_MS: u64 = 1_700_000_000_000;
const LOCAL_HOUR: u8 = 9;
const DAY_MS: u64 = 86_400_000;

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
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
fn painted(output: &egui::FullOutput) -> Vec<(&str, Rect)> {
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
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
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
/// The stations shelf is asked first, so its reads are answered empty: this
/// fixture draws no band, and the station asks do not stand in for the one this
/// returns.
fn related_ask(library: &mut Library) -> String {
    loop {
        match library.home_request() {
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
    library
}

/// A library whose four built shelves all have something to draw.
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
