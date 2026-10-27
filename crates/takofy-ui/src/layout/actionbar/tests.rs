use super::*;
use crate::layout::{grid_for, shell_for};

fn slot(top: f32, width: f32, height: f32) -> Rect {
    Rect::from_min_size(Pos2::new(0.0, top), Vec2::new(width, height))
}

/// A playlist's stuck row, with its field, at `width`.
fn playlist_row(width: f32, kind: Kind) -> (Rect, Placed) {
    let bar = slot(12.0, width, actionbar::STUCK_H);
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    (bar, placed)
}

/// The form a page at `width` has when its table is the one the width gives.
fn form_at(width: f32) -> Form {
    Form::at(width, regions_for(width).table)
}

/// A playlist page at `width`, with the form its own table gives it.
fn kind_at(width: f32) -> Kind {
    Kind::Playlist(form_at(width))
}

#[test]
fn a_bar_at_or_below_the_clip_rests() {
    let flow = slot(100.0, 400.0, actionbar::REST_H);
    let (rect, stuck) = pin(flow, 12.0);
    assert!(!stuck, "a row below the clip top stays in its place");
    assert_eq!(rect, flow);

    // A row whose top is exactly the clip's top has not moved either: the
    // pin is for a row that has scrolled past it.
    let (rect, stuck) = pin(slot(12.0, 400.0, actionbar::REST_H), 12.0);
    assert!(!stuck, "a row at the clip top is not pinned");
    assert_eq!(rect.top(), 12.0);
}

#[test]
fn a_bar_above_the_clip_pins_at_the_stuck_height() {
    let flow = slot(-30.0, 400.0, actionbar::REST_H);
    let (rect, stuck) = pin(flow, 12.0);
    assert!(stuck);
    assert_eq!(rect.top(), 12.0, "the row sits at the clip's top edge");
    assert_eq!(rect.height(), actionbar::STUCK_H);
    assert_eq!(rect.width(), flow.width(), "the row keeps the page's width");
}

#[test]
fn play_is_a_gutter_in_and_centred_at_both_heights() {
    for height in [actionbar::REST_H, actionbar::STUCK_H] {
        let bar = slot(12.0, 900.0, height);
        let placed = solve(
            bar,
            kind_at(900.0),
            Parts {
                shuffle: true,
                field: true,
                follow: None,
            },
        );
        assert_eq!(placed.play.left(), bar.left() + space::GUTTER);
        assert_eq!(placed.play.width(), hero::PLAY);
        assert_eq!(placed.play.height(), hero::PLAY);
        assert_eq!(placed.play.center().y, bar.center().y);
    }
}

#[test]
fn tools_hug_the_right_gutter_in_reference_order() {
    let (bar, placed) = playlist_row(1440.0, kind_at(1440.0));
    assert_eq!(placed.more.right(), bar.right() - space::GUTTER);
    let shuffle = placed.shuffle.expect("a playlist's row has a shuffle");
    let heart = placed.heart.expect("a wide playlist keeps its heart");
    assert_eq!(shuffle.right(), heart.left() - actionbar::TOOLS_GAP);
    assert_eq!(heart.right(), placed.more.left() - actionbar::TOOLS_GAP);
    for tool in [shuffle, heart, placed.more] {
        assert_eq!(tool.width(), extent::BUTTON_LG);
        assert_eq!(tool.height(), extent::BUTTON_LG);
        assert_eq!(tool.center().y, bar.center().y);
    }
}

#[test]
fn no_shuffle_closes_the_gap_without_moving_the_rest() {
    let bar = slot(12.0, 1440.0, actionbar::REST_H);
    let parts = |shuffle| Parts {
        shuffle,
        field: false,
        follow: None,
    };
    let with = solve(bar, Kind::Other, parts(true));
    let without = solve(bar, Kind::Other, parts(false));
    assert!(without.shuffle.is_none(), "an album has no shuffle");
    assert_eq!(without.play, with.play);
    assert_eq!(without.heart, with.heart);
    assert_eq!(without.more, with.more);
}

