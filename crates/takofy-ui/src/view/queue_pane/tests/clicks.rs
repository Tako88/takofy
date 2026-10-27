use egui::Modifiers;

use super::super::*;
use super::test_support::*;
use crate::player::Transport;

#[test]
fn a_click_reports_its_absolute_index() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(6, 1);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

    let at = painted_at(&output, "t4").expect("row t4 is painted");
    let reply = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));
    assert_eq!(
        reply.press,
        Some((4, Press::Replace)),
        "the click must report the queue's own index, not the row's position"
    );
    assert_eq!(reply.play, None, "a single click is a press, not a play");
}

#[test]
fn the_first_of_two_identical_entries_reports_its_own_index() {
    // Two entries with the same title and artist: keyed by anything but the
    // index the two rows would share one id, and the later one would win, so
    // clicking the first would report the second.
    let ctx = context();
    let mut queue = Queue::default();
    let same = track(0);
    let tracks = vec![track(1), same.clone(), same];
    queue.start(&tracks, 0, None, &mut Transport::default());

    let mut art = Art::default();
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "t0").expect("the first repeated row is painted");

    assert_eq!(
        click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0)).press,
        Some((1, Press::Replace)),
        "the first of two identical entries is the one that answers"
    );
}

#[test]
fn a_double_click_reports_the_row_to_play() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(6, 1);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "t4").expect("row t4 is painted");

    let reply = click_with(
        &ctx,
        PANE,
        &queue,
        at + Vec2::new(4.0, 4.0),
        Modifiers::NONE,
        2,
    );
    assert_eq!(reply.play, Some(4), "a double click asks for the row");
    assert_eq!(reply.press, None, "a double click is not a press");
}

#[test]
fn a_double_click_starts_an_armed_queue() {
    let ctx = context();
    let mut art = Art::default();
    let mut armed = Queue::default();
    assert_eq!(armed.append((0..3).map(track).collect()), Some(0..3));
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &armed, &mut art, now, Vec::new());
    let at = painted_at(&output, "t1").expect("an armed row is painted");

    let reply = click_with(
        &ctx,
        PANE,
        &armed,
        at + Vec2::new(4.0, 4.0),
        Modifiers::NONE,
        2,
    );
    assert_eq!(
        reply.play,
        Some(1),
        "a double click picks the row out of the waiting list"
    );
    assert_eq!(reply.press, None, "a double click is not a press");
}

#[test]
fn an_armed_row_is_pickable_without_starting_the_queue() {
    let ctx = context();
    let mut art = Art::default();
    let mut armed = Queue::default();
    assert_eq!(armed.append((0..3).map(track).collect()), Some(0..3));
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &armed, &mut art, now, Vec::new());
    let at = painted_at(&output, "t2").expect("an armed row is painted");

    let reply = click(&ctx, PANE, &armed, at + Vec2::new(4.0, 4.0));
    assert_eq!(reply.press, Some((2, Press::Replace)));
    assert_eq!(reply.play, None, "one click only picks the row");
}

#[test]
fn shift_and_ctrl_clicks_report_their_press() {
    for (modifiers, press) in [
        (
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            Press::Extend,
        ),
        (
            Modifiers {
                command: true,
                ..Modifiers::NONE
            },
            Press::Toggle,
        ),
    ] {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 0);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let at = painted_at(&output, "t4").expect("row t4 is painted");

        assert_eq!(
            click_with(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0), modifiers, 1).press,
            Some((4, press)),
            "{modifiers:?}"
        );
    }
}
