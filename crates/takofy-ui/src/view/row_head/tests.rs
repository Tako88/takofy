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
        }),
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
    let tokens = Tokens::DARK;
    let mut opened = Opened::default();
    for event in click_events(at) {
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            let mut head = head();
            if inert {
                head.reroll = Some(Button {
                    control: Control::DiscoveryReroll,
                    key: "row",
                    live: false,
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
