use super::super::*;
use super::test_support::*;
use egui::Pos2;

use crate::theme::{extent, space};
use crate::view::test_support::click_events;

/// The centre of one row's play toggle, read from where its title was painted.
fn toggle_centre(output: &egui::FullOutput, title: &str) -> Pos2 {
    let title = text_rect(output, title).expect("the row's title is painted");
    Pos2::new(
        title.left() - space::S4 - extent::EPISODE_TOGGLE / 2.0,
        title.top() + extent::EPISODE_TOGGLE / 2.0,
    )
}

#[test]
fn a_click_on_a_sorted_row_plays_that_episode() {
    let ctx = context();
    // Newest first, Episode 1 is the list's second row while it is the
    // wire's first: a draw over the wire's order would play Episode 2.
    let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
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
    let sorted = text_rect(&output, "Episode 1").expect("the sorted row is drawn");
    let leading = text_rect(&output, "Episode 2").expect("the newest row is drawn");
    assert!(
        leading.top() < sorted.top(),
        "the sorted row is not the first one"
    );

    let command = click(
        &ctx,
        toggle_centre(&output, "Episode 1"),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:episode:1".to_owned()
        }),
        "the toggle under the click plays the row the page drew there"
    );
}

#[test]
fn a_lift_on_a_sorted_row_carries_that_episode() {
    let ctx = context();
    let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
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
    let row = text_rect(&output, "Episode 1").expect("the sorted row is drawn");
    let mut events = crate::view::test_support::drag_events(
        row.center(),
        row.center() - Vec2::new(0.0, extent::EPISODE_ROW_H),
    );
    // Stop with the button still down: the lift is what this reads.
    events.pop();
    let mut carried = None;
    for event in events {
        let _ = frame(
            &ctx,
            vec![event],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        carried = carried.or_else(|| crate::view::carry::carried(&ctx));
    }
    let carried = carried.expect("the drag lifts the row");
    assert_eq!(
        carried.rows,
        vec![1],
        "the sorted row is the one that travels"
    );
    let uris: Vec<&str> = carried
        .entries
        .iter()
        .map(|entry| entry.uri.as_str())
        .collect();
    assert_eq!(
        uris,
        ["spotify:episode:1"],
        "the carried entry is the row's own episode"
    );
}

#[test]
fn a_row_toggle_click_returns_play_for_that_episode() {
    let ctx = context();
    let mut library = library_with(detail(vec![episode(1), episode(2)]));
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
    let centre = toggle_centre(&output, "Episode 2");
    let command = click(
        &ctx,
        centre,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:episode:2".to_owned()
        }),
        "the toggle under the click is what plays"
    );
}

#[test]
fn a_single_click_selects_and_a_double_click_plays_it() {
    let ctx = context();
    let mut library = library_with(detail(vec![episode(1), episode(2)]));
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
    let row = text_rect(&output, "Episode 2").expect("the row's title");

    let command = click(
        &ctx,
        row.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(command, None, "a single click plays nothing");
    assert!(transport.now_playing.is_none(), "and starts nothing");
    assert!(
        crate::view::carry::List::open(&ctx, egui::Id::unique(("show-episodes", SHOW)), 2)
            .look(1)
            .selected,
        "the row under the click is picked"
    );
    assert!(
        !crate::view::carry::List::open(&ctx, egui::Id::unique(("show-episodes", SHOW)), 2)
            .look(0)
            .selected,
        "and only that row"
    );

    let mut command = None;
    // A fresh page for the double click: a third click inside the window is
    // a triple click, and that plays nothing.
    let ctx = context();
    let mut library = library_with(detail(vec![episode(1), episode(2)]));
    let mut transport = Transport::default();
    for _ in 0..2 {
        for event in click_events(row.center()) {
            command = frame(
                &ctx,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .0;
        }
    }

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:episode:2".to_owned()
        }),
        "a double click plays the episode"
    );
}
