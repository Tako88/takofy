use egui::{Event, Key, Modifiers};

use super::super::*;
use super::test_support::*;
use crate::theme::Tokens;
use crate::view::test_support::{click_events, secondary_click_events};

#[test]
fn a_click_on_the_like_does_not_select_or_play() {
    let ctx = context();
    let row = row_of(1);
    let point = columns().at(body_rect().min).like.center();
    // A double click, because the second click is the one that would
    // otherwise play the row underneath the control.
    let mut events = click_events(point).to_vec();
    events.extend(click_events(point));
    let answers = run(&ctx, &row, &events, Modifiers::NONE);

    assert!(
        answers.iter().flatten().next().is_none(),
        "a click on the like reported {answers:?}"
    );
}

/// A row whose heart knows its state, or does not.
fn hearted(liked: Option<bool>) -> Row<'static> {
    Row { liked, ..row_of(1) }
}

fn like_rect() -> Rect {
    columns().at(body_rect().min).like
}

/// The sense the heart's slot was registered with this frame, if it drew.
fn heart_sense(ctx: &egui::Context, row: &Row<'_>, events: Vec<Event>) -> Option<egui::Sense> {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        let _ = show(ui, body_rect(), columns(), row, &tokens);
    });
    output.textures_delta.clear();
    let mut sense = None;
    let mut output = ctx.run_ui(input_with(events), |ui| {
        let _ = show(ui, body_rect(), columns(), row, &tokens);
        sense = ui
            .ctx()
            .read_response(controls::slot_id(
                ui,
                Control::TrackLike,
                (row.uri, row.number),
            ))
            .map(|response| response.sense);
    });
    output.textures_delta.clear();
    sense
}

#[test]
fn a_liked_row_draws_its_heart_with_no_hover_or_focus() {
    let ctx = context();
    let output = frame(&ctx, &hearted(Some(true)), Vec::new());
    assert!(
        !glyph_colors(&output, like_rect()).is_empty(),
        "a liked heart is not visible at rest"
    );
}

#[test]
fn an_unliked_row_draws_no_heart_until_hover_or_focus() {
    let ctx = context();
    let output = frame(&ctx, &hearted(Some(false)), Vec::new());
    assert!(
        glyph_colors(&output, like_rect()).is_empty(),
        "an unliked heart was drawn at rest"
    );

    let output = frame(
        &ctx,
        &hearted(Some(false)),
        vec![Event::PointerMoved(body_point())],
    );
    assert!(
        !glyph_colors(&output, like_rect()).is_empty(),
        "an unliked heart was not revealed under the pointer"
    );
}

#[test]
fn focus_on_the_more_button_reveals_the_heart() {
    let ctx = context();
    let row = Row {
        menu: true,
        liked: Some(false),
        ..row_of(1)
    };
    // The click gives the `⋯` the keyboard; the menu it opens is not the
    // point here.
    let _ = run(
        &ctx,
        &row,
        &click_events(actions().center()),
        Modifiers::NONE,
    );
    let output = frame(&ctx, &row, vec![Event::PointerMoved(Pos2::new(1.0, 1.0))]);
    assert!(
        !glyph_colors(&output, like_rect()).is_empty(),
        "the heart vanished while the `⋯` held the keyboard"
    );
}

#[test]
fn focus_on_the_heart_keeps_it_drawn_and_focused() {
    let ctx = context();
    let row = hearted(Some(false));
    let point = like_rect().center();
    let first = run(&ctx, &row, &click_events(point), Modifiers::NONE);
    assert_eq!(
        first.iter().flatten().last(),
        Some(&Click::Like(true)),
        "the heart did not take the click"
    );

    // The pointer leaves; the heart must stay drawn because it holds the
    // keyboard, and it must still answer Enter.
    let _ = run(
        &ctx,
        &row,
        &[Event::PointerMoved(Pos2::new(1.0, 1.0))],
        Modifiers::NONE,
    );
    let output = frame(&ctx, &row, Vec::new());
    assert!(
        !glyph_colors(&output, like_rect()).is_empty(),
        "the focused heart was hidden the frame after it took the keyboard"
    );
    let enter = |pressed| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let answers = run(&ctx, &row, &[enter(true), enter(false)], Modifiers::NONE);
    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Like(true)),
        "the focused heart did not answer Enter"
    );
}

#[test]
fn a_click_on_a_live_heart_reports_like_and_not_select_play_or_open() {
    let ctx = context();
    let row = hearted(Some(false));
    let answers = run(
        &ctx,
        &row,
        &click_events(like_rect().center()),
        Modifiers::NONE,
    );
    assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(true)));
    assert!(
        !answers.iter().flatten().any(|click| matches!(
            click,
            Click::Select(_) | Click::Play | Click::Open(_) | Click::Lift
        )),
        "a heart click did something else too: {answers:?}"
    );
}

#[test]
fn the_click_asks_for_the_opposite_of_the_rows_state() {
    let ctx = context();
    let row = hearted(Some(true));
    let answers = run(
        &ctx,
        &row,
        &click_events(like_rect().center()),
        Modifiers::NONE,
    );
    assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(false)));
}

