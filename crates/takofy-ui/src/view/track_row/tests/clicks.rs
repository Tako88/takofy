use egui::{Event, Key, Modifiers};

use super::super::*;
use super::test_support::*;
use crate::route::Page;
use crate::theme::Tokens;
use crate::view::test_support::click_events;

/// The ink a painted line was given, read back from a frame.
fn ink_of(output: &egui::FullOutput, label: &str) -> Color32 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.fallback_color)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label:?} was not painted"))
}

/// A fresh context, the standard row and the box `label` is painted into.
///
/// Each edge probe gets its own context: two clicks within egui's
/// double-click distance of each other are one double click.
fn fresh_words(label: &str) -> (egui::Context, Row<'static>, Rect) {
    let ctx = context();
    let row = row_of(1);
    let words = words(&ctx, &row, label);
    (ctx, row, words)
}

#[test]
fn a_single_click_on_the_body_selects_and_does_not_play() {
    let ctx = context();
    let row = row_of(1);
    let answers = run(&ctx, &row, &click_events(body_point()), Modifiers::NONE);

    assert_eq!(
        answers.iter().flatten().next(),
        Some(&Click::Select(Press::Replace)),
        "a single click picks the row"
    );
    assert!(
        !answers.iter().flatten().any(|click| *click == Click::Play),
        "a single click played: {answers:?}"
    );
}

#[test]
fn a_double_click_plays() {
    let ctx = context();
    let row = row_of(1);
    let point = body_point();
    let mut events = click_events(point).to_vec();
    events.extend(click_events(point));
    let answers = run(&ctx, &row, &events, Modifiers::NONE);

    assert!(
        answers.iter().flatten().any(|click| *click == Click::Play),
        "a double click did not play: {answers:?}"
    );
}

#[test]
fn enter_on_a_focused_row_plays_and_does_not_select() {
    let ctx = context();
    let row = row_of(1);
    let point = body_point();
    // The click is what gives the row the keyboard, and it selects once.
    let first = run(&ctx, &row, &click_events(point), Modifiers::NONE);
    assert!(
        first
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Select(_))),
        "the row never took the keyboard: {first:?}"
    );

    let key = |pressed| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let answers = run(&ctx, &row, &[key(true), key(false)], Modifiers::NONE);

    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Play),
        "Enter on a focused row plays"
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Select(_))),
        "Enter selected the row as well as playing it: {answers:?}"
    );
}

#[test]
fn a_link_click_opens_and_does_not_select() {
    let ctx = context();
    let row = row_of(1);
    let artist = words(&ctx, &row, "Slow Tide").center();
    let answers = run(&ctx, &row, &click_events(artist), Modifiers::NONE);

    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Open(Page::credit("spotify:artist:b"))),
        "a click on the artist cell's words opens it"
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Select(_))),
        "a link click selected the row: {answers:?}"
    );
}

#[test]
fn a_click_on_the_words_of_each_credit_opens_it() {
    let cells = columns().at(body_rect().min);
    let cases = [
        ("Slow Tide", cells.artist, Page::credit("spotify:artist:b")),
        (
            "Record",
            cells.album,
            Page::Album("spotify:album:c".to_owned()),
        ),
    ];
    for (label, cell, page) in cases {
        let (ctx, row, words) = fresh_words(label);
        let at = words.center();
        assert!(
            words.right() < cell.right() && cell.contains(at),
            "{label:?} ends at {} inside its cell, which ends at {}",
            words.right(),
            cell.right()
        );
        let answers = run(&ctx, &row, &click_events(at), Modifiers::NONE);
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Open(page)),
            "a click on {label:?} opens its page: {answers:?}"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "a click on {label:?} selected the row: {answers:?}"
        );
    }
}

#[test]
fn a_click_past_the_words_selects_the_row() {
    let cells = columns().at(body_rect().min);
    for (label, cell) in [("Slow Tide", cells.artist), ("Record", cells.album)] {
        let (ctx, row, words) = fresh_words(label);
        let at = space_after(cell, words);
        assert!(
            words.right() < at.x && at.x < cell.right(),
            "the probe at x={} sits between {label:?}, which ends at {}, and the cell's edge at {}",
            at.x,
            words.right(),
            cell.right()
        );
        let answers = run(&ctx, &row, &click_events(at), Modifiers::NONE);
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Select(Press::Replace)),
            "a click past {label:?} selects the row: {answers:?}"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Open(_))),
            "a click past {label:?} opened its page: {answers:?}"
        );
    }
}

#[test]
fn a_double_click_past_the_words_plays() {
    let cell = columns().at(body_rect().min).album;
    let (ctx, row, words) = fresh_words("Record");
    let at = space_after(cell, words);
    assert!(
        words.right() < at.x && at.x < cell.right(),
        "the probe at x={} must sit past the words at {}",
        at.x,
        words.right()
    );
    let mut events = click_events(at).to_vec();
    events.extend(click_events(at));
    let answers = run(&ctx, &row, &events, Modifiers::NONE);

    assert!(
        answers.iter().flatten().any(|click| *click == Click::Play),
        "a double click past the words does not play: {answers:?}"
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Open(_))),
        "a double click past the words opened the album: {answers:?}"
    );
}

