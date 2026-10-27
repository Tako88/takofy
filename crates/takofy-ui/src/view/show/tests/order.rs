use egui::Context;

use super::super::*;
use super::test_support::*;
use crate::theme::extent;
use crate::view::test_support::click_events;

/// The box a chip occupies, recovered from the label it painted: a chip's
/// label sits at the chip's top-left plus its padding.
fn chip_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    let text = text_rect(output, &label.to_uppercase())?;
    Some(text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y)))
}

/// How long the page asked the host to wait before the next frame.
fn repaint_delay(output: &egui::FullOutput) -> std::time::Duration {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay)
}

/// Whether the page's list holds `row` as a pick.
fn picked(ctx: &Context, count: usize, row: usize) -> bool {
    crate::view::carry::List::open(ctx, egui::Id::unique(("show-episodes", SHOW)), count)
        .look(row)
        .selected
}

#[test]
fn newest_first_puts_the_last_episode_first_and_keeps_every_row() {
    let ctx = context();
    // The wire lists the oldest first, so newest-first has to turn it round.
    let mut library = library_with(detail(vec![dated(1, 30), dated(2, 20), dated(3, 10)]));
    library.set_show_order(ShowOrder::Newest);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let top = |label: &str| {
        text_rect(&output, label)
            .unwrap_or_else(|| panic!("{label} is drawn"))
            .top()
    };
    assert!(
        top("Episode 3") < top("Episode 2") && top("Episode 2") < top("Episode 1"),
        "the newest episode leads and the other two follow it"
    );

    let texts = drawn_texts(&output);
    for number in 1..=3 {
        let label = format!("Episode {number}");
        assert_eq!(
            texts.iter().filter(|text| **text == label).count(),
            1,
            "every episode is drawn once: {texts:?}"
        );
    }
}

#[test]
fn a_sort_clears_the_picks_so_the_highlight_does_not_move() {
    let ctx = context();
    let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let row = text_rect(&output, "Episode 1").expect("the wire's first row");
    let command = click(
        &ctx,
        row.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a press picks a row and plays nothing");
    assert!(picked(&ctx, 2, 0), "the press picks the row it landed on");

    let chip = chip_rect(&output, "Oldest first").expect("the chip naming the order drawn");
    let command = click(
        &ctx,
        chip.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a sort is no command");
    assert_eq!(
        library.show_order,
        ShowOrder::Newest,
        "the chip reordered the page"
    );
    assert!(
        !picked(&ctx, 2, 0),
        "the pick on the row that moved is gone"
    );
    assert!(!picked(&ctx, 2, 1), "and no other row took it");
}

#[test]
fn a_chip_click_reorders_without_a_command() {
    let ctx = context();
    // The wire lists the oldest first, so the page opens naming that order.
    let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let oldest = chip_rect(&output, "Oldest first").expect("the chip naming the order drawn");
    let command = click(
        &ctx,
        oldest.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a sort is page state, not a command");
    assert_eq!(
        library.show_order,
        ShowOrder::Newest,
        "the press turned the list round"
    );

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let leading = text_rect(&output, "Episode 2").expect("the newest row");
    let trailing = text_rect(&output, "Episode 1").expect("the oldest row");
    assert!(
        leading.top() < trailing.top(),
        "the page draws the order the chip asked for"
    );
    let named = chip_rect(&output, "Newest first").expect("the chip renamed itself");
    assert!(
        chip_rect(&output, "Oldest first").is_none(),
        "one chip names one order"
    );

    // The chip is the way back: pressing the order it names gives the other.
    let command = click(
        &ctx,
        named.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a sort is page state, not a command");
    assert_eq!(
        library.show_order,
        ShowOrder::Oldest,
        "a second press turns it back"
    );
}

#[test]
fn a_chip_press_draws_the_new_order_without_a_second_event() {
    let ctx = context();
    // The wire lists the newest first, so oldest-first has to turn it round.
    let mut library = library_with(detail(vec![dated(2, 10), dated(1, 20)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let chip = chip_rect(&output, "Newest first").expect("the chip naming the order drawn");

    // The press in full, then nothing at all: the chip's order lands after
    // this frame's draw, so that frame has to be asked for by name.
    let mut command = None;
    let mut pressed = None;
    for event in click_events(chip.center()) {
        let (asked, drawn) = frame(
            &ctx,
            vec![event],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        command = command.or(asked);
        pressed = Some(drawn);
    }
    assert_eq!(command, None, "a sort is page state, not a command");
    let pressed = pressed.expect("the press drew a frame");
    let drew = |output: &egui::FullOutput, label: &str| {
        text_rect(output, label)
            .unwrap_or_else(|| panic!("{label} is drawn"))
            .top()
    };
    assert!(
        drew(&pressed, "Episode 2") < drew(&pressed, "Episode 1"),
        "the press frame itself still draws the order it started with"
    );
    assert_eq!(
        repaint_delay(&pressed),
        std::time::Duration::ZERO,
        "the press asks for the frame that paints the order it took"
    );

    let (_, after) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        drew(&after, "Episode 1") < drew(&after, "Episode 2"),
        "the frame after the press draws the oldest episode first"
    );
}

#[test]
fn pressing_the_chip_reverses_the_list_and_renames_itself() {
    let ctx = context();
    // The wire lists the newest first, so the page opens naming that order
    // and one press has to lead with the episode at the far end of it.
    let mut library = library_with(detail(vec![dated(3, 10), dated(2, 20), dated(1, 30)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let chip = chip_rect(&output, "Newest first").expect("the chip naming the order drawn");
    let command = click(
        &ctx,
        chip.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a sort is page state, not a command");

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let leading = text_rect(&output, "Episode 1").expect("the oldest row");
    let trailing = text_rect(&output, "Episode 3").expect("the newest row");
    assert!(
        leading.top() < trailing.top(),
        "the drawn first row is the episode at the newest one's far end"
    );
    assert!(
        chip_rect(&output, "Oldest first").is_some(),
        "the chip now names the order it turned the page to"
    );
    assert!(
        chip_rect(&output, "Newest first").is_none(),
        "and no longer names the one it left"
    );
}
