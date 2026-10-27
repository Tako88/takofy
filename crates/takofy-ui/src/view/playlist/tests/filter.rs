use super::super::hero; // the band recipe, which the tests' own `hero` module shadows
use super::super::*;
use super::test_support::*;
use crate::message::{Failure, Update};
use egui::{Context, Event, Modifiers, Pos2, Rect, Vec2};
use takofy_model::TrackEntry;

/// The clear control's centre, solved the way the view places it: the field's
/// right padding, then the glyph's side.
fn clear_point(field: Rect) -> Pos2 {
    Pos2::new(
        field.right() - space::S1 - (space::S3 + crate::theme::icon::Size::SM.side) / 2.0,
        field.center().y,
    )
}

/// The clear control's box, solved the way the view places it.
fn clear_box(field: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(
            field.right() - space::S3 - crate::theme::icon::Size::SM.side - space::S1,
            field.top(),
        ),
        Pos2::new(field.right() - space::S1, field.bottom()),
    )
}

/// The boxes of the geometry a frame painted: a glyph is paths and discs,
/// so this finds an icon without knowing which one it is.
fn painted_boxes(output: &egui::FullOutput) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Path(path) if !path.points.is_empty() => {
                let mut boxed = Rect::NOTHING;
                for point in &path.points {
                    boxed.extend_with(*point);
                }
                Some(boxed)
            }
            egui::epaint::Shape::Circle(circle) => Some(Rect::from_center_size(
                circle.center,
                Vec2::splat(circle.radius),
            )),
            _ => None,
        })
        .collect()
}

/// Every text shape a frame painted, as a box.
fn painted_text_boxes(output: &egui::FullOutput) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// One click at `at` with `modifiers` held, one pass per pointer event so
/// egui's hit test has the previous frame to read.
fn click_holding(
    ctx: &Context,
    at: Pos2,
    modifiers: Modifiers,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) {
    let tokens = Tokens::DARK;
    run(ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            library,
            transport,
            queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    });
    for event in crate::view::test_support::click_events(at) {
        run(
            ctx,
            input(vec![Event::ModifiersChanged(modifiers), event]),
            |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    transport,
                    queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            },
        );
    }
}

/// The action row's shuffle tile, solved as `play_tile_rect_for` solves play.
fn shuffle_tile_rect_for(ctx: &Context, title: &str) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let band = hero::Band {
                eyebrow: "Playlist",
                title,
                meta: Vec::new(),
                credit: None,
                description: "",
                cover: Cover::Missing,
                tint: None,
            };
            let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
            let row = actionbar::row_rect(ui);
            found = tiles_of(row).shuffle;
        });
    });
    found.expect("the band and its action row lay out")
}

fn queued(queue: &Queue) -> usize {
    usize::from(queue.current().is_some()) + queue.upcoming().len()
}

/// Every uri the queue holds, the current entry first.
fn queue_uris(queue: &Queue) -> Vec<String> {
    let mut uris: Vec<String> = queue
        .current()
        .map(|entry| entry.uri.clone())
        .into_iter()
        .collect();
    uris.extend(queue.upcoming().map(|(_, entry)| entry.uri.clone()));
    uris
}

/// Every uri the whole sixty-two-row list holds, in its own order.
fn whole_list() -> Vec<String> {
    (0..62).map(|index| format!("t{index}")).collect()
}

#[test]
fn typing_in_the_filter_keeps_the_matching_rows_with_their_true_numbers() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    let field = field_at(&layout);
    let _ = click_in(
        ALL,
        &ctx,
        field.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    let output = frame_in(
        &ctx,
        ALL,
        &mut library,
        vec![Event::Text("midnight".to_owned())],
    );

    assert_eq!(
        painted_rows(&output),
        MIDNIGHT.to_vec(),
        "the twelve matching rows are drawn, by their true indices"
    );
    assert!(
        drawn_texts(&output).contains(&"40"),
        "row forty still shows forty: {:?}",
        drawn_texts(&output)
    );
    let tops: Vec<f32> = MIDNIGHT
        .iter()
        .map(|index| {
            text_rect(&output, &format!("t{index}"))
                .unwrap_or_else(|| panic!("t{index} was not painted"))
                .top()
        })
        .collect();
    assert!(
        tops.windows(2)
            .all(|pair| (pair[1] - pair[0] - extent::ROW_H).abs() < 0.01),
        "the kept rows sit in consecutive slots: {tops:?}"
    );
}

