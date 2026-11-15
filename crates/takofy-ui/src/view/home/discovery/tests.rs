#![allow(clippy::useless_vec)]

use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::TrackEntry;
use takofy_model::home::{DiscoveryArtist, DiscoveryCard, DiscoveryKind, DiscoverySeed};

use super::*;
use crate::availability::Control;
use crate::cover::Art;
use crate::format::{
    HOME_DISCOVERY_SWITCH_OFF, HOME_DISCOVERY_SWITCH_ON, HOME_DISCOVERY_TITLE, discovery_source,
};
use crate::home::discovery::{Band, RowRead, RowView};
use crate::layout::Table;
use crate::library::Library;
use crate::theme::{Tokens, fonts, style};
use crate::view::controls;
use crate::view::playlist::root::MediaDraw;
use crate::view::test_support::click_events;

const PROBE: Vec2 = Vec2::new(1100.0, 1600.0);
const NOW_MS: u64 = 1_700_000_000_000;
const ARTIST: &str = "spotify:artist:card0";
const SEED: &str = "spotify:playlist:seed";

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

fn seed(kind: DiscoveryKind, uri: &str, name: &str) -> DiscoverySeed {
    DiscoverySeed {
        kind,
        uri: uri.to_owned(),
        name: name.to_owned(),
        anchor: None,
    }
}

fn track(index: usize) -> TrackEntry {
    TrackEntry {
        uri: format!("spotify:track:{index}"),
        title: format!("Track {index}"),
        artists: "Vela Moreno".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(200_000 + index as u32 * 1_000),
        named: true,
        cover: None,
    }
}

fn card(index: usize, tracks: usize) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: format!("spotify:artist:card{index}"),
            name: format!("Artist {index}"),
        },
        portrait: None,
        tracks: (0..tracks).map(|at| track(index * 10 + at)).collect(),
        latest_year: Some(2024),
    }
}

fn row<'a>(seed: &'a DiscoverySeed, cards: Vec<&'a DiscoveryCard>, read: RowRead) -> RowView<'a> {
    RowView {
        seed,
        genre: Some("Alternative"),
        cards,
        read,
    }
}

struct Frame {
    drew: bool,
    out: Pressed,
    open: Option<crate::route::Page>,
    output: egui::FullOutput,
}

fn run(
    ctx: &Context,
    band: &Band<'_>,
    library: &Library,
    table: Table,
    events: Vec<Event>,
) -> Frame {
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut drew = false;
    let mut output = ctx.run_ui(input(events), |ui| {
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        drew = show(ui, band, NOW_MS, &tokens, table, &mut draw, &mut out);
    });
    output.textures_delta.clear();
    Frame {
        drew,
        out,
        open,
        output,
    }
}

fn click(ctx: &Context, band: &Band<'_>, library: &Library, at: Pos2) -> Frame {
    let mut last = run(ctx, band, library, Table::Full, Vec::new());
    for event in click_events(at) {
        last = run(ctx, band, library, Table::Full, vec![event]);
    }
    last
}

fn painted(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .flat_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => vec![text.galley.text().to_owned()],
            egui::epaint::Shape::Vec(shapes) => shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn probe<T>(
    ctx: &Context,
    band: &Band<'_>,
    library: &Library,
    table: Table,
    events: Vec<Event>,
    find: impl Fn(&egui::Ui) -> T,
) -> T {
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut found = None;
    let mut output = ctx.run_ui(input(events), |ui| {
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        let _ = show(ui, band, NOW_MS, &tokens, table, &mut draw, &mut out);
        found = Some(find(ui));
    });
    output.textures_delta.clear();
    found.expect("the probe drew a frame")
}

fn name_rect(output: &egui::FullOutput, label: &str) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label} was not painted"))
}

/// A point inside the card's picture, found from the name painted under it.
fn reveal_point(output: &egui::FullOutput, label: &str) -> Pos2 {
    let name = name_rect(output, label);
    Pos2::new(name.center().x, name.top() - 10.0)
}

