use super::*;
use crate::layout::Table;
use crate::theme::{fonts, style};
use egui::{Context, Event, RawInput};

fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

fn frame(ctx: &Context, draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 800.0))),
        events: Vec::<Event>::new(),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
    output
}

/// Every rect the frame filled with the placeholder's own ink.
fn bars(output: &egui::FullOutput, tokens: &Tokens) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == tokens.palette.skeleton_rule => {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect()
}

fn fills(output: &egui::FullOutput, tokens: &Tokens) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == tokens.palette.skeleton => {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_placeholder_wakes_on_the_animation_clock() {
    let frames = WAKE.as_secs_f64() / f64::from(eq::FRAME);
    assert!(
        (frames - 2.0).abs() < 0.001,
        "a wake covers {frames} frames of the animation clock"
    );
}

#[test]
fn the_sheen_crosses_once_per_cycle_and_waits_for_its_delay() {
    assert_eq!(sheen(0.0, 0.0), 0.0);
    assert!((sheen(motion::SHEEN as f64 / 2.0, 0.0) - 0.5).abs() < 0.01);
    assert!((sheen(motion::SHEEN as f64 * 1.5, 0.0) - 0.5).abs() < 0.01);
    assert_eq!(sheen(motion::SHEEN as f64, 0.0), 0.0, "the cycle restarts");
    assert_eq!(
        sheen(0.05, motion::STAGGER),
        0.0,
        "a staggered placeholder holds still until its turn"
    );
}

#[test]
fn one_row_of_placeholders_sets_off_four_apart() {
    let delays: Vec<f32> = (0..5).map(stagger).collect();
    assert_eq!(
        delays,
        vec![
            0.0,
            motion::STAGGER,
            motion::STAGGER * 2.0,
            motion::STAGGER * 3.0,
            0.0
        ]
    );
}

#[test]
fn a_card_varies_its_lines_with_position_not_with_content() {
    // The reference's cascade: `3n` widens, then `2n` takes over, so a sixth
    // card is narrow even though it is also a third.
    assert_eq!(card_lines(0), (0.60, 0.44));
    assert_eq!(card_lines(1), (0.54, 0.34));
    assert_eq!(card_lines(2), (0.78, 0.46));
    assert_eq!(card_lines(5), (0.54, 0.34));
}

#[test]
fn a_row_of_placeholders_cycles_six_title_widths() {
    let widths: Vec<f32> = (0..7).map(row_title).collect();
    assert_eq!(widths, vec![0.52, 0.41, 0.63, 0.34, 0.47, 0.58, 0.52]);
}

#[test]
fn a_list_holds_open_what_it_knows_and_a_screenful_when_it_does_not() {
    assert_eq!(row_count(None), SCREENFUL);
    assert_eq!(row_count(Some(0)), 1, "an empty list still holds one open");
    assert_eq!(row_count(Some(12)), 12);
    assert_eq!(row_count(Some(5_000)), MAX_ROWS);
}

#[test]
fn a_loading_tracklist_reserves_one_row_height_per_row() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let mut height = 0.0;
    let output = frame(&ctx, |ui| {
        rows(ui, 3, Table::Full.columns(), true, &tokens);
        height = ui.min_rect().height();
    });
    assert_eq!(height, 3.0 * extent::ROW_H);
    // Index, title, artist, album, added and duration. The like and the
    // actions carry no placeholder, and the title is one line on every
    // shape now that no row stacks a credit under its name.
    assert_eq!(bars(&output, &tokens).len(), 3 * 6);
}

#[test]
fn a_loading_tracklist_gives_up_the_columns_the_table_has_dropped() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let output = frame(&ctx, |ui| {
        rows(ui, 2, Table::Narrow.columns(), true, &tokens);
    });
    // Index, title, the artist and the duration: the album, the date and
    // the like are not on this table at all, and a dropped column takes its
    // bar with it.
    assert_eq!(bars(&output, &tokens).len(), 2 * 4);
}

#[test]
fn a_loading_plays_list_holds_four_bars_a_row() {
    let tokens = Tokens::DARK;
    let ctx = context();
    // A plays row keeps the artist and drops the album and the date. Its
    // like and actions columns are real columns, but the skeleton draws no
    // placeholder into them, so the bars are the index, the title, the
    // artist and the duration.
    let present = Table::Full.columns().for_plays();
    let output = frame(&ctx, |ui| {
        rows(ui, 2, present, false, &tokens);
    });
    assert_eq!(bars(&output, &tokens).len(), 2 * 4);
}

#[test]
fn a_loading_rows_bars_all_sit_on_the_rows_centre_line() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let output = frame(&ctx, |ui| {
        rows(ui, 2, Table::Full.columns(), true, &tokens);
    });
    // Six bars a row on the full playlist table, and no bar below the title
    // line: the second, stacked line is gone.
    for (index, bar) in bars(&output, &tokens).iter().enumerate() {
        let row = index / 6;
        let centre = (row as f32 + 0.5) * extent::ROW_H;
        assert!(
            (bar.center().y - centre).abs() < 0.01,
            "bar {index} of row {row} sits at {} not {centre}",
            bar.center().y
        );
    }
}

#[test]
fn the_title_bar_starts_after_the_art_box_only_when_the_table_draws_one() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let present = Table::Full.columns().for_plays();
    let title_left = |art: bool| {
        let output = frame(&ctx, |ui| {
            rows(ui, 1, present, art, &tokens);
        });
        let bars = bars(&output, &tokens);
        // The index bar is first and the title bar second.
        bars[1].left()
    };

    let columns = table::columns(900.0, present);
    let title = columns.title.left();
    assert_eq!(
        title_left(false),
        title,
        "with no art box the name holds the whole title"
    );
    assert_eq!(
        title_left(true),
        title + extent::LIBRARY_ART + space::S3,
        "an art box pushes the name past its box and gutter"
    );
}

#[test]
fn a_loading_card_reserves_its_cover_and_two_lines() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let output = frame(&ctx, |ui| {
        let solved = cards::grid(
            Rect::from_min_size(ui.cursor().min, vec2(600.0, 0.0)),
            extent::MEDIA_MIN_W,
            space::S3,
            media::extra_height(ui),
        );
        tiles(ui, &solved, 4, &tokens);
    });
    assert_eq!(fills(&output, &tokens).len(), 4, "one cover box each");
    assert_eq!(
        bars(&output, &tokens).len(),
        4 * 2,
        "a name and a subline each"
    );
}

#[test]
fn a_loading_show_reserves_an_episode_row_each() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let mut height = 0.0;
    let output = frame(&ctx, |ui| {
        episode_rows(ui, 2, &tokens);
        height = ui.min_rect().height();
    });
    assert_eq!(height, 2.0 * episode::ROW_H);
    assert_eq!(fills(&output, &tokens).len(), 2, "one toggle each");
    assert_eq!(
        bars(&output, &tokens).len(),
        2 * 3,
        "a title, a subline and a length"
    );
}