#[test]
fn the_title_runs_from_play_plus_gap_to_the_first_tool_minus_gap() {
    let bar = slot(12.0, 1440.0, actionbar::STUCK_H);
    let kind = kind_at(1440.0);
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: false,
            follow: None,
        },
    );
    let label = title(bar, &placed, None, kind, false).expect("a wide row has room for a name");
    assert_eq!(label.top(), bar.top());
    assert_eq!(label.bottom(), bar.bottom());
    assert_eq!(label.left(), placed.play.right() + actionbar::GAP);
    assert_eq!(
        label.right(),
        placed.shuffle.expect("a playlist has a shuffle").left() - actionbar::GAP
    );

    // The album's row has no shuffle, so its name may run up to the heart.
    let placed = solve(
        bar,
        Kind::Other,
        Parts {
            shuffle: false,
            field: false,
            follow: None,
        },
    );
    assert_eq!(
        title(bar, &placed, None, Kind::Other, false)
            .expect("room")
            .right(),
        placed.heart.expect("an album keeps its heart").left() - actionbar::GAP
    );
}

#[test]
fn a_title_without_room_is_none() {
    // The box's left is the gutter, the play tile and the gap; its right is
    // the three tools, their gaps and the gutter, and the gap before them.
    let left = space::GUTTER + hero::PLAY + actionbar::GAP;
    let from_the_end =
        space::GUTTER + 3.0 * extent::BUTTON_LG + 2.0 * actionbar::TOOLS_GAP + actionbar::GAP;
    let fits = left + text::MD + from_the_end;

    let bar = slot(12.0, fits, actionbar::STUCK_H);
    let placed = solve(
        bar,
        Kind::Other,
        Parts {
            shuffle: true,
            field: false,
            follow: None,
        },
    );
    assert!(
        title(bar, &placed, None, Kind::Other, false).is_some(),
        "a box exactly one text tier wide still holds the name"
    );
    let bar = slot(12.0, fits - 1.0, actionbar::STUCK_H);
    let placed = solve(
        bar,
        Kind::Other,
        Parts {
            shuffle: true,
            field: false,
            follow: None,
        },
    );
    assert!(
        title(bar, &placed, None, Kind::Other, false).is_none(),
        "a box narrower than a name holds nothing"
    );
}

#[test]
fn the_field_is_176_by_34_between_the_count_and_shuffle() {
    let kind = kind_at(1440.0);
    let (bar, placed) = playlist_row(1440.0, kind);
    let field = placed.field.expect("a wide playlist row has a field");
    let shuffle = placed.shuffle.expect("a playlist row has a shuffle");

    assert_eq!(field.width(), actionbar::FIELD_W);
    assert_eq!(field.height(), extent::BUTTON_LG);
    assert_eq!(field.center().y, bar.center().y);
    assert_eq!(field.right(), shuffle.left() - actionbar::TOOLS_GAP);

    // The count is one tools gap to the field's left, and the name ends one
    // row gap before the count.
    let count = count(&placed, kind, 90.0).expect("a wide row has room for a count");
    assert_eq!(count.right(), field.left() - actionbar::TOOLS_GAP);
    assert_eq!(count.width(), 90.0);
    assert_eq!(
        title(bar, &placed, Some(count), kind, true)
            .expect("room")
            .right(),
        count.left() - actionbar::GAP
    );
}

#[test]
fn at_900_the_count_is_hidden_and_the_field_keeps_176() {
    // 900 is the rail's own breakpoint, inclusive: the count's room is the
    // sidebar's, so at exactly 900 there is no count.
    let form = form_at(900.0);
    assert!(!form.count, "the rail takes the count's room at 900");
    let kind = Kind::Playlist(form);
    let (bar, placed) = playlist_row(900.0, kind);
    let field = placed.field.expect("the field stays");
    assert_eq!(field.width(), actionbar::FIELD_W);
    assert!(
        count(&placed, kind, 0.0).is_none(),
        "no count is placed however narrow its label is"
    );
    assert!(
        title(bar, &placed, None, kind, true).is_some(),
        "the name keeps the room the count would have taken"
    );

    // One point wider and the rail is gone, so the count has its room back.
    assert!(form_at(901.0).count, "the count returns above 900");
}