#[test]
fn the_band_draws_its_title_and_every_seed_it_holds() {
    let ctx = context();
    let seed_a = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let seed_b = seed(DiscoveryKind::Artist, "spotify:artist:seed", "Marta");
    let cards: Vec<DiscoveryCard> = (0..4).map(|at| card(at, 1)).collect();
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![
        row(&seed_a, refs, RowRead::Ready),
        row(&seed_b, cards.iter().take(2).collect(), RowRead::Ready),
    ]);
    let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());

    assert!(frame.drew);
    let words = painted(&frame.output);
    assert!(words.iter().any(|word| word == HOME_DISCOVERY_TITLE));
    assert!(words.iter().any(|word| word == "Windows Down"));
    assert!(words.iter().any(|word| word == "Marta"));
    assert!(words.iter().any(|word| word == "Artist 0"));
    assert!(words.iter().any(|word| word == "4 ARTISTS"));
    assert!(words.iter().any(|word| word == "2 ARTISTS"));
}

#[test]
fn an_off_band_draws_its_switch_row_and_nothing_else() {
    let ctx = context();
    let output = run(
        &ctx,
        &Band::Off,
        &Library::default(),
        Table::Full,
        Vec::new(),
    );

    assert!(output.drew, "the off row is the band's only remaining copy");
    assert_eq!(
        painted(&output.output),
        vec![HOME_DISCOVERY_SWITCH_OFF.to_owned()],
        "off is the row that switched it, inverted"
    );

    let at = name_rect(&output.output, HOME_DISCOVERY_SWITCH_OFF).center();
    let frame = click(&ctx, &Band::Off, &Library::default(), at);
    assert_eq!(frame.out.switch, Some(true), "the row asks to turn back on");
}

#[test]
fn a_band_at_work_draws_the_switch_at_its_foot() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    for band in [
        Band::Rows(vec![row(&seed, refs.clone(), RowRead::Ready)]),
        Band::Rows(vec![row(&seed, Vec::new(), RowRead::Loading)]),
        Band::Waiting,
    ] {
        let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
        assert!(dry.drew, "{band:?}");
        assert_eq!(
            painted(&dry.output)
                .iter()
                .filter(|word| *word == HOME_DISCOVERY_SWITCH_ON)
                .count(),
            1,
            "{band:?} draws one switch"
        );

        let at = name_rect(&dry.output, HOME_DISCOVERY_SWITCH_ON).center();
        let frame = click(&ctx, &band, &Library::default(), at);
        assert_eq!(frame.out.switch, Some(false), "{band:?}");
    }
}

#[test]
fn absent_and_unreachable_draw_nothing() {
    let ctx = context();
    for band in [Band::Absent, Band::Unreachable] {
        let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
        assert!(!frame.drew, "{band:?}");
        assert!(painted(&frame.output).is_empty(), "{band:?}");
    }
}

#[test]
fn clicking_a_rows_seed_opens_the_page_it_names() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let at = name_rect(&dry.output, "Windows Down").center();
    let frame = click(&ctx, &band, &Library::default(), at);

    assert_eq!(
        frame.open,
        Some(crate::route::Page::Playlist(SEED.to_owned()))
    );
}

#[test]
fn a_loading_row_draws_its_head_with_no_ledger_or_buttons() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let band = Band::Rows(vec![row(&seed, Vec::new(), RowRead::Loading)]);
    let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());

    let words = painted(&frame.output);
    assert!(words.iter().any(|word| word == "Windows Down"));
    assert!(
        words
            .iter()
            .any(|word| word == &discovery_source(DiscoveryKind::Playlist, Some("Alternative"))),
        "the head still says where it came from"
    );
    assert!(
        !words.iter().any(|word| word.contains("artist")),
        "a loading row counts nothing: {words:?}"
    );

    let (reroll, play) = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        Vec::new(),
        |ui| {
            (
                ui.ctx()
                    .read_response(controls::slot_id(ui, Control::DiscoveryReroll, SEED)),
                ui.ctx()
                    .read_response(controls::slot_id(ui, Control::DiscoveryRowPlay, SEED)),
            )
        },
    );
    assert!(reroll.is_none(), "a loading row has no re-roll");
    assert!(play.is_none(), "a loading row has no play");
}