#[test]
fn the_count_line_reads_n_of_total_tracks() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");

    let output = tracklist_frame_in(&ctx, ALL, &mut library);
    let field = field_at(&output);
    let count = text_rect(&output, "12 OF 62 TRACKS").expect("the count is painted");
    assert!(
        count.right() <= field.left() - crate::theme::actionbar::TOOLS_GAP + 0.01,
        "the count sits one tools gap left of the field: {count:?} against {field:?}"
    );
    assert!(
        !drawn_texts(&output).contains(&"FILTER"),
        "text replaces the placeholder"
    );

    library.track_filter.edit(PLAYLIST).clear();
    library.track_filter.edit(PLAYLIST).push_str("   ");
    let output = tracklist_frame_in(&ctx, ALL, &mut library);
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF ")),
        "a whitespace query is no filter and paints no count"
    );
}

#[test]
fn at_900_the_count_gives_its_room_to_the_rail() {
    let ctx = context();
    // The rail starts at 900, inclusive, and the count is what steps aside
    // for it: the field keeps its 176 and the list still narrows.
    let rail = Vec2::new(900.0, 3200.0);
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");

    let output = tracklist_frame_in(&ctx, rail, &mut library);

    assert_eq!(
        painted_rows(&output),
        MIDNIGHT.to_vec(),
        "the query still narrows the list in a rail"
    );
    assert!(
        field_at(&output).width() == crate::theme::actionbar::FIELD_W,
        "the field keeps its width: {:?}",
        field_at(&output)
    );
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF ")),
        "the count is hidden at 900: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn the_first_keystroke_replaces_the_placeholder_in_its_own_frame() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    assert!(
        drawn_texts(&layout).contains(&"FILTER"),
        "the empty field shows its placeholder"
    );
    let field = field_at(&layout);
    let _ = click_in(
        ALL,
        &ctx,
        field.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    // `i` is in every `Midnight Artist` row and in no title, so the first
    // character is enough to tell the narrowed list from the whole one.
    let typed = frame_in(&ctx, ALL, &mut library, vec![Event::Text("i".to_owned())]);
    let texts = drawn_texts(&typed);

    assert_eq!(
        painted_rows(&typed),
        MIDNIGHT.to_vec(),
        "the first character narrows the rows on its own frame"
    );
    assert!(
        !texts.contains(&"FILTER"),
        "the placeholder leaves with the first character: {texts:?}"
    );
    assert!(
        texts.contains(&"i"),
        "the field paints the character on its own frame: {texts:?}"
    );
    assert!(
        texts.contains(&"12 OF 62 TRACKS"),
        "the count is on the keystroke's own frame: {texts:?}"
    );

    let clear = clear_box(field_at(&typed));
    assert!(
        painted_boxes(&typed)
            .iter()
            .any(|glyph| clear.contains_rect(*glyph)),
        "the clear control appears with the text: {clear:?}"
    );
    for text in painted_text_boxes(&typed) {
        assert!(
            !text.intersects(clear),
            "no text sits under the clear control: {text:?} against {clear:?}"
        );
    }
}

#[test]
fn clearing_the_filter_restores_every_row_in_order() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let narrowed = tracklist_frame_in(&ctx, ALL, &mut library);
    assert_eq!(painted_rows(&narrowed), MIDNIGHT.to_vec());

    library.track_filter.clear();
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert_eq!(
        painted_rows(&output),
        (0..62).collect::<Vec<_>>(),
        "clearing restores every row, in the list's order"
    );
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF "))
    );
}

