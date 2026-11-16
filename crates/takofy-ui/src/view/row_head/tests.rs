use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

const PROBE: Vec2 = Vec2::new(800.0, 400.0);
const RECT: Rect = Rect {
    min: Pos2::new(20.0, 40.0),
    max: Pos2::new(620.0, 92.0),
};

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

/// One head with a seed page and a play, and no ledger or re-roll.
fn head<'a>() -> Head<'a, &'a str> {
    Head {
        seed: "Windows Down",
        key: "spotify:playlist:1",
        seed_page: Some(Page::Playlist("spotify:playlist:1".to_owned())),
        provenance: "From your playlist",
        ledger: None,
        reroll: None,
        waited: None,
        play: Some(Button {
            control: Control::StationPlay,
            key: "spotify:station:artist:1",
            live: true,
            busy: false,
        }),
        rank: SeedRank::Row,
    }
}

/// The rect the seed link was drawn in, and the play's rect.
fn places(ctx: &Context) -> (Rect, Rect) {
    let tokens = Tokens::DARK;
    let mut seed = Rect::NOTHING;
    let mut play = Rect::NOTHING;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        show(ui, RECT, head(), &tokens);
        seed = ui
            .ctx()
            .read_response(ui.scope_id().with(("row-head-seed", "spotify:playlist:1")))
            .map_or(Rect::NOTHING, |response| response.rect);
        play = ui
            .ctx()
            .read_response(controls::slot_id(
                ui,
                Control::StationPlay,
                "spotify:station:artist:1",
            ))
            .map_or(Rect::NOTHING, |response| response.rect);
    });
    output.textures_delta.clear();
    (seed, play)
}

/// The presses of one click, run in the passes egui needs, and what they opened.
fn press(ctx: &Context, at: Pos2, inert: bool) -> Opened {
    press_rank(ctx, at, inert, SeedRank::Row)
}

/// [`press`] on a head whose seed is set at `rank`.
fn press_rank(ctx: &Context, at: Pos2, inert: bool, rank: SeedRank) -> Opened {
    let tokens = Tokens::DARK;
    let mut opened = Opened::default();
    for event in click_events(at) {
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            let mut head = head();
            head.rank = rank;
            if inert {
                head.reroll = Some(Button {
                    control: Control::DiscoveryReroll,
                    key: "row",
                    live: false,
                    busy: false,
                });
            }
            let next = show(ui, RECT, head, &tokens);
            opened.seed = next.seed.or(opened.seed.take());
            opened.play |= next.play;
            opened.reroll |= next.reroll;
        });
        output.textures_delta.clear();
    }
    opened
}

#[test]
fn a_seed_link_reports_the_page_it_names() {
    let ctx = context();
    let (seed, _) = places(&ctx);
    assert!(seed.width() > 0.0, "the seed is drawn and clickable");

    let opened = press(&ctx, seed.center(), false);

    assert_eq!(
        opened.seed,
        Some(Page::Playlist("spotify:playlist:1".to_owned()))
    );
}

#[test]
fn an_inert_reroll_never_reports() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut reroll = Rect::NOTHING;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let mut head = head();
        head.reroll = Some(Button {
            control: Control::DiscoveryReroll,
            key: "row",
            live: false,
            busy: false,
        });
        show(ui, RECT, head, &tokens);
        reroll = ui
            .ctx()
            .read_response(controls::slot_id(ui, Control::DiscoveryReroll, "row"))
            .map_or(Rect::NOTHING, |response| response.rect);
    });
    output.textures_delta.clear();
    assert!(reroll.width() > 0.0, "the re-roll is drawn present");

    let opened = press(&ctx, reroll.center(), true);

    assert!(!opened.reroll, "an inert re-roll does nothing");
}

#[test]
fn a_busy_reroll_holds_its_turn_and_reports_nothing() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut square = Rect::NOTHING;
    let mut sense = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let mut head = head();
        head.reroll = Some(busy());
        show(ui, RECT, head, &tokens);
        let response =
            ui.ctx()
                .read_response(controls::slot_id(ui, Control::DiscoveryReroll, "row"));
        if let Some(response) = response {
            square = response.rect;
            sense = Some(response.sense);
        }
    });
    output.textures_delta.clear();
    assert!(square.width() > 0.0, "the busy square is drawn present");
    assert!(
        sense.is_some_and(|sense| sense.is_focusable()),
        "a busy square keeps its Tab stop"
    );

    let mut opened = Opened::default();
    for event in click_events(square.center()) {
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            let mut head = head();
            head.reroll = Some(busy());
            opened.reroll |= show(ui, RECT, head, &tokens).reroll;
        });
        output.textures_delta.clear();
    }

    assert!(!opened.reroll, "a square holding its turn cannot act");
}