#[test]
fn a_landed_row_paints_its_ledger() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards: Vec<DiscoveryCard> = (0..4).map(|at| card(at, 1)).collect();
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());

    assert!(
        painted(&frame.output)
            .iter()
            .any(|word| word == "4 ARTISTS")
    );
}

#[test]
fn a_short_row_draws_no_retry_and_no_empty_slot() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Album, "spotify:album:seed", "Halcyon");
    let cards = vec![card(0, 1), card(1, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let words = painted(&frame.output);

    assert!(words.iter().any(|word| word == "2 ARTISTS"));
    assert!(!words.iter().any(|word| word == "Retry"));
    assert!(!words.iter().any(|word| word == "Artist 2"));
}

#[test]
fn a_split_draws_three_mini_rows_and_its_play_queues_them() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Artist, SEED, "Marta");
    let cards = vec![card(0, 3)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let words = painted(&frame.output);

    for at in 0..3 {
        assert!(
            words.iter().any(|word| word == &format!("Track {at}")),
            "the split draws its mini-rows: {words:?}"
        );
    }
    assert!(words.iter().any(|word| word == "1 ARTIST · 3 TRACKS"));

    let play = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        Vec::new(),
        |ui| {
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryRowPlay, SEED))
                .expect("the row has a play")
                .rect
                .center()
        },
    );
    let frame = click(&ctx, &band, &Library::default(), play);
    assert_eq!(frame.out.discovery, Some(cards[0].tracks.clone()));
}

#[test]
fn pressing_row_two_of_three_queues_rows_two_and_three() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Artist, SEED, "Marta");
    let cards = vec![card(0, 3)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let at = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            ui.ctx()
                .read_response(
                    ui.scope_id()
                        .with(("home-station-row", ARTIST, "spotify:track:1")),
                )
                .expect("the second mini-row is a widget")
                .rect
                .center()
        },
    );
    let frame = click(&ctx, &band, &Library::default(), at);

    assert_eq!(
        frame.out.discovery,
        Some(cards[0].tracks[1..].to_vec()),
        "the press starts at the row it names"
    );
}

#[test]
fn a_card_play_queues_only_its_first_track() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 3)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let key = (SEED, ARTIST);
    let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let at = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryPlay, key))
                .expect("the card has a play")
                .rect
                .center()
        },
    );
    let frame = click(&ctx, &band, &Library::default(), at);

    let pressed = frame.out.discovery.expect("the card's play");
    assert_eq!(pressed, vec![cards[0].tracks[0].clone()]);
    let mut library = Library::default();
    assert!(library.want_play_rows(None, pressed));
    assert_eq!(
        library.take_play().map(|start| start.tracks),
        Some(vec![cards[0].tracks[0].clone()]),
        "the card's play reaches the queue"
    );
}

#[test]
fn the_cards_corner_controls_live_in_the_picture_not_the_card() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let key = (SEED, ARTIST);
    let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let name = name_rect(&dry.output, "Artist 0");
    let (card, follow, more, play) = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            let rect = |control| {
                ui.ctx()
                    .read_response(controls::slot_id(ui, control, key))
                    .expect("the card draws its corners")
                    .rect
            };
            (
                rect(Control::DiscoveryCard),
                rect(Control::DiscoveryFollow),
                rect(Control::DiscoveryMore),
                rect(Control::DiscoveryPlay),
            )
        },
    );

    let art = crate::layout::discovery::card_art(card);
    for tile in [follow, more, play] {
        assert!(
            art.contains_rect(tile),
            "a corner control sits outside the picture: {tile:?} against {art:?}"
        );
    }
    assert!(
        art.bottom() <= name.top(),
        "the picture ends above the words: {art:?} against {name:?}"
    );
    assert!(
        (play.top() - follow.top()).abs() < 0.01,
        "one row of corners"
    );
    assert!(play.right() < follow.left() && follow.right() < more.left());
}

