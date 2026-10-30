use egui::{Context, Event, Pos2, RawInput, Shape, Ui, Vec2, vec2};

use super::*;
use crate::selection::Press;
use crate::theme::{fonts, style};
use crate::view::test_support::{rings_in, tab};

const PROBE: Rect = Rect {
    min: Pos2::new(10.0, 10.0),
    max: Pos2::new(610.0, 130.0),
};

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 400.0))),
        events,
        ..Default::default()
    }
}

fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
}

fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

fn row<'a>(title: &'a str) -> Row<'a> {
    Row {
        uri: "spotify:episode:a",
        index: 0,
        title,
        description: None,
        show: "",
        show_uri: None,
        age: "",
        duration: None,
        playable: true,
        state: State::Resting,
        look: Look::default(),
        refocus: false,
    }
}

const SHOW_URI: &str = "spotify:show:a";

/// A row whose byline names a show that has a page.
fn linked<'a>(show: &'a str, age: &'a str) -> Row<'a> {
    Row {
        show,
        show_uri: Some(SHOW_URI),
        age,
        ..row("A Title")
    }
}

/// The box the words of `label` were painted in.
fn words(output: &egui::FullOutput, label: &str) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label} is painted"))
}

/// The ink a pass painted `label` in.
fn ink(output: &egui::FullOutput, label: &str) -> egui::Color32 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == label => Some(text.fallback_color),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label} is painted"))
}

/// Every label a pass gave a link node.
fn link_labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    (node.role() == egui::accesskit::Role::Link)
                        .then(|| node.label().map(str::to_owned))
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Draws one row for a single pass.
fn output_of(ctx: &Context, row: &Row<'_>, events: Vec<Event>) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(events), |ui| {
        let _ = show(ui, PROBE, row, &tokens);
    });
    output.textures_delta.clear();
    output
}

/// Feeds `events` one pass each after a warm-up, returning what the row
/// answered in every pass.
fn feed(ctx: &Context, row: &Row<'_>, events: Vec<Event>) -> Vec<Option<Click>> {
    let tokens = Tokens::DARK;
    let _ = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, row, &tokens);
    });
    let mut answers = Vec::new();
    for event in events {
        let mut answer = None;
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            answer = show(ui, PROBE, row, &tokens);
        });
        output.textures_delta.clear();
        answers.push(answer);
    }
    answers
}

fn key(key: egui::Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Runs one pass with the focus frame opened, returning what it painted.
fn paint(ctx: &Context, input: RawInput, mut draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        draw(ui);
    });
    output.textures_delta.clear();
    output
}

#[test]
fn a_clicked_episode_row_wears_no_ring() {
    let ctx = context();
    let row = row("A Title");
    let tokens = Tokens::DARK;
    let at = Pos2::new(300.0, 40.0);
    let _ = paint(&ctx, input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, &row, &tokens);
    });
    for event in crate::view::test_support::click_events(at) {
        let _ = paint(&ctx, input(vec![event]), |ui| {
            let _ = show(ui, PROBE, &row, &tokens);
        });
    }
    let output = paint(&ctx, input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, &row, &tokens);
    });
    assert!(
        ctx.memory(|memory| memory.focused()).is_some(),
        "the click took the keyboard"
    );
    assert!(
        rings_in(&output).is_empty(),
        "a clicked episode row wears no ring: {:?}",
        rings_in(&output)
    );
}

#[test]
fn a_tabbed_episode_ring_is_not_covered_by_its_hairline() {
    let ctx = context();
    let row = row("A Title");
    let tokens = Tokens::DARK;
    let _ = paint(&ctx, input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, &row, &tokens);
    });
    let [press, release] = tab();
    let _ = paint(&ctx, input(vec![press]), |ui| {
        let _ = show(ui, PROBE, &row, &tokens);
    });
    let output = paint(&ctx, input(vec![release]), |ui| {
        let _ = show(ui, PROBE, &row, &tokens);
    });

    let rings = rings_in(&output);
    assert_eq!(rings.len(), 1, "Tab rings the row: {rings:?}");
    assert_eq!(
        rings[0].bottom(),
        PROBE.bottom(),
        "the ring lies on the row's own bottom edge"
    );

    let shapes = || output.shapes.iter().map(|clipped| &clipped.shape);
    let border = shapes()
        .position(|shape| {
            matches!(shape, Shape::Rect(rect)
                if (rect.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (rect.rect.bottom() - PROBE.bottom()).abs() < 0.01
                    && rect.fill == tokens.palette.border)
        })
        .expect("the row paints its closing hairline");
    let ring = shapes()
        .position(|shape| {
            matches!(shape, Shape::Rect(rect)
                if rect.stroke.width > 0.0 && rect.stroke.color == tokens.palette.accent)
        })
        .expect("the tabbed row wears its ring");
    assert!(
        ring > border,
        "the hairline was painted over the ring's bottom edge"
    );
}

