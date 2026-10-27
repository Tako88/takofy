use super::super::*;
use super::test_support::*;
use crate::view::test_support::click_events;
use egui::{Event, Modifiers};

/// Clicks `control` over the four passes a click takes, and reports what each
/// one returned.
fn click_control(
    ctx: &egui::Context,
    tokens: &Tokens,
    queue: &mut Queue,
    form: Playbar,
    control: Control,
) -> Vec<Option<Command>> {
    let bar = heart_bar();
    let centre = layout::playbar_for(bar, form).columns(bar)[1];
    let at = transport_slot(centre, control).center();
    let _ = queue_frame(ctx, tokens, queue, form, Bar::default(), Vec::new());
    let mut commands = vec![
        queue_frame(
            ctx,
            tokens,
            queue,
            form,
            Bar::default(),
            vec![Event::PointerMoved(at)],
        )
        .0
        .command,
    ];
    for event in click_events(at) {
        commands.push(
            queue_frame(ctx, tokens, queue, form, Bar::default(), vec![event])
                .0
                .command,
        );
    }
    commands
}

#[test]
fn shuffle_and_repeat_runs_follow_the_queue() {
    let mut queue = queue_of(4);

    let shuffle = down_runs(&queue)[0];
    assert_eq!(shuffle.control, Control::Shuffle);
    assert_eq!(shuffle.icon, Icon::Shuffle);
    assert_eq!(shuffle.label, "Shuffle off");
    assert!(!shuffle.lit);
    assert_eq!(shuffle.badge, None);

    let repeat = up_runs(Playbar::Full, &queue, Bar::default())[1];
    assert_eq!(repeat.control, Control::Repeat);
    assert_eq!(repeat.icon, Icon::Repeat);
    assert_eq!(repeat.label, "Repeat off");
    assert!(!repeat.lit, "off is not the mode's own state");
    assert_eq!(repeat.badge, None);

    queue.toggle_shuffle();
    queue.cycle_repeat();
    let shuffle = down_runs(&queue)[0];
    let repeat = up_runs(Playbar::Full, &queue, Bar::default())[1];
    assert_eq!(shuffle.label, "Shuffle on");
    assert!(shuffle.lit);
    assert_eq!(repeat.label, "Repeat all");
    assert!(repeat.lit);
    assert_eq!(repeat.badge, None);

    queue.cycle_repeat();
    let repeat = up_runs(Playbar::Full, &queue, Bar::default())[1];
    assert_eq!(repeat.label, "Repeat one");
    assert!(repeat.lit, "one is an on state");
    assert_eq!(repeat.badge, Some("1"));
}

/// The label a control's slot announces this frame.
fn announced(ui: &Ui, control: Control) -> String {
    let id = controls::slot_id(ui, control, ());
    ui.ctx()
        .accesskit_node_builder(id, |node| node.label().map(str::to_owned))
        .flatten()
        .unwrap_or_else(|| panic!("{control:?} has no AccessKit node"))
}

/// The two mode buttons' announced labels, drawn into the current `ui`.
fn mode_labels(ui: &mut Ui, tokens: &Tokens, queue: &Queue) -> Vec<String> {
    let can_act = |_| true;
    let pen = || Pen {
        tokens,
        claimed: None,
        volume: None,
        can_act: &can_act,
    };
    let mut x = 0.0;
    let _ = draw_run(ui, pen(), &down_runs(queue), &mut x, 20.0);
    let mut x = 100.0;
    let _ = draw_run(
        ui,
        pen(),
        &up_runs(Playbar::Full, queue, Bar::default()),
        &mut x,
        20.0,
    );
    [Control::Shuffle, Control::Repeat]
        .map(|control| announced(ui, control))
        .to_vec()
}

#[test]
fn the_labels_are_announced_per_state() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = queue_of(4);
    let labels = |queue: &mut Queue| {
        let mut announced = Vec::new();
        run(&ctx, input(Vec::new()), |ui| {
            announced = mode_labels(ui, &tokens, queue);
        });
        announced
    };

    assert_eq!(labels(&mut queue), ["Shuffle off", "Repeat off"]);
    queue.toggle_shuffle();
    assert_eq!(labels(&mut queue), ["Shuffle on", "Repeat off"]);
    queue.cycle_repeat();
    assert_eq!(labels(&mut queue), ["Shuffle on", "Repeat all"]);
    queue.cycle_repeat();
    assert_eq!(labels(&mut queue), ["Shuffle on", "Repeat one"]);
}

/// A mode is how the queue will play, so it can be set before there is
/// anything to play.
#[test]
fn the_modes_are_live_on_an_empty_queue() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = Queue::default();
    assert!(!queue.can_toggle(), "arrange: nothing is loaded");

    let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Shuffle);
    assert!(queue.shuffle(), "shuffle was inert with nothing playing");
    assert!(commands.iter().all(Option::is_none));

    let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Repeat);
    assert_eq!(
        queue.repeat(),
        crate::repeat::Repeat::All,
        "repeat was inert with nothing playing"
    );
    assert!(commands.iter().all(Option::is_none));
}