#[test]
fn pressing_the_follow_tile_asks_for_the_follow_and_opens_nothing() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let key = (SEED, ARTIST);
    let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let at = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryFollow, key))
                .expect("the follow tile is drawn present")
                .rect
                .center()
        },
    );
    let frame = click(&ctx, &band, &Library::default(), at);

    assert_eq!(frame.out.follow, Some(ARTIST.to_owned()));
    assert_eq!(frame.out.discovery, None);
    assert_eq!(frame.open, None, "the tile does not fall through");
}

#[test]
fn a_pending_follow_makes_the_tile_inert_without_falling_through() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let mut library = Library::default();
    assert!(
        library.ask_follow(ARTIST, true).is_some(),
        "arrange: the follow must be in flight"
    );
    let key = (SEED, ARTIST);
    let dry = run(&ctx, &band, &library, Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let at = probe(
        &ctx,
        &band,
        &library,
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryFollow, key))
                .expect("the follow tile is drawn present")
                .rect
                .center()
        },
    );
    let frame = click(&ctx, &band, &library, at);

    assert_eq!(frame.out.follow, None, "a write already out is not doubled");
    assert_eq!(frame.open, None, "an inert tile does not fall through");
}

#[test]
fn the_card_menu_is_posted_with_only_the_add_live() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 2)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let key = (SEED, ARTIST);
    let dry = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let at = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryMore, key))
                .expect("the card has a menu")
                .rect
                .center()
        },
    );
    let _ = click(&ctx, &band, &Library::default(), at);

    let request =
        crate::view::context_menu::take_request(&ctx).expect("the card posted its menu this frame");
    let live: Vec<bool> = request.items.iter().map(|item| item.live).collect();
    assert_eq!(live, [true, true, true]);
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::AddToPlaylist)
    );
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::HideFromDiscovery)
    );
}

fn enter() -> [Event; 2] {
    let key = |pressed| Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    [key(true), key(false)]
}

fn menu_key() -> [Event; 2] {
    let key = |pressed| Event::Key {
        key: egui::Key::Menu,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    [key(true), key(false)]
}

/// Focus the card with one Tab, returning the frame it focused.
fn focus_card(
    ctx: &Context,
    band: &Band<'_>,
    library: &Library,
) -> (
    crate::layout::Table,
    (egui::Id, egui::Id, egui::Id, egui::Id),
) {
    let [press, release] = crate::view::test_support::tab();
    // The row's own play is the band's first stop; the card is the second.
    for _ in 0..2 {
        let _ = run(ctx, band, library, Table::Full, vec![press.clone()]);
        let _ = run(ctx, band, library, Table::Full, vec![release.clone()]);
    }
    let key = (SEED, ARTIST);
    let dry = run(ctx, band, library, Table::Full, Vec::new());
    let point = reveal_point(&dry.output, "Artist 0");
    let ids = probe(
        ctx,
        band,
        library,
        Table::Full,
        vec![Event::PointerMoved(point)],
        |ui| {
            (
                controls::slot_id(ui, Control::DiscoveryCard, key),
                controls::slot_id(ui, Control::DiscoveryMore, key),
                controls::slot_id(ui, Control::DiscoveryPlay, key),
                controls::slot_id(ui, Control::DiscoveryFollow, key),
            )
        },
    );
    (Table::Full, ids)
}

#[test]
fn tab_reaches_the_card_then_its_corners() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let library = Library::default();
    let (table, (card_id, more_id, play_id, follow_id)) = focus_card(&ctx, &band, &library);
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(card_id),
        "the card is the band's first Tab stop"
    );

    let [press, release] = crate::view::test_support::tab();
    let mut reached = Vec::new();
    for _ in 0..3 {
        let _ = run(&ctx, &band, &library, table, vec![press.clone()]);
        reached.push(ctx.memory(|memory| memory.focused()));
        let _ = run(&ctx, &band, &library, table, vec![release.clone()]);
    }
    assert_eq!(reached, [Some(follow_id), Some(more_id), Some(play_id)]);
}

#[test]
fn enter_on_the_card_opens_its_artist() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let library = Library::default();
    let (table, _) = focus_card(&ctx, &band, &library);

    let [press, release] = enter();
    let pressed = run(&ctx, &band, &library, table, vec![press]);
    let released = run(&ctx, &band, &library, table, vec![release]);

    assert_eq!(
        pressed.open.or(released.open),
        Some(crate::route::Page::Artist(ARTIST.to_owned()))
    );
}