/// The re-roll holding its turn: live, but waiting.
fn busy() -> Button<&'static str> {
    Button {
        control: Control::DiscoveryReroll,
        key: "row",
        live: true,
        busy: true,
    }
}

#[test]
fn a_live_play_reports() {
    let ctx = context();
    let (_, play) = places(&ctx);
    let opened = press(&ctx, play.center(), false);
    assert!(opened.play);
}

#[test]
fn a_spent_row_registers_no_reroll_and_the_play_keeps_the_edge() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut reroll = Rect::NOTHING;
    let mut play = Rect::NOTHING;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        show(ui, RECT, head(), &tokens);
        reroll = ui
            .ctx()
            .read_response(controls::slot_id(ui, Control::DiscoveryReroll, "row"))
            .map_or(Rect::NOTHING, |response| response.rect);
        play = ui
            .ctx()
            .read_response(controls::slot_id(
                ui,
                Control::StationPlay,
                "spotify:station:artist:1",
            ))
            .map_or(Rect::NOTHING, |response| response.rect);
    });
    output.textures_delta.clear();

    assert_eq!(reroll, Rect::NOTHING, "nothing to draw, nothing to click");
    assert!(
        (play.right() - RECT.right()).abs() < 0.01,
        "the play takes the edge the re-roll left"
    );
}

#[test]
fn a_cooling_reroll_is_drawn_with_the_edge_it_has_walked() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut reroll = Rect::NOTHING;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let mut head = head();
        head.reroll = Some(Button {
            control: Control::DiscoveryReroll,
            key: "row",
            live: false,
            busy: false,
        });
        head.waited = Some(0.5);
        show(ui, RECT, head, &tokens);
        reroll = ui
            .ctx()
            .read_response(controls::slot_id(ui, Control::DiscoveryReroll, "row"))
            .map_or(Rect::NOTHING, |response| response.rect);
    });
    let walked = output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::epaint::Shape::Path(path) => {
                !path.closed
                    && matches!(
                        path.stroke.color,
                        egui::epaint::ColorMode::Solid(colour) if colour == tokens.palette.text_muted
                    )
                    && path.points.iter().all(|point| reroll.expand(1.0).contains(*point))
            }
            _ => false,
        })
        .count();
    output.textures_delta.clear();

    assert_eq!(
        walked, 1,
        "the half-walked edge is drawn on the button, not just its border"
    );
}

/// The painted height of the seed line, and the rect the seed link took.
fn seed_line(ctx: &Context, rank: SeedRank) -> (f32, Rect) {
    let tokens = Tokens::DARK;
    let mut seed = Rect::NOTHING;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let mut head = head();
        head.rank = rank;
        show(ui, RECT, head, &tokens);
        seed = ui
            .ctx()
            .read_response(ui.scope_id().with(("row-head-seed", "spotify:playlist:1")))
            .map_or(Rect::NOTHING, |response| response.rect);
    });
    let height = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Windows Down" => {
                Some(text.galley.size().y)
            }
            _ => None,
        })
        .unwrap_or(0.0);
    output.textures_delta.clear();
    (height, seed)
}

#[test]
fn a_shelf_seed_reads_at_the_title_rank_and_still_links() {
    let ctx = context();
    let (row_h, row_seed) = seed_line(&ctx, SeedRank::Row);
    let (shelf_h, shelf_seed) = seed_line(&ctx, SeedRank::Shelf);

    assert!(
        shelf_h > row_h,
        "the shelf's seed is set at the title rank: {shelf_h} vs {row_h}"
    );
    for (rank, seed) in [(SeedRank::Row, row_seed), (SeedRank::Shelf, shelf_seed)] {
        assert!(seed.width() > 0.0, "{rank:?} draws a clickable seed");
        let opened = press_rank(&ctx, seed.center(), false, rank);
        assert_eq!(
            opened.seed,
            Some(Page::Playlist("spotify:playlist:1".to_owned())),
            "{rank:?} seed is a link"
        );
    }
}
