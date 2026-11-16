use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::home::{NewRelease, ReleaseDate};

use super::*;
use crate::cover::Art;
use crate::home::releases::Card;
use crate::library::Library;
use crate::message::Failure;
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

/// The probe window: wide enough for the page's four card columns.
const PROBE: Vec2 = Vec2::new(1200.0, 900.0);

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

/// What one pass over the shelf asked for, and the room it took.
#[derive(Default)]
struct Pressed {
    open: Option<Page>,
    play: Option<(String, SidebarKind)>,
    height: f32,
}

fn run(
    ctx: &Context,
    events: Vec<Event>,
    section: &Section<List<'_>>,
) -> (Pressed, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let library = Library::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut pressed = Pressed::default();
    let mut output = ctx.run_ui(input(events), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    let top = ui.cursor().min.y;
                    let mut draw = MediaDraw {
                        art: &mut art,
                        now: Instant::now(),
                        tokens: &tokens,
                        library: &library,
                        select: &mut select,
                        open: &mut open,
                        play: &mut play,
                    };
                    show(ui, section, &tokens, &mut draw);
                    pressed.height = ui.min_rect().bottom() - top;
                    pressed.open = open.take();
                    pressed.play = play.take();
                });
            });
    });
    output.textures_delta.clear();
    (pressed, output)
}

/// One click, in the passes egui needs to report it.
fn click(ctx: &Context, pos: Pos2, section: &Section<List<'_>>) -> Pressed {
    let mut last = Pressed::default();
    for event in click_events(pos) {
        let (pressed, _) = run(ctx, vec![event], section);
        if pressed.open.is_some() || pressed.play.is_some() {
            last.open = pressed.open;
            last.play = pressed.play;
        }
    }
    last
}

/// The text one frame painted, in paint order.
fn painted(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

fn date(year: u16, month: Option<u8>, day: Option<u8>) -> ReleaseDate {
    ReleaseDate { year, month, day }
}

fn release(uri: &str, name: &str, artist: &str, at: ReleaseDate) -> NewRelease {
    NewRelease {
        artist: artist.to_owned(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        date: at,
        day: takofy_model::home::Day::from_days(739_600),
        cover: None,
    }
}

/// Two releases, one dated by its day and one by its year alone.
fn releases() -> Vec<NewRelease> {
    vec![
        release(
            "spotify:album:a",
            "Slow Tide",
            "Vela Moreno",
            date(2025, Some(10), Some(14)),
        ),
        release(
            "spotify:album:b",
            "Winter Light",
            "Fen & Farrow",
            date(1996, None, None),
        ),
    ]
}

fn list(releases: &[NewRelease]) -> List<'_> {
    List {
        cards: releases.iter().map(|release| Card { release }).collect(),
    }
}

fn ready(releases: &[NewRelease]) -> Section<List<'_>> {
    Section::Ready(list(releases))
}

/// The rect the shelf's `index`-th card takes: the shelf head, then the grid.
fn card_rect_for(ctx: &Context, index: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    section::head(ui, HOME_RELEASES_TITLE, "2 releases", &tokens);
                    found = Some(super::super::grid::solve(ui).card_rect(index));
                });
            });
    });
    output.textures_delta.clear();
    found.expect("the shelf lays out")
}

#[test]
fn the_head_names_the_shelf_and_counts_what_it_drew() {
    let ctx = context();
    let releases = releases();
    let (_, output) = run(&ctx, Vec::new(), &ready(&releases));
    let drawn = painted(&output);

    assert_eq!(
        drawn.first(),
        Some(&HOME_RELEASES_TITLE),
        "the shelf's title comes first"
    );
    assert!(
        drawn.contains(&release_count_label(2).to_uppercase().as_str()),
        "the aside counts what is drawn: {drawn:?}"
    );
}

