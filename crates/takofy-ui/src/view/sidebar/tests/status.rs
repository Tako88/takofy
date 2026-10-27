use super::super::*;
use super::test_support::*;

use egui::Event;

use crate::message::Update;
use crate::view::test_support::click_events;

/// The count's own words, read from its status node inside the pass.
fn status_label(ctx: &egui::Context, library: &mut Library, art: &mut Art) -> Option<String> {
    let (_, label) = frame_probing(ctx, Vec::new(), library, art, true, |ui| {
        ui.ctx()
            .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                node.label().map(str::to_owned)
            })
            .flatten()
    });
    label
}

#[test]
fn the_status_node_carries_the_count_politely() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    let mut art = Art::default();

    let (_, read) = frame_probing(&ctx, Vec::new(), &mut library, &mut art, true, |ui| {
        ui.ctx()
            .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                (node.role(), node.live(), node.label().map(str::to_owned))
            })
            .expect("the count has a status node")
    });

    assert_eq!(read.0, egui::Role::Status);
    assert_eq!(read.1, Some(egui::accesskit::Live::Polite));
    assert_eq!(
        read.2.as_deref(),
        Some("2 saved"),
        "the rest string is the reference's own words"
    );
}

#[test]
fn the_status_node_says_n_of_total_while_narrowed() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut library = movable_playlists(&["Midnight City", "Outro", "Bravo"], 7, true, None);
    library.rail_filter.edit().push_str("mid");
    let mut art = Art::default();

    assert_eq!(
        status_label(&ctx, &mut library, &mut art).as_deref(),
        Some("1 of 3"),
        "the announced string counts both axes"
    );
}

#[test]
fn the_status_node_follows_a_chip_change() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
        "An artist",
    )]));
    let mut art = Art::default();

    assert_eq!(
        status_label(&ctx, &mut library, &mut art).as_deref(),
        Some("2 of 3")
    );

    assert!(library.choose_kind(SidebarKind::Artists));
    assert_eq!(
        status_label(&ctx, &mut library, &mut art).as_deref(),
        Some("1 of 3"),
        "the chosen kind is what the count counts"
    );
}

#[test]
fn the_fields_placeholder_property_matches_the_status_string() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut library = movable_playlists(&["Midnight City", "Outro"], 7, true, None);
    let mut art = Art::default();

    // The field's own resting line, and the same string on its node.
    let read = |library: &mut Library, art: &mut Art| {
        frame_probing(&ctx, Vec::new(), library, art, true, |ui| {
            let count = ui
                .ctx()
                .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                    node.label().map(str::to_owned)
                });
            let field = ui
                .ctx()
                .accesskit_node_builder(ui.scope_id().with("library-filter"), |node| {
                    node.placeholder().map(str::to_owned)
                });
            (count.flatten(), field.flatten())
        })
        .1
    };

    let (count, field) = read(&mut library, &mut art);
    assert_eq!(count.as_deref(), Some("2 saved"));
    assert_eq!(field, count, "the two are one string");

    library.rail_filter.edit().push_str("mid");
    let (count, field) = read(&mut library, &mut art);
    assert_eq!(count.as_deref(), Some("1 of 2"));
    assert_eq!(field, count, "the two are one string while narrowed too");
}

#[test]
fn a_status_node_exists_while_the_create_field_is_open_but_the_field_gets_no_stray_node() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    library.naming.open();
    let mut art = Art::default();

    let (_, (status, stray)) =
        frame_probing(&ctx, Vec::new(), &mut library, &mut art, true, |ui| {
            let status = ui
                .ctx()
                .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                    node.label().map(str::to_owned)
                })
                .flatten();
            let stray = ui
                .ctx()
                .has_accesskit_node(ui.scope_id().with("library-filter"));
            (status, stray)
        });

    assert_eq!(
        status.as_deref(),
        Some("2 saved"),
        "the count is announced whether or not the field is drawn"
    );
    assert!(!stray, "the undrawn field was given a node of its own");
}

#[test]
fn the_head_draws_the_resting_count_as_the_placeholder() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let field = filter_field_of(&output);
    let (pos, _) = painted_text(&output, "2 SAVED");
    assert!(
        field.contains(pos),
        "the count is the field's resting line: {pos:?} in {field:?}"
    );
    assert!(
        pos.x >= field.left() + Size::SM.side,
        "the line starts past the magnifier's glyph: {}",
        pos.x
    );
}

#[test]
fn the_placeholder_names_the_kind_count_when_a_chip_narrows() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
        "A show",
    )]));
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
        "An artist",
    )]));
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(
        painted(&output, "2 OF 4"),
        "the resting line names what the chip shows: {:?}",
        drawn_text(&output)
    );
    assert!(
        !painted(&output, "4 SAVED"),
        "the total is not the whole story"
    );
}

#[test]
fn the_old_count_line_is_gone() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let texts = drawn_text(&output);
    assert!(
        !texts.contains(&"2 saved".to_owned()),
        "the head still draws a second copy of the count: {texts:?}"
    );
    assert!(texts.contains(&"2 SAVED".to_owned()));
}

#[test]
fn the_placeholder_disappears_the_moment_text_exists() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let field = filter_field_of(&output);

    for event in click_events(field.center()) {
        let _ = run_column(&ctx, vec![event], &mut library, &mut art);
    }
    assert!(
        ctx.text_edit_focused(),
        "the click focused the rail's field"
    );

    let (output, _) = run_column(
        &ctx,
        vec![Event::Text("a".to_owned())],
        &mut library,
        &mut art,
    );

    assert_eq!(library.rail_filter.query(), "a");
    assert!(
        !painted(&output, "2 SAVED"),
        "the placeholder is still painted over the text"
    );
}

#[test]
fn the_field_takes_what_the_create_button_leaves() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let field = filter_field_of(&output);
    let row_left = rail().left() + space::S4;
    let create_left = rail().right() - space::S4 - extent::BUTTON;

    assert!(
        (field.left() - row_left).abs() < 0.01,
        "the field starts at the row's left: {} vs {row_left}",
        field.left()
    );
    assert!(
        (field.right() + space::S2 - create_left).abs() < 0.01,
        "the field takes what the button leaves: {} vs {create_left}",
        field.right()
    );
    assert!(
        (field.height() - extent::BUTTON).abs() < 0.01,
        "the field is one button tall: {}",
        field.height()
    );
}

#[test]
fn opening_the_create_field_swaps_the_filter_field_out_and_clears_the_query() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    library.rail_filter.edit().push_str("zzz");
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert!(
        painted(&output, "zzz"),
        "the query is drawn before the create field opens"
    );

    library.begin_create();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(
        library.rail_filter.query().is_empty(),
        "the create field's opening clears the query"
    );
    assert!(
        filter_hairlines(&output).is_empty(),
        "the filter field is not drawn beside the new playlist's name"
    );
}

#[test]
fn closing_the_create_field_restores_the_filter_field() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    library.naming.open();
    let (open, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert!(filter_hairlines(&open).is_empty(), "the field is out");

    library.naming.cancel();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(
        filter_hairlines(&output).len(),
        1,
        "the filter field came back"
    );
    assert!(painted(&output, "2 SAVED"), "and its resting line with it");
}