#[test]
fn at_760_a_playlist_field_fills_and_the_heart_goes() {
    let form = form_at(crate::layout::TABLE_NARROW_MAX);
    assert!(form.narrow, "760 is the narrow table's own breakpoint");
    let kind = Kind::Playlist(form);
    let (bar, placed) = playlist_row(760.0, kind);
    let field = placed.field.expect("the field fills the row");
    let shuffle = placed.shuffle.expect("a playlist row has a shuffle");

    assert!(placed.heart.is_none(), "the heart gives the field its room");
    assert_eq!(field.left(), placed.play.right() + actionbar::GAP);
    assert_eq!(field.right(), shuffle.left() - actionbar::TOOLS_GAP);
    assert!(
        field.width() > actionbar::FIELD_W,
        "the field took the room the count and the heart left: {field:?}"
    );
    assert!(
        title(bar, &placed, None, kind, true).is_none(),
        "a stuck narrow row has no name"
    );

    // With no field this frame, a condensed row keeps its heart and name:
    // the loading page's tools must not shift when its tracks arrive.
    let without_field = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: false,
            follow: None,
        },
    );
    assert!(
        without_field.heart.is_some(),
        "without a field the heart stays"
    );
    assert!(
        title(bar, &without_field, None, kind, false).is_some(),
        "and so does the name"
    );

    // Even in a condensed row too narrow to hold a field, the name stays
    // gone while the page has one: the room it would have had is the
    // field's, and the row has given the field up rather than hand it to the
    // name.
    let squeezed = slot(12.0, 300.0, actionbar::STUCK_H);
    let placed = solve(
        squeezed,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    assert!(
        placed.field.is_none(),
        "a row this narrow holds no field: {:?}",
        placed.field
    );
    assert!(
        title(squeezed, &placed, None, kind, true).is_none(),
        "a condensed row keeps its name off the pin whatever it holds"
    );
}

#[test]
fn an_albums_bar_does_not_change_at_760() {
    let form = Form::at(crate::layout::TABLE_NARROW_MAX, Table::Narrow);
    assert!(form.narrow, "the width alone would condense a playlist");

    let parts = Parts {
        shuffle: false,
        field: false,
        follow: None,
    };
    let bar = slot(12.0, 760.0, actionbar::STUCK_H);
    let album = solve(bar, Kind::Other, parts);

    assert!(album.heart.is_some(), "an album keeps its heart at 760");
    assert!(album.field.is_none(), "an album has no field");
    assert!(
        title(bar, &album, None, Kind::Other, false).is_some(),
        "an album keeps its name at 760"
    );
}

#[test]
fn the_title_yields_before_the_count() {
    let kind = kind_at(1440.0);
    let (bar, placed) = playlist_row(1440.0, kind);
    let field = placed.field.expect("a wide row has a field");
    let count = count(&placed, kind, 120.0).expect("a wide row has a count");
    let with = title(bar, &placed, Some(count), kind, true).expect("room");
    let without = title(bar, &placed, None, kind, true).expect("room");
    assert!(
        with.right() < without.right(),
        "the count takes the name's room: {with:?} against {without:?}"
    );
    assert_eq!(with.right(), count.left() - actionbar::GAP);
    assert!(
        with.right() < field.left() - actionbar::GAP,
        "the name stops at the count, not at the field: {with:?} against {field:?}"
    );
}

