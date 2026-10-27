use super::super::*;
use super::test_support::*;
use egui::{Event, Modifiers};

/// The label of the node the frame says has the keyboard.
fn focused_label(output: &egui::FullOutput) -> Option<String> {
    let update = output.platform_output.accesskit_update.as_ref()?;
    update
        .nodes
        .iter()
        .find(|(id, _)| *id == update.focus)
        .and_then(|(_, node)| node.label().map(str::to_owned))
}

/// The last button of a form's extras, which is fullscreen in both.
fn last_extra(rect: Rect, form: Playbar) -> Rect {
    let [_, _, extras] = layout::playbar_for(rect, form).columns(rect);
    Rect::from_min_size(
        Pos2::new(
            extras.right() - extent::BUTTON,
            extras.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    )
}

fn fullscreen_run_in(form: Playbar, bar: Bar) -> Run {
    *extras_runs(form, bar, &Volume::default())
        .iter()
        .find(|run| run.control == Control::Fullscreen)
        .expect("both forms draw the fullscreen button")
}

#[test]
fn fullscreen_labels_name_the_state_and_are_distinct() {
    let labels = [None, Some(false), Some(true)].map(|state| fullscreen_run(state).label);
    for label in labels {
        assert!(
            label.starts_with("Full screen"),
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
fn the_fullscreen_run_is_lit_only_when_the_window_is() {
    assert!(fullscreen_run(Some(true)).lit);
    for state in [None, Some(false)] {
        assert!(!fullscreen_run(state).lit, "{state:?} lit the button");
    }
    for form in [Playbar::Full, Playbar::Reduced] {
        let bar = Bar {
            fullscreen: Some(true),
            ..Bar::default()
        };
        assert!(fullscreen_run_in(form, bar).lit, "{form:?}");
    }
}

#[test]
fn fullscreen_is_live_only_when_the_window_state_is_known() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = queue_of(4);

    let live = |ctx: &egui::Context, queue: &mut Queue, fullscreen| {
        let bar = Bar {
            fullscreen,
            ..Bar::default()
        };
        let (_, output) = queue_frame(ctx, &tokens, queue, Playbar::Full, bar, Vec::new());
        let label = fullscreen_run(fullscreen).label;
        !announced_disabled(&output, label)
    };

    assert!(
        live(&ctx, &mut queue, Some(false)),
        "a windowed window left the fullscreen button inert"
    );
    assert!(
        live(&ctx, &mut queue, Some(true)),
        "a fullscreen window left the button inert"
    );
    assert!(
        !live(&ctx, &mut queue, None),
        "a button with no state to act on still acted"
    );
}

#[test]
fn a_click_on_fullscreen_reports_a_press_only_when_the_state_is_known() {
    for (fullscreen, pressed) in [(Some(false), true), (Some(true), true), (None, false)] {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let bar = Bar {
            fullscreen,
            ..Bar::default()
        };
        let at = last_extra(heart_bar(), Playbar::Full).center();

        let replies = click_at(&ctx, &tokens, &mut queue, Playbar::Full, bar, at);

        assert_eq!(
            replies.iter().any(|reply| reply.fullscreen),
            pressed,
            "{fullscreen:?}: the click reached the wrong answer: {replies:?}"
        );
        assert!(
            replies.iter().all(|reply| reply.command.is_none()),
            "{fullscreen:?}: a window request is not a command: {replies:?}"
        );
    }
}

#[test]
fn a_held_enter_on_the_fullscreen_button_presses_once() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = queue_of(4);
    let bar = Bar {
        fullscreen: Some(false),
        ..Bar::default()
    };
    let label = fullscreen_run(Some(false)).label;

    let _ = queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
    let mut focused = None;
    for _ in 0..12 {
        let (_, output) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            bar,
            vec![key(Key::Tab, Modifiers::NONE)],
        );
        focused = focused_label(&output);
        if focused.as_deref() == Some(label) {
            break;
        }
    }
    assert_eq!(
        focused.as_deref(),
        Some(label),
        "the fullscreen button is not in the tab order"
    );

    let enter = |repeat| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    let mut presses = Vec::new();
    for repeat in [false, true, true] {
        let (reply, _) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            bar,
            vec![enter(repeat)],
        );
        presses.push(reply.fullscreen);
    }

    assert_eq!(
        presses,
        vec![true, false, false],
        "a held Enter asked for fullscreen more than once"
    );
}

#[test]
fn a_lit_fullscreen_paints_the_accent_and_an_unlit_one_does_not() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let at = last_extra(heart_bar(), Playbar::Full);

    for (fullscreen, lit) in [(Some(true), true), (Some(false), false), (None, false)] {
        let bar = Bar {
            fullscreen,
            ..Bar::default()
        };
        let (_, output) = queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
        assert_eq!(
            accent_inside(&output, &tokens, at),
            lit,
            "{fullscreen:?} drew the accent {lit}"
        );
    }
}
