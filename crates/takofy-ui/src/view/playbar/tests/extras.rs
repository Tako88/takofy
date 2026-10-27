use super::super::*;
use super::test_support::*;
use egui::RawInput;

#[test]
fn the_drawn_extras_fill_the_width_the_layout_reserves() {
    for form in [Playbar::Full, Playbar::Reduced] {
        assert!(
            (run_width(extras_controls(form)) - layout::playbar_extras(form)).abs() < 1e-3,
            "{form:?}: the view draws {} but the layout reserves {}",
            run_width(extras_controls(form)),
            layout::playbar_extras(form)
        );
    }
}

#[test]
fn the_reduced_bar_sheds_the_two_buttons_the_reference_sheds() {
    let full: Vec<Control> = extras_controls(Playbar::Full)
        .iter()
        .map(|run| run.control)
        .collect();
    let reduced: Vec<Control> = extras_controls(Playbar::Reduced)
        .iter()
        .map(|run| run.control)
        .collect();
    assert!(!reduced.contains(&Control::Queue));
    assert!(!reduced.contains(&Control::Devices));
    assert!(full.contains(&Control::Queue));
    assert!(full.contains(&Control::Devices));
    // Shed from the extras, carried by the transport.
    assert!(
        up_controls(Playbar::Reduced)
            .iter()
            .any(|run| run.control == Control::Queue)
    );
}

#[test]
fn the_queue_control_is_in_exactly_one_run_in_each_form() {
    for form in [Playbar::Full, Playbar::Reduced] {
        let runs = [
            &NOW,
            DOWN.as_slice(),
            up_controls(form),
            extras_controls(form),
        ];
        let count = runs
            .iter()
            .flat_map(|run| run.iter())
            .filter(|run| run.control == Control::Queue)
            .count();
        assert_eq!(count, 1, "{form:?}: the queue must be drawn once");
    }
}

#[test]
fn the_reduced_transport_ends_with_the_queue_and_the_full_one_does_not() {
    assert_eq!(
        up_controls(Playbar::Reduced).last().map(|run| run.control),
        Some(Control::Queue)
    );
    assert!(
        !up_controls(Playbar::Full)
            .iter()
            .any(|run| run.control == Control::Queue)
    );
}

#[test]
fn the_queue_button_and_its_drop_area_fit_at_every_supported_width() {
    for width in 640..=2000 {
        let width = width as f32;
        let rect = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(width, extent::PLAYBAR_H));
        let form = layout::regions_for(width).playbar;
        let [_, centre, extras] = layout::playbar_for(rect, form).columns(rect);
        let column = match form {
            Playbar::Full => extras,
            Playbar::Reduced => centre,
        };

        let button = queue_button(rect, form);
        assert_eq!(button.size(), Vec2::splat(extent::BUTTON), "{width}");
        assert!(
            column.contains(button.min) && column.contains(button.max),
            "{width}: {form:?} draws the queue button at {button:?} outside {column:?}"
        );

        let area = crate::landing::Area::around(button).hit;
        // The reduced transport sits 12pt above the centre of a 64pt bar and
        // the 44pt area reaches 22pt above a 28pt button, so its top laps the
        // bar's edge by exactly 2pt. That is still the panel above it.
        assert!(
            area.left() >= rect.left()
                && area.right() <= rect.right()
                && area.top() >= rect.top() - 2.0
                && area.bottom() <= rect.bottom(),
            "{width}: {form:?} puts the drop area at {area:?} outside {rect:?}"
        );
    }
}

/// The shell builds the queue's drop area around `queue_button`, so that
/// function has to return the rect the bar actually paints.
#[test]
fn the_claimed_queue_button_is_painted_where_queue_button_says() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(900.0, extent::PLAYBAR_H));
    let mut transport = playing(0);

    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 300.0))),
            ..Default::default()
        },
        |ui| {
            let mut queue = Queue::default();
            let _ = show(
                ui,
                &mut transport,
                &mut queue,
                0,
                Playbar::Reduced,
                Bar {
                    queue_claimed: true,
                    ..Bar::default()
                },
                None,
                bar,
                Cover::Missing,
                &tokens,
            );
        },
    );
    output.textures_delta.clear();

    let claimed: Vec<Rect> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.fill == tokens.palette.row_selected => {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(claimed, vec![queue_button(bar, Playbar::Reduced)]);
}