#[test]
fn the_clear_button_clears_in_the_frame_it_is_clicked() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    let field = field_at(&layout);
    let _ = click_in(
        ALL,
        &ctx,
        field.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    let typed = frame_in(
        &ctx,
        ALL,
        &mut library,
        vec![Event::Text("midnight".to_owned())],
    );
    assert_eq!(painted_rows(&typed), MIDNIGHT.to_vec());

    let clear = clear_point(field_at(&typed));
    let mut output = typed;
    for event in crate::view::test_support::click_events(clear) {
        output = frame_in(&ctx, ALL, &mut library, vec![event]);
    }

    assert_eq!(
        painted_rows(&output),
        (0..62).collect::<Vec<_>>(),
        "the click frame itself shows every row"
    );
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF ")),
        "the count is gone with the query"
    );
    assert!(library.track_filter.edit(PLAYLIST).is_empty());

    // The chrome follows the click on its own frame: the clear control goes
    // with the text it cleared, and the hairline already says the field kept
    // the keyboard the click gave it.
    assert!(
        drawn_texts(&output).contains(&"FILTER"),
        "the placeholder is back on the click frame"
    );
    let clear = clear_box(field_at(&output));
    assert!(
        !painted_boxes(&output)
            .iter()
            .any(|glyph| clear.contains_rect(*glyph)),
        "the clear control leaves with the text: {clear:?}"
    );
    assert_eq!(
        filter_hairline(&output).1,
        Tokens::DARK.palette.text,
        "the hairline is focused on the click frame"
    );

    let _ = tracklist_frame_in(&ctx, ALL, &mut library);
    assert!(ctx.text_edit_focused(), "the field keeps the keyboard");
}

#[test]
fn escape_in_the_focused_field_clears_and_returns_the_keyboard() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    let field = field_at(&layout);
    let _ = click_in(
        ALL,
        &ctx,
        field.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    let typed = frame_in(
        &ctx,
        ALL,
        &mut library,
        vec![Event::Text("midnight".to_owned())],
    );
    assert_eq!(painted_rows(&typed), MIDNIGHT.to_vec());

    let output = frame_in(&ctx, ALL, &mut library, vec![key(egui::Key::Escape)]);

    assert!(library.track_filter.edit(PLAYLIST).is_empty());
    assert_eq!(
        painted_rows(&output),
        (0..62).collect::<Vec<_>>(),
        "the full list is back in the Escape frame"
    );
    assert!(!ctx.text_edit_focused(), "Escape returns the keyboard");

    // The chrome is the Escape frame's too: the placeholder is back, the
    // clear control is not, and the hairline has already let focus go.
    assert!(
        drawn_texts(&output).contains(&"FILTER"),
        "the placeholder returns on the Escape frame"
    );
    let clear = clear_box(field_at(&output));
    assert!(
        !painted_boxes(&output)
            .iter()
            .any(|glyph| clear.contains_rect(*glyph)),
        "the clear control leaves with the text: {clear:?}"
    );
    assert_eq!(
        filter_hairline(&output).1,
        Tokens::DARK.palette.border,
        "the hairline gives up the keyboard on the Escape frame"
    );
}

#[test]
fn escape_the_frame_after_a_click_still_clears() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    let centre = field_at(&layout).center();
    let _ = frame_in(&ctx, ALL, &mut library, vec![Event::PointerMoved(centre)]);
    let _ = frame_in(
        &ctx,
        ALL,
        &mut library,
        vec![crate::view::test_support::button(centre, true)],
    );
    let output = frame_in(&ctx, ALL, &mut library, vec![key(egui::Key::Escape)]);

    assert!(
        library.track_filter.edit(PLAYLIST).is_empty(),
        "the Escape immediately after a click still clears"
    );
    assert_eq!(painted_rows(&output), (0..62).collect::<Vec<_>>());
    assert!(!ctx.text_edit_focused());
}

#[test]
fn escape_in_an_empty_focused_field_surrenders_focus() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    let field = field_at(&layout);
    let _ = click_in(
        ALL,
        &ctx,
        field.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    assert!(ctx.text_edit_focused(), "the click focused the field");

    let _ = frame_in(&ctx, ALL, &mut library, vec![key(egui::Key::Escape)]);

    assert!(
        !ctx.text_edit_focused(),
        "Escape with nothing to clear still returns the keyboard"
    );
}

#[test]
fn escape_with_focus_on_a_row_leaves_the_query() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
    let row = row_rect_for(&ctx, 12, 0).center();
    let _ = click(&ctx, row, &mut library, &mut transport, &mut queue);

    let output = frame_in(&ctx, PROBE, &mut library, vec![key(egui::Key::Escape)]);

    assert_eq!(
        library.track_filter.needle(PLAYLIST).as_deref(),
        Some("midnight"),
        "a page-wide Escape would fight the drawer's"
    );
    let painted = painted_rows(&output);
    assert!(!painted.is_empty(), "the filtered rows are still drawn");
    assert_eq!(
        painted,
        MIDNIGHT[..painted.len()].to_vec(),
        "and they are still the matching rows"
    );
}