#[test]
fn a_card_draws_its_title_and_its_sub_in_both_date_shapes() {
    let ctx = context();
    let releases = releases();
    let (_, output) = run(&ctx, Vec::new(), &ready(&releases));
    let drawn = painted(&output);

    assert!(drawn.contains(&"Slow Tide"));
    assert!(drawn.contains(&"Vela Moreno · 14 Oct"));
    assert!(drawn.contains(&"Winter Light"));
    assert!(
        drawn.contains(&"Fen & Farrow · 1996"),
        "a year-only release draws its year: {drawn:?}"
    );
}

#[test]
fn a_press_on_a_cards_play_wants_the_release() {
    let ctx = context();
    let releases = releases();
    let section = ready(&releases);
    let card = card_rect_for(&ctx, 0);

    let pressed = click(
        &ctx,
        crate::view::media::tile_play_rect(card).center(),
        &section,
    );

    assert_eq!(
        pressed.play,
        Some((
            "spotify:album:a".to_owned(),
            crate::sidebar_kind::SidebarKind::Albums
        )),
        "the press never became a play"
    );
    assert_eq!(pressed.open, None, "a play press opened the album");
}

#[test]
fn a_press_on_a_card_opens_the_album() {
    let ctx = context();
    let releases = releases();
    let section = ready(&releases);
    let card = card_rect_for(&ctx, 1);

    let pressed = click(&ctx, card.center(), &section);

    assert_eq!(
        pressed.open,
        Some(Page::Album("spotify:album:b".to_owned()))
    );
    assert_eq!(pressed.play, None);
}

#[test]
fn an_absent_or_failed_shelf_takes_no_room() {
    for section in [
        Section::<List<'_>>::Absent,
        Section::<List<'_>>::Failed(Failure::Load),
    ] {
        let ctx = context();
        let (pressed, output) = run(&ctx, Vec::new(), &section);
        assert!(painted(&output).is_empty(), "there is no head to draw");
        assert_eq!(pressed.height, 0.0, "the shelf took room it did not fill");
    }
}

#[test]
fn a_waiting_shelf_draws_its_title_and_one_skeleton_row() {
    let ctx = context();
    let section = Section::<List<'_>>::Waiting;
    let (_, output) = run(&ctx, Vec::new(), &section);

    assert_eq!(
        painted(&output),
        vec![HOME_RELEASES_TITLE],
        "the waiting shelf drew more than its title"
    );
    let waiting = {
        let (pressed, _) = run(&ctx, Vec::new(), &section);
        pressed.height
    };
    let ready = {
        let releases = releases();
        let (pressed, _) = run(&ctx, Vec::new(), &ready(&releases));
        pressed.height
    };
    assert!(waiting > 0.0, "the waiting shelf held no room");
    assert_eq!(
        waiting, ready,
        "the shelf's content moved the page when it landed"
    );
}

/// Hovers the first card, then presses its `⋯`, and answers the request posted.
fn more_press(
    ctx: &Context,
    section: &Section<List<'_>>,
    at: Rect,
) -> Option<crate::menu::Request> {
    use crate::view::context_menu;
    let _ = run(ctx, vec![Event::PointerMoved(at.center())], section);
    let mut request = None;
    for event in click_events(at.center()) {
        let _ = run(ctx, vec![event], section);
        request = request.or(context_menu::take_request(ctx));
    }
    request
}

#[test]
fn a_press_on_a_cards_more_posts_the_albums_menu_with_its_radio() {
    let ctx = context();
    let releases = releases();
    let section = ready(&releases);
    let card = card_rect_for(&ctx, 0);
    let art = crate::view::media::art_rect(card);
    let corner = crate::layout::cards::corner_more(art);

    let request = more_press(&ctx, &section, corner).expect("the card posted no menu");
    let offered: Vec<crate::menu::Verb> = request.items.iter().map(|item| item.verb).collect();

    assert!(
        offered.contains(&crate::menu::Verb::StartRadio),
        "the release card offers no radio: {offered:?}"
    );
    assert!(
        offered.contains(&crate::menu::Verb::CopyLink),
        "the release card's menu is missing its album's verbs: {offered:?}"
    );
    assert_eq!(
        request.targets,
        Vec::new(),
        "a card is not a pick of tracks"
    );
}