#[test]
fn the_count_holds_at_exactly_its_room_and_not_a_point_less() {
    let kind = Kind::Playlist(Form {
        count: true,
        narrow: false,
    });
    // The width at which the field's left edge is exactly one row gap right
    // of the play tile: the count's own room is then zero.
    let play_run = space::GUTTER + hero::PLAY + actionbar::GAP;
    let tools_run = space::GUTTER + 3.0 * extent::BUTTON_LG + 2.0 * actionbar::TOOLS_GAP;
    let tight = play_run + actionbar::FIELD_W + actionbar::TOOLS_GAP + tools_run;
    let label_w = 90.0;
    let exact = tight + actionbar::TOOLS_GAP + label_w;

    let bar = slot(12.0, exact, actionbar::STUCK_H);
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    let fitted = count(&placed, kind, label_w).expect("the count fits exactly");
    assert!(
        (fitted.left() - (placed.play.right() + actionbar::GAP)).abs() < 0.01,
        "the count starts exactly one gap after the play tile: {fitted:?} against {:?}",
        placed.play
    );

    let bar = slot(12.0, exact - 1.0, actionbar::STUCK_H);
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    assert!(
        count(&placed, kind, label_w).is_none(),
        "a point narrower and the count would reach into the play tile"
    );
}

#[test]
fn no_room_drops_the_count_then_shrinks_the_field_then_hides_it() {
    let kind = Kind::Playlist(Form {
        count: true,
        narrow: false,
    });
    // The room a field needs, the gap before it and the play tile's own run
    // are fixed; a row this wide has room for the field and nothing else.
    let play_run = space::GUTTER + hero::PLAY + actionbar::GAP;
    let tools_run = space::GUTTER + 3.0 * extent::BUTTON_LG + 2.0 * actionbar::TOOLS_GAP;
    let field_room = play_run + actionbar::FIELD_W + actionbar::TOOLS_GAP + tools_run;
    let bar = slot(12.0, field_room, actionbar::STUCK_H);
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    assert_eq!(
        placed.field.expect("the field just fits").width(),
        actionbar::FIELD_W
    );
    assert!(
        count(&placed, kind, 60.0).is_none(),
        "the count is what goes first"
    );

    // Narrower than the field's own width: the field shrinks, and the count
    // is still the first thing gone.
    let bar = slot(
        12.0,
        field_room - (actionbar::FIELD_W - 140.0),
        actionbar::STUCK_H,
    );
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    let field = placed.field.expect("a shrunk field is still drawn");
    assert!((field.width() - 140.0).abs() < 0.01, "{field:?}");
    assert!(field.width() < actionbar::FIELD_W);
    assert!(count(&placed, kind, 60.0).is_none());

    // At exactly the floor the field is still drawn.
    let bar = slot(
        12.0,
        field_room - (actionbar::FIELD_W - actionbar::FIELD_MIN_W),
        actionbar::STUCK_H,
    );
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    assert_eq!(
        placed
            .field
            .expect("the field at exactly its floor is drawn")
            .width(),
        actionbar::FIELD_MIN_W
    );

    // Below the floor there is no field, and so nothing to type a query into.
    let bar = slot(
        12.0,
        field_room - (actionbar::FIELD_W - actionbar::FIELD_MIN_W) - 1.0,
        actionbar::STUCK_H,
    );
    let placed = solve(
        bar,
        kind,
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    assert!(
        placed.field.is_none(),
        "the field is not drawn below its floor: {:?}",
        placed.field
    );
    assert!(count(&placed, kind, 60.0).is_none(), "nor is the count");
}

/// The size the artist's text control asks for, measured the way the view
/// measures it: the sweep is only worth anything at the drawn size.
fn text_slot(label: &str) -> Vec2 {
    let tokens = crate::theme::Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::theme::fonts::definitions());
    let mut size = Vec2::ZERO;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 400.0))),
            ..Default::default()
        },
        |ui| size = crate::view::controls::text_button_size(ui, &tokens, label),
    );
    output.textures_delta.clear();
    size
}

/// The parts of the artist's row: a shuffle, no field, and the text slot.
fn artist_parts(label: &str) -> Parts {
    Parts {
        shuffle: true,
        field: false,
        follow: Some(text_slot(label)),
    }
}