#[test]
fn a_query_with_no_match_hides_the_head_and_says_so() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("zzz");

    let output = tracklist_frame_in(&ctx, ALL, &mut library);
    let texts = drawn_texts(&output);

    assert!(
        texts.contains(&"Nothing in this list matches \u{201c}zzz\u{201d}"),
        "the note names the query: {texts:?}"
    );
    assert!(!texts.contains(&"TITLE"), "the head hides with the rows");
    assert!(
        texts.contains(&"0 OF 62 TRACKS"),
        "the count still says zero"
    );
    assert!(field_at(&output).width() > 0.0, "the field stays drawn");
}

#[test]
fn a_shift_click_on_a_filtered_list_picks_only_visible_rows() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
    let first = row_rect_for(&ctx, 12, 0).center();
    let third = row_rect_for(&ctx, 12, 2).center();
    let _ = click(&ctx, first, &mut library, &mut transport, &mut queue);
    click_holding(
        &ctx,
        third,
        Modifiers::SHIFT,
        &mut library,
        &mut transport,
        &mut queue,
    );

    for row in [1, 4, 9] {
        assert!(
            picked(&ctx, PLAYLIST, 62, row),
            "visible row {row} is in the span"
        );
    }
    for row in [0, 2, 3, 12] {
        assert!(
            !picked(&ctx, PLAYLIST, 62, row),
            "hidden row {row} never enters the span"
        );
    }
}

#[test]
fn a_pick_made_before_filtering_loses_its_hidden_rows() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let list = egui::Id::unique(("playlist-tracks", PLAYLIST));
    crate::view::carry::select_rows(&ctx, list, 0..5);
    assert!(picked(&ctx, PLAYLIST, 62, 0));

    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert!(!picked(&ctx, PLAYLIST, 62, 0), "a hidden pick is dropped");
    assert!(!picked(&ctx, PLAYLIST, 62, 2));
    assert!(picked(&ctx, PLAYLIST, 62, 1), "a visible pick survives");
    assert!(picked(&ctx, PLAYLIST, 62, 4));
    assert_eq!(painted_rows(&output), MIDNIGHT.to_vec());
}

#[test]
fn reloading_the_same_playlist_keeps_the_query_and_the_narrowed_rows() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let narrowed = tracklist_frame_in(&ctx, ALL, &mut library);
    assert_eq!(painted_rows(&narrowed), MIDNIGHT.to_vec());

    library.select(PLAYLIST);
    assert!(matches!(library.tracks, Load::Loading));
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);
    library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: midnight_tracks(),
        cover: None,
        access: access(true),
    });
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert_eq!(
        painted_rows(&output),
        MIDNIGHT.to_vec(),
        "the reload is not a page change"
    );
    assert!(
        drawn_texts(&output).contains(&"midnight"),
        "the field comes back populated: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn a_failed_load_clears_the_query() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);

    library.apply(&Update::TracksFailed {
        playlist: PLAYLIST.to_owned(),
        failure: Failure::Load,
    });
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert_eq!(library.track_filter.needle(PLAYLIST), None);
    assert!(
        !drawn_texts(&output).contains(&"FILTER"),
        "no field to see it in"
    );
    assert!(drawn_texts(&output).contains(&crate::signin::failure_message(Failure::Load)));
}

#[test]
fn an_empty_list_clears_the_query() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);

    library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: access(true),
    });
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert_eq!(library.track_filter.needle(PLAYLIST), None);
    assert!(
        drawn_texts(&output).contains(&"No tracks in this playlist."),
        "the empty page is unfiltered"
    );
}

#[test]
fn a_query_survives_loading_and_its_field_is_absent_meanwhile() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);

    library.select(PLAYLIST);
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert_eq!(
        library.track_filter.needle(PLAYLIST).as_deref(),
        Some("midnight"),
        "a reload of the same URI keeps the query"
    );
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF ")),
        "the skeleton has no count"
    );
    assert!(!drawn_texts(&output).contains(&"midnight"));
}

