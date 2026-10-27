use egui::{Event, Modifiers, Pos2};

use super::super::*;
use super::test_support::*;

#[test]
fn the_focused_row_is_reported() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(6, 0);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "t3").expect("row t3 is painted");

    let reply = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));
    assert_eq!(reply.focused, Some(3), "the picked row holds the keyboard");
}

#[test]
fn a_focused_row_scrolled_out_of_view_is_still_reported() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    let mut queue = queue_of(40, 0);
    queue.refocus_at(5);
    let tall = Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), CAPTION_H + 6.0 * ROW_H));
    let short = Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), CAPTION_H + 2.0 * ROW_H));

    let (_, output) = pass(&ctx, tall, &queue, &mut art, now, Vec::new());
    assert!(
        painted_at(&output, "t5").is_some(),
        "the row asked for the keyboard must be drawn first"
    );

    // The pane shrinks until the focused row is below the fold. egui forgets a
    // widget it does not see twice, so the frame it leaves is the one pinned.
    let mut reply = Reply::default();
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        reply = show(ui, short, &queue, &mut art, now, &tokens);
    });
    output.textures_delta.clear();
    assert!(
        painted_rows(&output).iter().all(|row| *row < 5),
        "the focused row must be off screen: {:?}",
        painted_rows(&output)
    );
    assert_eq!(
        reply.focused,
        Some(5),
        "the keyboard stays on the row the pane no longer draws"
    );
}

#[test]
fn enter_on_a_focused_row_reports_the_row_to_play() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(6, 0);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "t4").expect("row t4 is painted");

    // The click leaves the keyboard on the row it picked.
    let pressed = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));
    assert_eq!(pressed.focused, Some(4));

    let enter = Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, vec![enter]);
    assert_eq!(reply.play, Some(4), "Enter plays the focused row");
    assert_eq!(reply.press, None, "Enter is not a press");
}

#[test]
fn a_pending_refocus_takes_the_keyboard() {
    let ctx = context();
    let mut art = Art::default();
    let now = Instant::now();
    let mut queue = queue_of(6, 0);
    queue.refocus_at(3);

    // The frame that draws the row hands it the keyboard.
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    assert!(
        painted_at(&output, "t3").is_some(),
        "the row asked for the keyboard must be drawn"
    );

    let enter = Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, vec![enter]);
    assert_eq!(
        reply.play,
        Some(3),
        "Enter plays the row the edit landed on"
    );
    assert_eq!(
        queue.refocus(),
        Some(3),
        "the pane reports and never mutates the queue"
    );
}

#[test]
fn a_focused_row_reports_the_delete() {
    let ctx = context();
    let queue = queue_of(6, 1);
    let at = row_at(&ctx, &queue, "t4");
    let _ = click(&ctx, PANE, &queue, at);

    let reply = feed_doors(
        &ctx,
        PANE,
        &queue,
        vec![Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        Reply::default(),
    );

    assert_eq!(reply.remove, Some(4));
}

#[test]
fn a_held_delete_reports_once() {
    let ctx = context();
    let queue = queue_of(6, 1);
    let at = row_at(&ctx, &queue, "t4");
    let _ = click(&ctx, PANE, &queue, at);

    let mut reply = feed_doors(
        &ctx,
        PANE,
        &queue,
        vec![Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        Reply::default(),
    );
    assert_eq!(reply.remove, Some(4));

    let mut art = Art::default();
    let tokens = Tokens::DARK;
    for _ in 0..3 {
        art.begin_frame();
        run(
            &ctx,
            input(vec![Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: true,
                modifiers: Modifiers::NONE,
            }]),
            |ui| {
                reply = show(ui, PANE, &queue, &mut art, Instant::now(), &tokens);
            },
        );
        assert_eq!(reply.remove, None, "a held Del reported again");
    }
}