#[test]
fn a_text_slot_replaces_the_heart_and_sits_a_gap_left_of_the_tools() {
    let bar = slot(12.0, 1440.0, actionbar::REST_H);
    let placed = solve(bar, Kind::Other, artist_parts("Follow"));
    let follow = placed.follow.expect("the row has a text slot");
    let shuffle = placed.shuffle.expect("the artist's row has a shuffle");

    assert!(
        placed.heart.is_none(),
        "a text slot means no heart: {:?}",
        placed.heart
    );
    assert_eq!(
        follow.right(),
        shuffle.left() - actionbar::GAP,
        "the slot sits a row gap left of the tools: {follow:?} against {shuffle:?}"
    );
    assert_eq!(follow.size(), text_slot("Follow"));
    assert!(
        (follow.center().y - bar.center().y).abs() < 0.01,
        "the slot is centred on the row's line: {follow:?} against {bar:?}"
    );
}

#[test]
fn without_a_shuffle_the_slot_sits_a_gap_left_of_the_menu() {
    let bar = slot(12.0, 1440.0, actionbar::REST_H);
    let placed = solve(
        bar,
        Kind::Other,
        Parts {
            shuffle: false,
            field: false,
            follow: Some(text_slot("Follow")),
        },
    );
    assert!(placed.shuffle.is_none(), "an album has no shuffle");
    assert_eq!(
        placed.follow.expect("the row has a text slot").right(),
        placed.more.left() - actionbar::GAP,
        "with nothing between them the slot still clears the menu"
    );
}

#[test]
fn the_title_ends_a_gap_before_the_text_slot() {
    let bar = slot(12.0, 1440.0, actionbar::STUCK_H);
    let placed = solve(bar, Kind::Other, artist_parts("Follow"));
    let label =
        title(bar, &placed, None, Kind::Other, false).expect("a wide row has room for a name");
    assert_eq!(
        label.right(),
        placed.follow.expect("the row has a text slot").left() - actionbar::GAP,
        "the name stops one row gap short of the slot, the way it does of a tool"
    );
}

#[test]
fn the_tools_do_not_move_when_the_label_widens() {
    let bar = slot(12.0, 1440.0, actionbar::REST_H);
    let at = |width: f32| {
        solve(
            bar,
            Kind::Other,
            Parts {
                shuffle: true,
                field: false,
                follow: Some(Vec2::new(width, text_slot("Follow").y)),
            },
        )
    };
    let narrow = at(60.0);
    let wide = at(90.0);

    assert_eq!(narrow.play, wide.play, "the play tile is anchored");
    assert_eq!(
        narrow.shuffle, wide.shuffle,
        "the tools are anchored to the far edge, so a wider label never moves them"
    );
    assert_eq!(narrow.more, wide.more);
    let narrow = narrow.follow.expect("the slot is drawn");
    let wide = wide.follow.expect("the slot is drawn");
    assert_eq!(
        narrow.right(),
        wide.right(),
        "the slot grows leftward: {narrow:?} against {wide:?}"
    );
    assert!(
        (wide.left() - (narrow.left() - 30.0)).abs() < 0.01,
        "the wider label takes its room from the name side: {narrow:?} against {wide:?}"
    );
}

#[test]
fn a_page_with_no_slot_is_unchanged() {
    let bar = slot(12.0, 1440.0, actionbar::REST_H);
    let placed = solve(
        bar,
        Kind::Other,
        Parts {
            shuffle: false,
            field: false,
            follow: None,
        },
    );
    let heart = placed.heart.expect("an album keeps its heart");
    assert!(placed.follow.is_none(), "a page without a slot has none");
    assert_eq!(heart.right(), placed.more.left() - actionbar::TOOLS_GAP);
    assert_eq!(placed.more.right(), bar.right() - space::GUTTER);
    assert_eq!(
        title(bar, &placed, None, Kind::Other, false)
            .expect("room")
            .right(),
        heart.left() - actionbar::GAP,
        "the name still runs up to the heart"
    );
}