#[test]
fn an_unknown_heart_reports_nothing() {
    let ctx = context();
    let row = hearted(None);
    let answers = run(
        &ctx,
        &row,
        &click_events(like_rect().center()),
        Modifiers::NONE,
    );
    assert!(
        answers.iter().flatten().next().is_none(),
        "an unknown heart reported {answers:?}"
    );
}

#[test]
fn a_double_click_on_the_heart_does_not_play() {
    let ctx = context();
    let row = hearted(Some(false));
    let point = like_rect().center();
    let mut events = click_events(point).to_vec();
    events.extend(click_events(point));
    let answers = run(&ctx, &row, &events, Modifiers::NONE);
    assert!(
        !answers.iter().flatten().any(|click| *click == Click::Play),
        "a double click on the heart played the row: {answers:?}"
    );
}

#[test]
fn a_right_click_on_the_heart_still_opens_the_menu() {
    let ctx = context();
    let row = Row {
        menu: true,
        liked: Some(false),
        ..row_of(1)
    };
    let answers = run(
        &ctx,
        &row,
        &secondary_click_events(like_rect().center()),
        Modifiers::NONE,
    );
    assert_eq!(
        door(&answers).map(|door| door.device),
        Some(Device::Pointer),
        "a right-click on the heart opened no menu: {answers:?}"
    );
}

#[test]
fn enter_on_a_focused_heart_toggles_once() {
    let ctx = context();
    let row = hearted(Some(false));
    let _ = run(
        &ctx,
        &row,
        &click_events(like_rect().center()),
        Modifiers::NONE,
    );
    let enter = |repeat| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    let answers = run(&ctx, &row, &[enter(false)], Modifiers::NONE);
    assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(true)));
}

#[test]
fn a_held_enter_on_the_heart_toggles_once() {
    let ctx = context();
    let row = hearted(Some(false));
    let _ = run(
        &ctx,
        &row,
        &click_events(like_rect().center()),
        Modifiers::NONE,
    );
    let enter = |repeat| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    let answers = run(
        &ctx,
        &row,
        &[enter(false), enter(true), enter(true)],
        Modifiers::NONE,
    );
    assert_eq!(
        answers
            .iter()
            .flatten()
            .filter(|click| matches!(click, Click::Like(_)))
            .count(),
        1,
        "a held Enter toggled more than once: {answers:?}"
    );
}

#[test]
fn the_heart_is_a_tab_stop_only_when_live() {
    let ctx = context();
    let live = heart_sense(
        &ctx,
        &hearted(Some(false)),
        vec![Event::PointerMoved(body_point())],
    );
    assert!(
        live.is_some_and(|sense| sense.is_focusable()),
        "a live heart is not a Tab stop"
    );

    let ctx = context();
    let unknown = heart_sense(
        &ctx,
        &hearted(None),
        vec![Event::PointerMoved(body_point())],
    );
    assert!(
        unknown.is_some_and(|sense| !sense.is_focusable()),
        "an unknown heart joined the Tab order"
    );
}

#[test]
fn a_drag_that_starts_on_the_heart_lifts_nothing() {
    let ctx = context();
    let row = hearted(Some(false));
    let answers = run(
        &ctx,
        &row,
        &twitch(like_rect().center(), 7.0),
        Modifiers::NONE,
    );
    assert!(
        !answers.iter().flatten().any(|click| *click == Click::Lift),
        "a drag from the heart lifted the row: {answers:?}"
    );
}

#[test]
fn a_drag_elsewhere_on_a_row_with_a_live_heart_still_lifts() {
    let ctx = context();
    let row = hearted(Some(false));
    let answers = run(&ctx, &row, &twitch(body_point(), 7.0), Modifiers::NONE);
    assert!(
        answers.iter().flatten().any(|click| *click == Click::Lift),
        "a live heart stopped the row's own drag: {answers:?}"
    );
}

#[test]
fn a_carried_row_draws_no_heart_even_when_liked() {
    let ctx = context();
    let row = Row {
        liked: Some(true),
        look: Look {
            carrying: true,
            ..Look::default()
        },
        ..row_of(1)
    };
    let output = frame(&ctx, &row, Vec::new());
    assert!(
        glyph_colors(&output, like_rect()).is_empty(),
        "a carried row drew its heart"
    );
}

#[test]
fn a_heart_stays_live_while_a_playlist_write_is_pending() {
    let ctx = context();
    // The page's gate is shut for the menu, but a heart is not the menu.
    let row = Row {
        menu: false,
        liked: Some(true),
        ..row_of(1)
    };
    let answers = run(
        &ctx,
        &row,
        &click_events(like_rect().center()),
        Modifiers::NONE,
    );
    assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(false)));
}

#[test]
fn the_liked_heart_is_painted_in_the_accent() {
    let ctx = context();
    let output = frame(&ctx, &hearted(Some(true)), Vec::new());
    let colors = glyph_colors(&output, like_rect());
    assert!(
        colors.contains(&Tokens::DARK.palette.accent),
        "the liked heart was painted {colors:?}"
    );
}