#[test]
fn the_menu_key_on_the_card_posts_its_menu() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let library = Library::default();
    let (table, _) = focus_card(&ctx, &band, &library);

    let [press, release] = menu_key();
    let _ = run(&ctx, &band, &library, table, vec![press]);
    let _ = run(&ctx, &band, &library, table, vec![release]);

    let request =
        crate::view::context_menu::take_request(&ctx).expect("the menu key posted the card's menu");
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::AddToPlaylist)
    );
}

fn card_with_year(index: usize, tracks: usize, year: Option<u16>) -> DiscoveryCard {
    DiscoveryCard {
        latest_year: year,
        ..card(index, tracks)
    }
}

#[test]
fn the_provenance_carries_a_genre_only_when_the_row_named_one() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Album, "spotify:album:seed", "Halcyon");
    let cards = vec![card(0, 1)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![RowView {
        seed: &seed,
        genre: None,
        cards: refs,
        read: RowRead::Ready,
    }]);
    let frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    let words = painted(&frame.output);

    assert!(
        words
            .iter()
            .any(|word| word == &discovery_source(DiscoveryKind::Album, None)),
        "a row with no genre leaves no dangling separator"
    );
}

#[test]
fn a_cards_sub_line_is_drawn_only_when_the_wire_dated_it() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let dated = vec![card_with_year(0, 1, Some(2024))];
    let dated_refs: Vec<&DiscoveryCard> = dated.iter().collect();
    let band = Band::Rows(vec![row(&seed, dated_refs, RowRead::Ready)]);
    let dated_frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    assert!(
        painted(&dated_frame.output)
            .iter()
            .any(|word| word == "Latest 2024")
    );

    let undated = vec![card_with_year(0, 1, None)];
    let undated_refs: Vec<&DiscoveryCard> = undated.iter().collect();
    let band = Band::Rows(vec![row(&seed, undated_refs, RowRead::Ready)]);
    let undated_frame = run(&ctx, &band, &Library::default(), Table::Full, Vec::new());
    assert!(
        !painted(&undated_frame.output)
            .iter()
            .any(|word| word.starts_with("Latest")),
        "an undated card draws no sub line"
    );
}

#[test]
fn the_ledger_goes_from_nothing_to_its_count_when_the_row_lands() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Playlist, SEED, "Windows Down");
    let loading = Band::Rows(vec![row(&seed, Vec::new(), RowRead::Loading)]);
    let loading_frame = run(&ctx, &loading, &Library::default(), Table::Full, Vec::new());
    assert!(
        !painted(&loading_frame.output)
            .iter()
            .any(|word| word.ends_with("ARTISTS")),
        "a row in flight counts nothing"
    );

    let cards: Vec<DiscoveryCard> = (0..4).map(|at| card(at, 1)).collect();
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let landed = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);
    let landed_frame = run(&ctx, &landed, &Library::default(), Table::Full, Vec::new());
    assert!(
        painted(&landed_frame.output)
            .iter()
            .any(|word| word == "4 ARTISTS")
    );
}

#[test]
fn a_plays_rows_reach_the_librarys_queue() {
    let ctx = context();
    let seed = seed(DiscoveryKind::Artist, SEED, "Marta");
    let cards = vec![card(0, 3)];
    let refs: Vec<&DiscoveryCard> = cards.iter().collect();
    let band = Band::Rows(vec![row(&seed, refs, RowRead::Ready)]);

    let play = probe(
        &ctx,
        &band,
        &Library::default(),
        Table::Full,
        Vec::new(),
        |ui| {
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryRowPlay, SEED))
                .expect("the row has a play")
                .rect
                .center()
        },
    );
    let frame = click(&ctx, &band, &Library::default(), play);
    let mut library = Library::default();
    assert!(library.want_play_rows(None, frame.out.discovery.expect("a play")));
    assert_eq!(
        library.take_play().map(|start| start.tracks),
        Some(cards[0].tracks.clone())
    );
}