#[test]
fn a_toggle_click_returns_play() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = row("A Title");
    let centre = PROBE.min
        + vec2(
            space::S3 + extent::EPISODE_TOGGLE / 2.0,
            space::S4 + extent::EPISODE_TOGGLE / 2.0,
        );
    let mut played = false;
    for event in crate::view::test_support::click_events(centre) {
        let played = &mut played;
        run(&ctx, input(vec![event]), |ui| {
            if show(ui, PROBE, &row, &tokens) == Some(Click::Play) {
                *played = true;
            }
        });
    }
    assert!(played, "the toggle must ask to play");
}

/// Enter is the row's key. A space belongs to the transport and reaches no
/// row, which is what keeps a focused row from starting a track.
#[test]
fn keyboard_activation_returns_play() {
    for pressed in [egui::Key::Enter, egui::Key::Space] {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row("A Title");
        // Warm up the focus traversal, then Tab onto the row.
        for events in [Vec::new(), vec![key(egui::Key::Tab)]] {
            run(&ctx, input(events), |ui| {
                let _ = show(ui, PROBE, &row, &tokens);
            });
        }
        let mut played = false;
        run(&ctx, input(vec![key(pressed)]), |ui| {
            played = show(ui, PROBE, &row, &tokens) == Some(Click::Play);
        });
        assert_eq!(
            played,
            pressed == egui::Key::Enter,
            "{pressed:?} on the focused row"
        );
    }
}

#[test]
fn a_single_click_on_the_text_does_not_play() {
    // Scanning the list with the pointer must not start an episode: only the
    // toggle and a double click do.
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = row("A Title");
    let mut answers = Vec::new();
    for event in crate::view::test_support::click_events(Pos2::new(300.0, 40.0)) {
        let answers = &mut answers;
        run(&ctx, input(vec![event]), |ui| {
            if let Some(click) = show(ui, PROBE, &row, &tokens) {
                answers.push(click);
            }
        });
    }
    assert!(
        !answers.contains(&Click::Play),
        "one click on the row is not a play"
    );
    assert_eq!(
        answers,
        vec![Click::Select(Press::Replace)],
        "one click on the row is a press"
    );
}

#[test]
fn a_double_click_plays() {
    let ctx = context();
    let row = row("A Title");
    run(&ctx, input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, &row, &Tokens::DARK);
    });
    let mut played = false;
    for _ in 0..2 {
        for event in crate::view::test_support::click_events(Pos2::new(300.0, 40.0)) {
            let played = &mut played;
            run(&ctx, input(vec![event]), |ui| {
                if show(ui, PROBE, &row, &Tokens::DARK) == Some(Click::Play) {
                    *played = true;
                }
            });
        }
    }
    assert!(played, "a double click must play the episode");
}

#[test]
fn shift_and_ctrl_clicks_carry_their_press() {
    for (modifiers, press) in [
        (
            egui::Modifiers {
                shift: true,
                ..egui::Modifiers::NONE
            },
            Press::Extend,
        ),
        (
            egui::Modifiers {
                command: true,
                ..egui::Modifiers::NONE
            },
            Press::Toggle,
        ),
    ] {
        let ctx = context();
        let row = row("A Title");
        run(&ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row, &Tokens::DARK);
        });
        let mut answers = Vec::new();
        let mut events = vec![Event::ModifiersChanged(modifiers)];
        events.extend(crate::view::test_support::click_events(Pos2::new(
            300.0, 40.0,
        )));
        for event in events {
            let answers = &mut answers;
            run(&ctx, input(vec![event]), |ui| {
                if let Some(click) = show(ui, PROBE, &row, &Tokens::DARK) {
                    answers.push(click);
                }
            });
        }
        assert_eq!(answers, vec![Click::Select(press)], "{modifiers:?}");
    }
}

