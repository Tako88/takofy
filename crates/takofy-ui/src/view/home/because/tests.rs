use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::FollowedArtist;

use super::*;
use crate::cover::Art;
use crate::library::Library;
use crate::route::Page;
use crate::theme::{Tokens, fonts, style};
use crate::view::playlist::root::MediaDraw;
use crate::view::test_support::click_events;

use super::super::Pressed;

/// A window tall enough for the head and one row of cards.
const PROBE: Vec2 = Vec2::new(900.0, 700.0);
const NOW_MS: u64 = 1_700_000_000_000;

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

/// One pass over the shelf alone, with the press it took left in `open`.
fn draw(
    ctx: &Context,
    art: &mut Art,
    open: &mut Option<Page>,
    row: &Row<'_>,
    events: Vec<Event>,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let library = Library::default();
    let mut select = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut output = ctx.run_ui(input(events), |ui| {
        let mut draw = MediaDraw {
            art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open,
            play: &mut play,
        };
        show(ui, row, NOW_MS, &tokens, &mut draw, &mut out);
    });
    output.textures_delta.clear();
    output
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

/// The text one frame actually painted, in paint order.
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

#[test]
fn the_head_names_the_followed_artist_and_counts_nothing() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let cards = vec![artist("spotify:artist:z", "Zed")];
    let row = Row {
        seed_name: "Aria",
        seed_uri: "spotify:artist:aria",
        cards: &cards,
    };

    let output = draw(&ctx, &mut art, &mut open, &row, Vec::new());
    let painted = painted(&output);
    let head = format!("{HOME_BECAUSE_TITLE} Aria");

    assert!(
        painted.contains(&head.as_str()),
        "the head named something other than the followed artist"
    );
    assert!(
        !painted.contains(&"1"),
        "the head counted the cards it drew"
    );
}

#[test]
fn a_card_press_opens_the_artists_page() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let cards = vec![
        artist("spotify:artist:z", "Zed"),
        artist("spotify:artist:y", "Wye"),
    ];
    let row = Row {
        seed_name: "Aria",
        seed_uri: "spotify:artist:aria",
        cards: &cards,
    };

    let output = draw(&ctx, &mut art, &mut open, &row, Vec::new());
    let pos = text_rect(&output, "Wye")
        .expect("the card is drawn")
        .center();

    let mut opened = None;
    for event in click_events(pos) {
        draw(&ctx, &mut art, &mut open, &row, vec![event]);
        opened = opened.or(open.take());
    }

    assert_eq!(opened, Some(Page::Artist("spotify:artist:y".to_owned())));
}

#[test]
fn a_card_says_the_artists_name_and_no_kind_word() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let cards = vec![artist("spotify:artist:z", "Zed")];
    let row = Row {
        seed_name: "Aria",
        seed_uri: "spotify:artist:aria",
        cards: &cards,
    };

    let output = draw(&ctx, &mut art, &mut open, &row, Vec::new());
    let drawn = painted(&output);

    assert!(drawn.contains(&"Zed"), "the card draws its name");
    assert!(
        !drawn.contains(&"Artist"),
        "the reference's artist card is a face and a name: {drawn:?}"
    );
}

/// The rect the shelf's first card takes: the head, then the grid.
fn card_rect_for(ctx: &Context, row: &Row<'_>) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        section::head(
            ui,
            &format!("{HOME_BECAUSE_TITLE} {}", row.seed_name),
            "",
            &tokens,
        );
        found = Some(super::super::grid::solve(ui).card_rect(0));
    });
    output.textures_delta.clear();
    found.expect("the shelf lays out")
}

#[test]
fn a_press_on_a_cards_more_posts_the_artists_menu_with_its_radio() {
    use crate::view::context_menu;
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let cards = vec![artist("spotify:artist:z", "Zed")];
    let row = Row {
        seed_name: "Aria",
        seed_uri: "spotify:artist:aria",
        cards: &cards,
    };
    let corner =
        crate::layout::cards::corner_more(crate::view::media::art_rect(card_rect_for(&ctx, &row)));

    // A reader hovers the card before its corner is drawn.
    let _ = draw(
        &ctx,
        &mut art,
        &mut open,
        &row,
        vec![Event::PointerMoved(corner.center())],
    );
    let mut request = None;
    for event in click_events(corner.center()) {
        let _ = draw(&ctx, &mut art, &mut open, &row, vec![event]);
        request = request.or(context_menu::take_request(&ctx));
    }

    let request = request.expect("the card posted no menu");
    let offered: Vec<crate::menu::Verb> = request.items.iter().map(|item| item.verb).collect();
    assert!(
        offered.contains(&crate::menu::Verb::StartRadio),
        "the related artist offers no radio: {offered:?}"
    );
    assert!(
        offered.contains(&crate::menu::Verb::Follow),
        "a card nobody follows offers no follow: {offered:?}"
    );
    assert_eq!(open, None, "the `⋯` also opened the artist's page");
}