#[test]
fn the_hit_rect_ends_where_the_painted_words_end() {
    let cells = columns().at(body_rect().min);
    let cases = [
        ("Slow Tide", cells.artist, Page::credit("spotify:artist:b")),
        (
            "Record",
            cells.album,
            Page::Album("spotify:album:c".to_owned()),
        ),
    ];
    for (label, cell, page) in cases {
        let (ctx, row, words) = fresh_words(label);
        let inside = Pos2::new(words.right() - 1.0, cell.center().y);
        let answers = run(&ctx, &row, &click_events(inside), Modifiers::NONE);
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Open(page)),
            "a click one point inside {label:?} opens its page: {answers:?}"
        );

        let (ctx, row, words) = fresh_words(label);
        let outside = Pos2::new(words.right() + 2.0, cell.center().y);
        assert!(
            outside.x < cell.right(),
            "the probe at x={} must still be inside the cell, which ends at {}",
            outside.x,
            cell.right()
        );
        let answers = run(&ctx, &row, &click_events(outside), Modifiers::NONE);
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Select(Press::Replace)),
            "a click two points past {label:?} selects the row: {answers:?}"
        );
    }
}

/// A row whose credit elides in a reduced-width table, with the box its
/// visible run occupies.
fn elided_credit() -> (egui::Context, Rect, Columns, Row<'static>, Rect) {
    let width = 700.0;
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(width, crate::theme::extent::ROW_H));
    let columns = crate::layout::table::columns(width, crate::layout::Table::Reduced.columns());
    let artist = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends";
    let row = Row {
        artist,
        ..row_of(1)
    };
    let ctx = context();
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        let _ = show(ui, rect, columns, &row, &Tokens::DARK);
    });
    output.textures_delta.clear();
    let galley = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == artist => {
                Some(text.galley.clone())
            }
            _ => None,
        })
        .expect("the artist column paints its label");
    assert!(galley.elided, "the credit elides");
    assert_eq!(galley.rows.len(), 1, "the credit stays on one line");
    let words = text_rect(&output, artist).expect("the credit's painted box");
    (ctx, rect, columns, row, words)
}

#[test]
fn an_elided_credit_is_a_link_only_over_its_visible_run() {
    let (ctx, rect, columns, row, words) = elided_credit();
    let cell = columns.at(rect.min).artist;
    assert!(
        words.right() <= cell.right() + 0.01,
        "the visible run ends at {} inside a cell ending at {}",
        words.right(),
        cell.right()
    );
    let answers = run_in(
        &ctx,
        rect,
        columns,
        &row,
        &click_events(words.center()),
        Modifiers::NONE,
    );
    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Open(Page::credit("spotify:artist:b"))),
        "a click on the visible run opens the artist: {answers:?}"
    );

    let (ctx, rect, columns, row, words) = elided_credit();
    let past = Pos2::new(words.right() + 2.0, words.center().y);
    let answers = run_in(
        &ctx,
        rect,
        columns,
        &row,
        &click_events(past),
        Modifiers::NONE,
    );
    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Select(Press::Replace)),
        "a click past the visible run selects the row: {answers:?}"
    );
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Open(_))),
        "a click past the visible run opened the artist: {answers:?}"
    );
}

#[test]
fn a_credit_brightens_and_points_only_over_its_words() {
    let tokens = Tokens::DARK;
    let cell = columns().at(body_rect().min).album;
    let ctx = context();
    let row = row_of(1);
    let words = words(&ctx, &row, "Record");

    let settle = |at: Pos2| {
        for _ in 0..12 {
            let _ = paint_row(&ctx, &row, vec![Event::PointerMoved(at)], Modifiers::NONE);
        }
        paint_row(&ctx, &row, vec![Event::PointerMoved(at)], Modifiers::NONE)
    };

    let over = settle(words.center());
    assert_eq!(
        over.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "the words point the hand"
    );
    assert_eq!(
        ink_of(&over, "Record"),
        tokens.palette.text,
        "the words brighten"
    );

    let at = space_after(cell, words);
    assert!(
        words.right() < at.x && at.x < cell.right(),
        "the probe at x={} must sit past the words at {}",
        at.x,
        words.right()
    );
    let away = settle(at);
    assert_ne!(
        away.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "the empty part of the cell does not point the hand"
    );
    assert_eq!(
        ink_of(&away, "Record"),
        tokens.palette.text_muted,
        "the label goes muted again off the words"
    );
}

#[test]
fn a_drag_past_the_words_lifts_the_row() {
    let cell = columns().at(body_rect().min).album;
    let (ctx, row, words) = fresh_words("Record");
    let at = space_after(cell, words);
    assert!(
        words.right() < at.x && at.x < cell.right(),
        "the probe at x={} must sit past the words at {}",
        at.x,
        words.right()
    );
    let answers = run(&ctx, &row, &twitch(at, 7.0), Modifiers::NONE);
    assert!(
        answers.iter().flatten().any(|click| *click == Click::Lift),
        "a drag past the words does not lift the row: {answers:?}"
    );
}

#[test]
fn the_link_node_covers_only_the_words() {
    let ctx = context();
    ctx.enable_accesskit();
    let row = row_of(1);
    let key = (row.uri, row.number);
    let mut response = None;
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
        let link = ui.scope_id().with(("track-artist", key));
        response = ui.ctx().read_response(link).map(|response| response.rect);
    });
    output.textures_delta.clear();

    let words = text_rect(&output, "Slow Tide").expect("the credit is painted");
    let update = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accesskit is on");
    let (_, node) = update
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == egui::accesskit::Role::Link
                && node.label() == Some("Open artist Slow Tide")
        })
        .expect("the artist cell registers a link node");
    let bounds = node.bounds().expect("the link node covers the words");
    let node_rect = Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    );
    assert!(
        (node_rect.min - words.min).length() < 1.0 && (node_rect.max - words.max).length() < 1.0,
        "the link node is {node_rect:?} where the words are {words:?}"
    );

    // The node's bounds are the response's rect, which is the narrowed hit.
    let rect = response.expect("the link was interacted this frame");
    assert!(
        (rect.min - words.min).length() < 1.0 && (rect.max - words.max).length() < 1.0,
        "the link's response is {rect:?} where the words are {words:?}"
    );
}