#[test]
fn a_seven_point_drag_lifts_the_row_and_a_three_point_twitch_does_not() {
    for (travel, lifts) in [(3.0, false), (7.0, true)] {
        let ctx = context();
        let row = row("A Title");
        run(&ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row, &Tokens::DARK);
        });
        let start = Pos2::new(300.0, 40.0);
        let end = start + Vec2::new(travel, 0.0);
        let mut lifted = false;
        for event in [
            Event::PointerMoved(start),
            crate::view::test_support::button(start, true),
            Event::PointerMoved(end),
            crate::view::test_support::button(end, false),
        ] {
            let lifted = &mut lifted;
            run(&ctx, input(vec![event]), |ui| {
                if show(ui, PROBE, &row, &Tokens::DARK) == Some(Click::Lift) {
                    *lifted = true;
                }
            });
        }
        assert_eq!(lifted, lifts, "a {travel}-point move");
    }
}

#[test]
fn a_lifted_row_reports_a_lift_once() {
    let ctx = context();
    let row = row("A Title");
    run(&ctx, input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, &row, &Tokens::DARK);
    });

    let start = Pos2::new(300.0, 40.0);
    let end = start + Vec2::new(10.0, 0.0);
    let mut lifts = 0;
    for events in [
        vec![Event::PointerMoved(start)],
        vec![crate::view::test_support::button(start, true)],
        vec![Event::PointerMoved(end)],
        vec![Event::PointerMoved(end + Vec2::new(2.0, 0.0))],
        vec![Event::PointerMoved(end + Vec2::new(4.0, 0.0))],
    ] {
        let lifts = &mut lifts;
        run(&ctx, input(events), |ui| {
            if show(ui, PROBE, &row, &Tokens::DARK) == Some(Click::Lift) {
                *lifts += 1;
            }
        });
    }
    assert_eq!(lifts, 1, "a held drag lifts once, not every frame");
}

#[test]
fn the_row_does_not_reveal_or_fade_while_carrying() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = Row {
        look: Look {
            carrying: true,
            ..Look::default()
        },
        ..row("A Title")
    };
    let at = Pos2::new(300.0, 40.0);
    let mut washed = false;
    for _ in 0..30 {
        let mut output = ctx.run_ui(input(vec![Event::PointerMoved(at)]), |ui| {
            let _ = show(ui, PROBE, &row, &tokens);
        });
        output.textures_delta.clear();
        washed |= output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, Shape::Rect(shape) if shape.fill == tokens.palette.row_hover)
        });
    }
    assert!(!washed, "nothing hovers while a block is carried");
}

#[test]
fn a_hovered_row_washes_with_row_hover() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = row("A Title");
    let at = Pos2::new(300.0, 40.0);
    let mut washed = false;
    for _ in 0..30 {
        let mut output = ctx.run_ui(input(vec![Event::PointerMoved(at)]), |ui| {
            let _ = show(ui, PROBE, &row, &tokens);
        });
        output.textures_delta.clear();
        washed |= output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, Shape::Rect(shape) if shape.fill == tokens.palette.row_hover)
        });
    }
    assert!(washed, "a hovered row must paint the row hover wash");
}

#[test]
fn the_ghost_is_a_queue_row_with_no_cover() {
    let row = Row {
        description: Some("A long way home."),
        show: "Field Notes",
        age: "3 days ago",
        duration: Some("46 min"),
        ..row("A Title")
    };
    let ghost = row.lifted();
    assert_eq!(
        ghost,
        queue_row::Lifted::new(
            "A Title".to_owned(),
            "Field Notes · 3 days ago".to_owned(),
            Some("46 min".to_owned()),
            Cover::Missing
        )
    );
}

#[test]
fn the_title_the_description_and_the_meta_are_painted() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = Row {
        description: Some("A long way home."),
        show: "Field Notes",
        age: "3 days ago",
        duration: Some("46 min"),
        ..row("A Title")
    };
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = show(ui, PROBE, &row, &tokens);
    });
    output.textures_delta.clear();
    let texts = drawn_texts(&output);
    for label in ["A Title", "A long way home."] {
        assert!(
            texts.contains(&label),
            "the row must paint {label}: {texts:?}"
        );
    }
    assert!(
        texts.contains(&"FIELD NOTES · 3 DAYS AGO"),
        "the meta line is upper-cased: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("46 MIN")),
        "the duration is upper-cased: {texts:?}"
    );
}

#[test]
fn the_show_name_is_a_link_to_its_show() {
    let ctx = context();
    ctx.enable_accesskit();
    let output = output_of(&ctx, &linked("Field Notes", "3 days ago"), Vec::new());
    assert_eq!(
        link_labels(&output),
        vec!["Open show FIELD NOTES".to_owned()],
        "the show's name is the row's one link"
    );
}

