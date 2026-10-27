use egui::{Pos2, RawInput, Shape};

use super::*;

const WIDTH: f32 = 600.0;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::theme::fonts::definitions());
    ctx.set_global_style(crate::theme::style(&Tokens::DARK));
    ctx.enable_accesskit();
    ctx
}

/// The second pass of a head drawn at `WIDTH`, with the cursor it left.
fn draw(title: &str, note: &str) -> (egui::FullOutput, f32) {
    let ctx = context();
    let mut out = None;
    for _ in 0..2 {
        let mut end = 0.0;
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, 400.0))),
                ..Default::default()
            },
            |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                show(ui, title, note, &Tokens::DARK);
                end = ui.cursor().top();
            },
        );
        output.textures_delta.clear();
        out = Some((output, end));
    }
    out.expect("two passes")
}

/// Every text run painted, as it reads, with the box it was painted into.
fn lines(output: &egui::FullOutput) -> Vec<(String, Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => text.galley.rows.first().map(|row| {
                (
                    row.row.text().to_owned(),
                    Rect::from_min_size(text.pos, text.galley.size()),
                )
            }),
            _ => None,
        })
        .collect()
}

fn labels(output: &egui::FullOutput) -> Vec<String> {
    let update = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accesskit is on");
    update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Label)
        .filter_map(|(_, node)| node.value().map(str::to_owned))
        .collect()
}

#[test]
fn the_title_sits_the_reference_air_below_the_page_top() {
    // The reference's `.page-head` carries `padding-top: var(--space-5)`, and
    // nothing above it may add to that: a page's own inset would push the
    // head down and, worse, push a band down with it.
    let (output, _) = draw("Your Library", "18 saved");
    let title = lines(&output)
        .into_iter()
        .find(|(line, _)| line == "Your Library")
        .expect("the title")
        .1;
    assert!(
        (title.top() - space::S5).abs() < 0.01,
        "the title sits at {} rather than {}",
        title.top(),
        space::S5
    );
}

#[test]
fn the_title_and_the_uppercased_note_are_painted() {
    let (output, _) = draw("Your Library", "18 saved");
    let painted: Vec<String> = lines(&output).into_iter().map(|(line, _)| line).collect();
    assert!(painted.contains(&"Your Library".to_owned()), "{painted:?}");
    assert!(painted.contains(&"18 SAVED".to_owned()), "{painted:?}");
}

#[test]
fn a_long_title_ends_before_the_note() {
    let long = "Results for “a very long query that would run under the note”";
    let (output, _) = draw(long, "Top 20 tracks");
    let runs = lines(&output);
    let note = runs
        .iter()
        .find(|(line, _)| line == "TOP 20 TRACKS")
        .expect("the note")
        .1;
    let title = runs
        .iter()
        .find(|(line, _)| line.starts_with("Results"))
        .expect("the title")
        .1;
    assert!(
        title.right() <= note.left() - space::S4 + 0.5,
        "{title:?} {note:?}"
    );
    assert!(
        runs.iter().all(|(line, _)| line != long),
        "the title elides"
    );
}

#[test]
fn an_empty_note_paints_nothing_and_leaves_the_title_its_width() {
    let (output, _) = draw("Your Library", "");
    let runs = lines(&output);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].0, "Your Library");
}

#[test]
fn one_label_speaks_the_title_and_the_note() {
    let (output, _) = draw("Results for “harbour”", "Top 20 tracks");
    assert_eq!(labels(&output), ["Results for “harbour”, Top 20 tracks"]);
    let (output, _) = draw("Your Library", "");
    assert_eq!(labels(&output), ["Your Library"]);
}

#[test]
fn the_head_closes_with_a_rule_and_a_step_of_air() {
    let (output, end) = draw("Your Library", "18 saved");
    let rules = output
        .shapes
        .iter()
        .filter(|clipped| {
            matches!(&clipped.shape, Shape::Rect(rect)
                if rect.fill == Tokens::DARK.palette.border
                    && (rect.rect.height() - stroke::HAIRLINE).abs() < 1e-3)
        })
        .count();
    assert_eq!(rules, 1);
    let title_h = lines(&output)[0].1.height();
    assert!(
        (end - (space::S3 + space::S3 + title_h + space::S4 + space::S5)).abs() < 1.5,
        "{end}"
    );
}