#[test]
fn the_artist_bar_never_overlaps_between_the_window_minimum_and_2400() {
    let size = text_slot("Following");
    let height = crate::layout::WINDOW_MIN[1];
    let mut width = crate::layout::WINDOW_MIN[0];
    while width <= 2400.0 {
        let regions = regions_for(width);
        let shell = shell_for(&regions);
        let grid = grid_for(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height)),
            &shell,
        );
        let bar = slot(grid.view.top(), grid.view.width(), actionbar::STUCK_H);
        let placed = solve(
            bar,
            Kind::Other,
            Parts {
                shuffle: true,
                field: false,
                follow: Some(size),
            },
        );
        let follow = placed.follow.expect("the artist's slot is always drawn");
        let shuffle = placed.shuffle.expect("the artist's row has a shuffle");
        assert!(
            follow.right() <= shuffle.left() - actionbar::GAP + 0.01,
            "{width}: the slot closes on the tools: {follow:?} against {shuffle:?}"
        );
        assert!(
            follow.left() >= placed.play.right() + actionbar::GAP,
            "{width}: the slot reaches into the play tile: {follow:?} against {:?}",
            placed.play
        );
        let boxes = [
            ("play", placed.play),
            ("follow", follow),
            ("shuffle", shuffle),
            ("more", placed.more),
        ];
        for (index, (name, rect)) in boxes.iter().enumerate() {
            assert!(
                bar.contains_rect(*rect),
                "{width}: {name} leaves the row: {rect:?} against {bar:?}"
            );
            for (other, against) in &boxes[index + 1..] {
                assert!(
                    !rect.intersects(*against),
                    "{width}: {name} {rect:?} overlaps {other} {against:?}"
                );
            }
        }
        width += 1.0;
    }
}

#[test]
fn no_part_overlaps_at_any_supported_width() {
    let height = crate::layout::WINDOW_MIN[1];
    let mut width = crate::layout::WINDOW_MIN[0];
    while width <= 2400.0 {
        let regions = regions_for(width);
        let shell = shell_for(&regions);
        let grid = grid_for(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height)),
            &shell,
        );
        let bar = slot(grid.view.top(), grid.view.width(), actionbar::STUCK_H);
        let kind = Kind::Playlist(Form::at(width, regions.table));
        let placed = solve(
            bar,
            kind,
            Parts {
                shuffle: true,
                field: true,
                follow: None,
            },
        );

        let field = placed
            .field
            .unwrap_or_else(|| panic!("{width}: the field is not drawn in the page's own width"));
        assert!(
            field.width() >= actionbar::FIELD_MIN_W,
            "{width}: the field is below its floor: {field:?}"
        );
        assert!(
            field.left() >= bar.left() && field.right() <= bar.right(),
            "{width}: the field leaves the row: {field:?} against {bar:?}"
        );

        let mut boxes = vec![
            ("play", placed.play),
            ("field", field),
            ("more", placed.more),
        ];
        if let Some(shuffle) = placed.shuffle {
            boxes.push(("shuffle", shuffle));
        }
        if let Some(heart) = placed.heart {
            boxes.push(("heart", heart));
        }
        if let Some(count) = count(&placed, kind, 120.0) {
            boxes.push(("count", count));
            assert!(
                count.left() >= placed.play.right() + actionbar::GAP,
                "{width}: the count reaches into the play tile: {count:?}"
            );
        }
        for (index, (name, rect)) in boxes.iter().enumerate() {
            assert!(
                bar.contains_rect(*rect),
                "{width}: {name} leaves the row: {rect:?} against {bar:?}"
            );
            for (other, against) in &boxes[index + 1..] {
                assert!(
                    !rect.intersects(*against),
                    "{width}: {name} {rect:?} overlaps {other} {against:?}"
                );
            }
        }
        width += 1.0;
    }
}