#[test]
fn the_date_is_plain() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = linked("Field Notes", "3 days ago");
    let output = output_of(&ctx, &row, Vec::new());
    let date = words(&output, " · 3 DAYS AGO");
    assert_eq!(
        ink(&output, " · 3 DAYS AGO"),
        tokens.palette.text_subtle,
        "the date keeps the byline's rest ink"
    );
    let answers = feed(
        &ctx,
        &row,
        crate::view::test_support::click_events(date.center()).to_vec(),
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Open(_))),
        "the date opened a page: {answers:?}"
    );
    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Select(Press::Replace)),
        "the date is the row's own click: {answers:?}"
    );
}

#[test]
fn a_click_on_the_show_returns_open_and_does_not_select() {
    let ctx = context();
    let row = linked("Field Notes", "3 days ago");
    let at = words(&output_of(&ctx, &row, Vec::new()), "FIELD NOTES").center();
    let answers = feed(
        &ctx,
        &row,
        crate::view::test_support::click_events(at).to_vec(),
    );
    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Open(crate::route::Page::Show(SHOW_URI.to_owned()))),
        "a click on the name must open the show: {answers:?}"
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Select(_) | Click::Play)),
        "the name is a link, not the row: {answers:?}"
    );
}

#[test]
fn a_double_click_on_the_show_does_not_play() {
    let ctx = context();
    let row = linked("Field Notes", "3 days ago");
    let at = words(&output_of(&ctx, &row, Vec::new()), "FIELD NOTES").center();
    let mut events = Vec::new();
    for _ in 0..2 {
        events.extend(crate::view::test_support::click_events(at));
    }
    let answers = feed(&ctx, &row, events);
    assert!(
        answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Open(_))),
        "the words never opened their page: {answers:?}"
    );
    assert!(
        !answers.iter().flatten().any(|click| *click == Click::Play),
        "a double click on the words played the episode: {answers:?}"
    );
}

#[test]
fn a_click_beside_the_words_still_selects_and_a_double_click_there_plays() {
    let ctx = context();
    let row = linked("Field Notes", "");
    let show = words(&output_of(&ctx, &row, Vec::new()), "FIELD NOTES");
    let beside = Pos2::new(show.right() + 20.0, show.center().y);
    assert!(
        beside.x < PROBE.right(),
        "the probe must stay inside the row: {beside:?}"
    );

    let single = feed(
        &ctx,
        &row,
        crate::view::test_support::click_events(beside).to_vec(),
    );
    assert_eq!(
        single.iter().flatten().last(),
        Some(&Click::Select(Press::Replace)),
        "a click beside the words must pick the row: {single:?}"
    );

    let mut events = Vec::new();
    for _ in 0..2 {
        events.extend(crate::view::test_support::click_events(beside));
    }
    let double = feed(&ctx, &row, events);
    assert!(
        double.iter().flatten().any(|click| *click == Click::Play),
        "a double click beside the words must play: {double:?}"
    );
}

#[test]
fn no_show_uri_paints_the_byline_as_one_plain_line() {
    let ctx = context();
    ctx.enable_accesskit();
    let row = Row {
        show: "Field Notes",
        show_uri: None,
        age: "3 days ago",
        ..row("A Title")
    };
    let output = output_of(&ctx, &row, Vec::new());
    assert!(
        link_labels(&output).is_empty(),
        "a show with no page drew a link node: {:?}",
        link_labels(&output)
    );
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"FIELD NOTES · 3 DAYS AGO"),
        "the byline is one line: {texts:?}"
    );
    assert!(
        !texts.contains(&"FIELD NOTES"),
        "the show is not a line of its own: {texts:?}"
    );
}

#[test]
fn a_carried_episode_row_has_no_link() {
    let ctx = context();
    ctx.enable_accesskit();
    let row = Row {
        look: Look {
            carrying: true,
            ..Look::default()
        },
        ..linked("Field Notes", "3 days ago")
    };
    let output = output_of(&ctx, &row, Vec::new());
    assert!(
        link_labels(&output).is_empty(),
        "a carried row drew a link node: {:?}",
        link_labels(&output)
    );
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"FIELD NOTES · 3 DAYS AGO"),
        "a carried row keeps the joined byline: {texts:?}"
    );
    let at = words(&output, "FIELD NOTES · 3 DAYS AGO").center();
    let answers = feed(
        &ctx,
        &row,
        crate::view::test_support::click_events(at).to_vec(),
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Open(_))),
        "a carried row opened a page: {answers:?}"
    );
}