#[test]
fn deleting_every_visible_row_keeps_the_field_and_the_no_match_note() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);

    let remainder: Vec<TrackEntry> = midnight_tracks()
        .into_iter()
        .filter(|entry| !entry.artists.contains("Midnight"))
        .collect();
    library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: remainder,
        cover: None,
        access: access(true),
    });
    let output = tracklist_frame_in(&ctx, ALL, &mut library);
    let texts = drawn_texts(&output);

    assert!(
        texts.contains(&"Nothing in this list matches \u{201c}midnight\u{201d}"),
        "the user can still see why, and clear it: {texts:?}"
    );
    assert!(texts.contains(&"0 OF 50 TRACKS"));
    assert!(field_at(&output).width() > 0.0, "the field is still drawn");
}

#[test]
fn another_page_shows_an_unfiltered_list() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);

    library.select("spotify:playlist:b");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:b".to_owned(),
        tracks: vec![track("u0", "t0"), track("u1", "t1")],
        cover: None,
        access: access(true),
    });
    let output = tracklist_frame_in(&ctx, ALL, &mut library);

    assert_eq!(library.track_filter.needle("spotify:playlist:b"), None);
    assert_eq!(painted_rows(&output), vec![0, 1], "the new page is whole");
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF "))
    );
}

#[test]
fn a_filtered_editable_page_offers_no_drop_zone_and_an_unfiltered_one_does() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());

    let _ = tracklist_frame_in(&ctx, ALL, &mut library);
    assert!(
        crate::view::carry::take_offer(&ctx).is_some(),
        "an unfiltered editable page offers its rows"
    );

    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let output = tracklist_frame_in(&ctx, ALL, &mut library);
    assert_eq!(painted_rows(&output), MIDNIGHT.to_vec());
    assert!(
        crate::view::carry::take_offer(&ctx).is_none(),
        "a filtered page offers no reorder"
    );

    library.track_filter.clear();
    let _ = tracklist_frame_in(&ctx, ALL, &mut library);
    assert!(
        crate::view::carry::take_offer(&ctx).is_some(),
        "clearing the query restores the drop zone"
    );
}

#[test]
fn a_filtered_page_still_lifts_a_pick_to_the_rail_but_offers_nothing() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
    let first = row_rect_for(&ctx, 12, 0).center();
    lift_row(&ctx, first, &mut library, &mut transport, &mut queue);

    let carried = crate::view::carry::carried(&ctx).expect("a filtered row still lifts");
    assert_eq!(carried.rows, vec![1], "the lift carries the true index");
    assert!(
        crate::view::carry::take_offer(&ctx).is_none(),
        "the page still withholds the reorder"
    );
}

#[test]
fn a_filtered_double_click_plays_the_true_index() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
    let first = row_rect_for(&ctx, 12, 0).center();
    let command = double_click(&ctx, first, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t1".to_owned()
        }),
        "slot zero is the second true row"
    );
}

#[test]
fn the_band_play_and_shuffle_ignore_the_filter() {
    let ctx = context();

    // The band is the whole list's, hidden rows included: both tiles start
    // the queue on the true first row (the first row the query leaves is
    // `t1`), and both hand the queue all sixty-two entries.
    for (shuffled, tile) in [
        (false, play_tile_rect_for as fn(&Context, &str) -> Rect),
        (true, shuffle_tile_rect_for),
    ] {
        let mut library = library_named(midnight_tracks(), "Evening");
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let at = tile(&ctx, "Evening").center();
        let command = click(&ctx, at, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            }),
            "the band starts the true first row (shuffled: {shuffled})"
        );
        assert_eq!(
            queued(&queue),
            62,
            "the queue holds the whole playlist (shuffled: {shuffled})"
        );
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("t0"),
            "the true first row is the current one (shuffled: {shuffled})"
        );
        let mut uris = queue_uris(&queue);
        let mut expected = whole_list();
        if shuffled {
            // Shuffle draws the rest in its own order, so the whole list is
            // read as the same set of entries, not the same sequence.
            uris.sort();
            expected.sort();
        }
        assert_eq!(
            uris, expected,
            "every true row is in the queue (shuffled: {shuffled})"
        );
    }
}

#[test]
fn a_play_from_the_page_remembers_the_playlist() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Evening");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let at = play_tile_rect_for(&ctx, "Evening").center();

    let command = click(&ctx, at, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t0".to_owned()
        })
    );
    assert_eq!(
        queue.origin(),
        Some(PLAYLIST),
        "a move from here has a page to carry"
    );
}
