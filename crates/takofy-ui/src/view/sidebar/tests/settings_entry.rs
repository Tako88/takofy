use super::super::*;
use super::test_support::*;

use egui::Event;

/// One frame of the drawer form: its output, and what it asked the shell for.
fn asked(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
) -> (egui::FullOutput, Asked) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
        events,
        ..Default::default()
    };
    let tokens = Tokens::DARK;
    let mut out = Asked::default();
    let mut output = ctx.run_ui(input, |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        out = show(ui, library, art, Instant::now(), &tokens, true);
    });
    output.textures_delta.clear();
    (output, out)
}

/// Where the foot row's word was painted.
fn foot(output: &egui::FullOutput) -> Pos2 {
    let (pos, size) = painted_text(output, "SETTINGS");
    Pos2::new(pos.x + size.x / 2.0, pos.y + size.y / 2.0)
}

#[test]
fn the_foot_row_opens_the_settings_page() {
    let ctx = context();
    let mut library = Library::default();
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let command = click(&ctx, foot(&output), &mut library, &mut art);
    assert!(command.is_none(), "the page arms no read");
    assert_eq!(library.page(), Some(&Page::Settings));
}

#[test]
fn the_foot_row_closes_the_drawer_and_sends_nothing() {
    let ctx = context();
    let mut library = Library::default();
    let mut art = Art::default();
    let (output, first) = asked(&ctx, Vec::new(), &mut library, &mut art);
    assert!(!first.picked, "nothing is picked before a press");

    let at = foot(&output);
    let mut last = Asked::default();
    for event in crate::view::test_support::click_events(at) {
        let (_, asked) = asked(&ctx, vec![event], &mut library, &mut art);
        last = asked;
    }
    assert!(last.picked, "a press in the drawer closes it");
    assert!(last.command.is_none(), "settings arms no read");
    assert_eq!(library.page(), Some(&Page::Settings));
}