#[test]
fn the_date_hangs_one_tracking_step_off_the_show() {
    let ctx = context();
    let output = output_of(&ctx, &linked("Field Notes", "3 days ago"), Vec::new());
    let show = words(&output, "FIELD NOTES");
    let date = words(&output, " · 3 DAYS AGO");
    let spacing = recipe::tracking_pts(text::XXS, tracking::WIDE);
    assert!(
        (date.left() - show.right() - spacing).abs() < 0.01,
        "the date starts {} from the show, not {spacing}",
        date.left() - show.right()
    );
    assert!(
        (date.top() - show.top()).abs() < 0.01,
        "the date sits on another line: {} against {}",
        date.top(),
        show.top()
    );
}

#[test]
fn a_blank_or_dated_half_paints_no_stray_separator() {
    // A blank show drops out of the link path, so the date stands alone; an
    // unknown age paints no dot at all.
    for blank in ["", "   "] {
        let ctx = context();
        let output = output_of(&ctx, &linked(blank, "3 days ago"), Vec::new());
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"3 DAYS AGO"),
            "the date stands alone: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains('·')),
            "a blank show left a stray separator: {texts:?}"
        );
    }

    let ctx = context();
    let output = output_of(&ctx, &linked("Field Notes", ""), Vec::new());
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"FIELD NOTES"),
        "the show is painted: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains('·')),
        "an unknown age left a trailing separator: {texts:?}"
    );
}

#[test]
fn a_drag_from_the_show_still_lifts_the_row() {
    // The link senses a click only; the body owns the drag under it.
    for (travel, lifts) in [(3.0, false), (7.0, true)] {
        let ctx = context();
        let row = linked("Field Notes", "3 days ago");
        let start = words(&output_of(&ctx, &row, Vec::new()), "FIELD NOTES").center();
        let end = start + Vec2::new(travel, 0.0);
        let mut lifted = false;
        for event in [
            Event::PointerMoved(start),
            crate::view::test_support::button(start, true),
            Event::PointerMoved(end),
            crate::view::test_support::button(end, false),
        ] {
            let lifted = &mut lifted;
            run(&ctx, input(vec![event]), |ui| {
                if show(ui, PROBE, &row, &Tokens::DARK) == Some(Click::Lift) {
                    *lifted = true;
                }
            });
        }
        assert_eq!(lifted, lifts, "a {travel}-point move from the show's words");
    }
}

#[test]
fn the_show_elides_and_hides_the_date_when_it_fills_the_width() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let narrow = Rect {
        min: Pos2::new(10.0, 10.0),
        max: Pos2::new(200.0, 130.0),
    };
    let row = linked("Extraordinarilylongshownameofgreatlength", "3 days ago");
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = show(ui, narrow, &row, &tokens);
    });
    output.textures_delta.clear();

    let body = row_parts(narrow, 0.0).body;
    let show = words(&output, "EXTRAORDINARILYLONGSHOWNAMEOFGREATLENGTH");
    assert!(
        show.width() <= body.width() + 0.01,
        "the show {show:?} runs past the body {body:?}"
    );
    assert!(
        show.width() >= body.width() - 10.0,
        "the show {show:?} elides to fill the body {body:?}"
    );
    let painted = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text)
                if text.galley.text() == "EXTRAORDINARILYLONGSHOWNAMEOFGREATLENGTH" =>
            {
                Some(
                    text.galley
                        .rows
                        .iter()
                        .flat_map(|row| row.glyphs.iter().map(|glyph| glyph.chr))
                        .collect::<String>(),
                )
            }
            _ => None,
        });
    assert!(
        painted.as_deref().is_some_and(|text| text.contains('…')),
        "the show elides: {painted:?}"
    );
    let texts = drawn_texts(&output);
    assert!(
        !texts.iter().any(|text| text.contains("3 DAYS AGO")),
        "the date is dropped rather than left a stump: {texts:?}"
    );
}

#[test]
fn a_carried_byline_elides_at_the_same_width_as_a_linked_one() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let narrow = Rect {
        min: Pos2::new(10.0, 10.0),
        max: Pos2::new(200.0, 130.0),
    };
    let carried = Row {
        look: Look {
            carrying: true,
            ..Look::default()
        },
        ..linked("Extraordinarilylongshownameofgreatlength", "3 days ago")
    };
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = show(ui, narrow, &carried, &tokens);
    });
    output.textures_delta.clear();

    let body = row_parts(narrow, 0.0).body;
    let joined = words(
        &output,
        "EXTRAORDINARILYLONGSHOWNAMEOFGREATLENGTH · 3 DAYS AGO",
    );
    assert!(
        joined.width() <= body.width() + 0.01,
        "the carried byline {joined:?} runs past the body {body:?}"
    );
}