fn controls_of(runs: &[Run]) -> Vec<Control> {
    runs.iter().map(|run| run.control).collect()
}

#[test]
fn the_extras_and_up_runs_keep_the_tables_controls_and_widths() {
    let queue = queue_of(4);
    let bars = [
        Bar::default(),
        Bar {
            pane: PaneButton::Shown,
            ..Bar::default()
        },
    ];
    for form in [Playbar::Full, Playbar::Reduced] {
        let down = down_runs(&queue);
        assert_eq!(controls_of(&down), controls_of(&DOWN), "{form:?}");
        assert_eq!(run_width(&down), run_width(&DOWN), "{form:?}");

        for bar in bars {
            let up = up_runs(form, &queue, bar);
            assert_eq!(controls_of(&up), controls_of(up_controls(form)), "{form:?}");
            assert_eq!(
                run_width(&up),
                run_width(up_controls(form)),
                "{form:?}: the layout reserves what the drawn runs take"
            );

            let extras = extras_runs(form, bar, &Volume::default());
            assert_eq!(
                controls_of(&extras),
                controls_of(extras_controls(form)),
                "{form:?}"
            );
            assert_eq!(
                run_width(&extras),
                run_width(extras_controls(form)),
                "{form:?}: the layout reserves what the drawn runs take"
            );
        }
    }
}

fn pane_run_in(form: Playbar, bar: Bar) -> Run {
    let mut runs = extras_runs(form, bar, &Volume::default());
    runs.extend_from_slice(&up_runs(form, &queue_of(4), bar));
    *runs
        .iter()
        .find(|run| run.control == Control::Queue)
        .expect("the queue is drawn once per form")
}

#[test]
fn pane_labels_name_the_state_and_are_distinct() {
    let labels = [
        PaneButton::Unavailable,
        PaneButton::Shown,
        PaneButton::Hidden,
    ]
    .map(|pane| pane_run(pane).label);
    for label in labels {
        assert!(
            label.starts_with("Queue"),
            "{label} does not name the button"
        );
    }
    let mut unique = labels.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        labels.len(),
        "{labels:?} do not tell the states apart"
    );
}

#[test]
fn the_queue_run_is_lit_only_when_the_pane_is_shown() {
    let lit = |pane| {
        pane_run_in(
            Playbar::Full,
            Bar {
                pane,
                ..Bar::default()
            },
        )
        .lit
    };
    assert!(lit(PaneButton::Shown), "a shown pane left the button unlit");
    assert!(!lit(PaneButton::Hidden), "a hidden pane lit the button");
    assert!(!lit(PaneButton::Unavailable), "the inert button was lit");
}

#[test]
fn queue_is_live_only_when_the_pane_can_toggle_and_nothing_is_carried() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = Queue::default();

    let live = |ctx: &egui::Context, queue: &mut Queue, bar: Bar| {
        let (_, output) = queue_frame(ctx, &tokens, queue, Playbar::Full, bar, Vec::new());
        !announced_disabled(&output, pane_run(bar.pane).label)
    };

    assert!(
        live(
            &ctx,
            &mut queue,
            Bar {
                pane: PaneButton::Shown,
                ..Bar::default()
            }
        ),
        "a shown pane left the queue button inert"
    );
    assert!(
        !live(&ctx, &mut queue, Bar::default()),
        "an unavailable pane button still acted"
    );

    egui::DragAndDrop::set_payload(&ctx, egui::Vec2::ZERO);
    assert!(
        !live(
            &ctx,
            &mut queue,
            Bar {
                pane: PaneButton::Shown,
                ..Bar::default()
            }
        ),
        "a block in flight left the pane switch live"
    );
}