#[test]
fn a_click_on_shuffle_toggles_the_queue_and_sends_no_command() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    assert!(!queue.shuffle());

    let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Shuffle);

    assert!(queue.shuffle(), "the click did not reach the queue");
    assert!(
        commands.iter().all(Option::is_none),
        "a mode is the window's own state, so it asks for nothing: {commands:?}"
    );
}

#[test]
fn three_clicks_on_repeat_cycle_off_all_one_off() {
    use crate::repeat::Repeat;

    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let mut modes = Vec::new();

    for _ in 0..3 {
        let _ = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Repeat);
        modes.push(queue.repeat());
    }

    assert_eq!(modes, [Repeat::All, Repeat::One, Repeat::Off]);
}

#[test]
fn a_held_enter_on_repeat_and_on_shuffle_changes_state_once() {
    use crate::repeat::Repeat;

    let enter = |repeat| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    let cases = [
        (Control::Shuffle, 1, (true, Repeat::Off)),
        (Control::Repeat, 5, (false, Repeat::All)),
    ];
    for (control, tabs, expected) in cases {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);

        let _ = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            Bar::default(),
            Vec::new(),
        );
        for _ in 0..tabs {
            let _ = queue_frame(
                &ctx,
                &tokens,
                &mut queue,
                Playbar::Full,
                Bar::default(),
                vec![key(Key::Tab, Modifiers::NONE)],
            );
        }

        let mut states = Vec::new();
        for repeat in [false, true, true] {
            let _ = queue_frame(
                &ctx,
                &tokens,
                &mut queue,
                Playbar::Full,
                Bar::default(),
                vec![enter(repeat)],
            );
            states.push((queue.shuffle(), queue.repeat()));
        }

        assert_eq!(
            states, [expected; 3],
            "{control:?}: a held key stepped the mode more than once"
        );
    }
}

#[test]
fn shuffle_is_inert_while_a_block_is_carried_and_repeat_is_not() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    egui::DragAndDrop::set_payload(&ctx, egui::Vec2::ZERO);
    assert!(crate::view::carry::carrying(&ctx));

    let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Shuffle);
    assert!(
        !queue.shuffle(),
        "a shuffle under a drag would refuse the drop"
    );
    assert!(
        commands.iter().all(Option::is_none),
        "an inert button asks for nothing: {commands:?}"
    );

    let _ = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Repeat);
    assert_eq!(
        queue.repeat(),
        crate::repeat::Repeat::All,
        "repeat moves no row"
    );
}

#[test]
fn the_reduced_bar_keeps_both_live() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);

    let _ = click_control(
        &ctx,
        &tokens,
        &mut queue,
        Playbar::Reduced,
        Control::Shuffle,
    );
    assert!(queue.shuffle(), "the reduced bar shed shuffle");

    let _ = click_control(&ctx, &tokens, &mut queue, Playbar::Reduced, Control::Repeat);
    assert_eq!(
        queue.repeat(),
        crate::repeat::Repeat::All,
        "the reduced bar shed repeat"
    );
}

#[test]
fn a_lit_shuffle_paints_the_accent_and_an_unlit_one_does_not() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let bar = heart_bar();
    let centre = layout::playbar_for(bar, Playbar::Full).columns(bar)[1];
    let shuffle = transport_slot(centre, Control::Shuffle);

    let (_, output) = queue_frame(
        &ctx,
        &tokens,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        Vec::new(),
    );
    assert!(
        !accent_inside(&output, &tokens, shuffle),
        "an off shuffle took the accent"
    );

    queue.toggle_shuffle();
    let (_, output) = queue_frame(
        &ctx,
        &tokens,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        Vec::new(),
    );
    assert!(
        accent_inside(&output, &tokens, shuffle),
        "an on shuffle did not take the accent"
    );
}

#[test]
fn repeat_one_paints_a_one_and_the_other_states_do_not() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let bar = heart_bar();
    let centre = layout::playbar_for(bar, Playbar::Full).columns(bar)[1];
    let repeat = transport_slot(centre, Control::Repeat);

    for _ in 0..2 {
        let (_, output) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            Bar::default(),
            Vec::new(),
        );
        assert!(
            !text_inside(&output, repeat).contains(&"1".to_owned()),
            "{:?} carried the badge",
            queue.repeat()
        );
        queue.cycle_repeat();
    }

    let (_, output) = queue_frame(
        &ctx,
        &tokens,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        Vec::new(),
    );
    assert_eq!(queue.repeat(), crate::repeat::Repeat::One);
    assert!(
        text_inside(&output, repeat).contains(&"1".to_owned()),
        "repeat one carries no badge"
    );
}
