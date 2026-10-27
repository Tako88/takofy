use super::super::*;
use super::test_support::*;
use crate::layout::search_menu::row_rect;
use crate::menu::{Device, Item, Kind, Verb};
use crate::theme::extent;
use egui::{Event, Rect, Shape, pos2, vec2};

/// Whether a glyph, and not a line of text, was painted into `rect`.
fn glyph_into(output: &egui::FullOutput, rect: Rect) -> bool {
    output.shapes.iter().any(|clipped| {
        matches!(clipped.shape, Shape::Path(_))
            && clipped.shape.visual_bounding_rect().intersects(rect)
    })
}

/// A search state with one answer, open the way a click on the field opens
/// it.
fn answered_search() -> crate::search::Search {
    let mut search = crate::search::Search::default();
    search.text = "a".to_owned();
    search.text_changed();
    search.apply(&crate::message::Update::Search {
        query: "a".to_owned(),
        tracks: vec![track()],
    });
    let _ = search.step(crate::search::KeyInput {
        focused: true,
        clicked_field: true,
        ..crate::search::KeyInput::default()
    });
    assert!(search.is_open(), "the search menu is drawn");
    search
}

fn field() -> Rect {
    Rect::from_min_size(pos2(40.0, 12.0), vec2(320.0, extent::SEARCH_H))
}

/// Draws one frame of the search drop-down, returning the frame that paints.
fn search_frame(ctx: &egui::Context, search: &mut crate::search::Search) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut art = crate::cover::Art::default();
    let mut output = None;
    for pass in 0..2 {
        art.begin_frame();
        let mut drawn = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = crate::view::search_menu::show(
                ui,
                field(),
                search,
                &crate::player::Transport::default(),
                &mut art,
                std::time::Instant::now(),
                &tokens,
            );
        });
        drawn.textures_delta.clear();
        if pass == 1 {
            output = Some(drawn);
        }
    }
    output.expect("two passes")
}

#[test]
fn the_box_is_the_search_menus_box() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let search_output = search_frame(&ctx, &mut answered_search());

    let tokens = Tokens::DARK;
    let box_of = |output: &egui::FullOutput| {
        let rects: Vec<&egui::epaint::RectShape> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(shape) => Some(shape),
                _ => None,
            })
            .collect();
        let shadow = rects
            .iter()
            .find(|shape| shape.blur_width > 0.0)
            .expect("a shadow");
        let fill = rects
            .iter()
            .find(|shape| shape.fill == tokens.palette.surface_raised)
            .expect("a fill");
        let hairline = rects
            .iter()
            .find(|shape| shape.stroke.color == tokens.palette.border_strong)
            .expect("a hairline");
        (
            shadow.blur_width,
            shadow.fill,
            fill.corner_radius,
            hairline.corner_radius,
            hairline.stroke.width,
        )
    };
    assert_eq!(
        box_of(&drawn.output),
        box_of(&search_output),
        "the two menus are not one box"
    );
}

#[test]
fn the_menu_draws_above_the_search_drop_down() {
    // The shell draws the drop-down first and the context menu after it, so
    // both hang in the foreground layer and this one is painted last.
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut search = answered_search();
    let mut menu = opened(Device::Pointer);
    let mut art = crate::cover::Art::default();
    let mut output = None;
    for pass in 0..2 {
        art.begin_frame();
        let mut drawn = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = crate::view::search_menu::show(
                ui,
                field(),
                &mut search,
                &crate::player::Transport::default(),
                &mut art,
                std::time::Instant::now(),
                &tokens,
            );
            let _ = show(
                ui,
                Frame {
                    menu: &mut menu,
                    free: true,
                    tokens: &tokens,
                },
            );
        });
        drawn.textures_delta.clear();
        if pass == 1 {
            output = Some(drawn);
        }
    }
    let output = output.expect("two passes");

    let cards: Vec<(usize, Rect)> = output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(index, clipped)| match &clipped.shape {
            Shape::Rect(shape)
                if shape.fill == tokens.palette.surface_raised
                    && shape.rect.width() > extent::MENU_COVER =>
            {
                Some((index, shape.rect))
            }
            _ => None,
        })
        .collect();
    assert_eq!(cards.len(), 2, "both menus are drawn: {cards:?}");
    let drop_down = cards
        .iter()
        .min_by(|left, right| left.1.top().total_cmp(&right.1.top()))
        .expect("a drop-down");
    let context = cards
        .iter()
        .max_by(|left, right| left.1.top().total_cmp(&right.1.top()))
        .expect("a context menu");
    assert!(
        drop_down.0 < context.0,
        "the context menu is painted under the drop-down"
    );
}

#[test]
fn a_pointer_open_paints_no_rule_and_no_wash() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let tokens = Tokens::DARK;
    for wash in [tokens.palette.row_selected, tokens.palette.row_hover] {
        assert!(
            fills_of(&drawn.output, wash).is_empty(),
            "a pointer open painted a tier: {wash:?}"
        );
    }
    let rule = fills_of(&drawn.output, tokens.palette.text)
        .into_iter()
        .filter(|rect| rect.width() == crate::theme::stroke::RULE);
    assert_eq!(
        rule.count(),
        0,
        "a pointer open painted the keyboard's rule"
    );
}

#[test]
fn a_keyboard_open_paints_the_rule_and_the_selected_wash_on_row_0_only() {
    let ctx = context();
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let tokens = Tokens::DARK;
    let card = card(&drawn.output);

    let washes = fills_of(&drawn.output, tokens.palette.row_selected);
    assert_eq!(washes, vec![row_rect(card, 0)], "the wash is not on row 0");

    let rules: Vec<Rect> = fills_of(&drawn.output, tokens.palette.text)
        .into_iter()
        .filter(|rect| rect.width() == crate::theme::stroke::RULE)
        .collect();
    assert_eq!(rules.len(), 1, "the keyboard's rule is drawn once");
    assert_eq!(rules[0].top(), row_rect(card, 0).top());
    assert_eq!(rules[0].height(), extent::ROW_H);
    assert!(
        fills_of(&drawn.output, tokens.palette.row_hover).is_empty(),
        "a keyboard open painted the pointer's wash"
    );
}

#[test]
fn the_pointer_tier_paints_wash_and_tick_only_under_the_pointer() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let card = card(&drawn.output);
    let row = row_rect(card, 1);
    let tokens = Tokens::DARK;

    // The pointer arrives, and the frame after it draws the tier.
    frame(
        &ctx,
        &mut menu,
        true,
        vec![Event::PointerMoved(row.center())],
    );
    let drawn = frame(
        &ctx,
        &mut menu,
        true,
        vec![Event::PointerMoved(row.center())],
    );

    assert_eq!(
        fills_of(&drawn.output, tokens.palette.row_hover),
        vec![row],
        "the pointer's wash is not on the row under the pointer"
    );
    let ticks: Vec<Rect> = fills_of(&drawn.output, tokens.palette.text_muted)
        .into_iter()
        .filter(|rect| rect.width() == crate::theme::stroke::RULE)
        .collect();
    assert_eq!(ticks.len(), 1, "the pointer's tier is one tick");
    assert_eq!(ticks[0].center().y, row.center().y);
    assert!(
        fills_of(&drawn.output, tokens.palette.row_selected).is_empty(),
        "the pointer's tier borrowed the keyboard's wash"
    );
}

#[test]
fn rows_are_named_menu_items_and_the_card_is_a_menu() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (_, nodes) = draw(&ctx, &mut menu, true, Vec::new(), |ui, area| {
        let surface = ui
            .ctx()
            .accesskit_node_builder(area.with("surface"), |node| {
                (node.role(), node.label().map(str::to_owned))
            })
            .expect("the card is a node");
        let rows: Vec<_> = (0..4)
            .map(|index| {
                ui.ctx()
                    .accesskit_node_builder(area.with(("row", index)), |node| {
                        (node.role(), node.label().map(str::to_owned))
                    })
                    .expect("a row is a node")
            })
            .collect();
        (surface, rows)
    });

    assert_eq!(nodes.0.0, egui::Role::Menu);
    assert_eq!(nodes.0.1.as_deref(), Some("Harbour Wall"));
    let expected = ["Add to queue", "Go to album", "Go to artist", "Copy link"];
    assert_eq!(nodes.1.len(), expected.len());
    for ((role, label), name) in nodes.1.into_iter().zip(expected) {
        assert_eq!(role, egui::Role::MenuItem);
        assert_eq!(label.as_deref(), Some(name));
    }
}

#[test]
fn a_destructive_row_is_full_ink_with_a_hairline_above_it() {
    let ctx = context();
    let items = vec![
        Item {
            label: "New playlist…".to_owned(),
            kind: Kind::Plain,
            key: None,
            submenu: false,
            verb: Verb::AddToQueue,
        },
        Item {
            label: "Remove from this playlist".to_owned(),
            kind: Kind::Destructive,
            key: None,
            submenu: false,
            verb: Verb::CopyLink,
        },
    ];
    let mut menu = opened_with(Device::Pointer, items);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let tokens = Tokens::DARK;
    let card = card(&drawn.output);
    let destructive = row_rect(card, 1);

    let (_, color) = texts(&drawn.output)
        .into_iter()
        .find(|(text, _)| text == "Remove from this playlist")
        .expect("the destructive label is painted");
    assert_eq!(color, tokens.palette.text, "the command is not full ink");

    let (_, plain) = texts(&drawn.output)
        .into_iter()
        .find(|(text, _)| text == "New playlist…")
        .expect("the plain label is painted");
    assert_eq!(
        plain, tokens.palette.text_muted,
        "a resting command is not one tier below the ink"
    );

    let hairline = Rect::from_min_size(
        destructive.min,
        vec2(destructive.width(), crate::theme::stroke::HAIRLINE),
    );
    assert!(
        fills_of(&drawn.output, tokens.palette.border).contains(&hairline),
        "the destructive row has no hairline above it"
    );
}

#[test]
fn a_key_hint_draws_only_when_the_item_has_one() {
    let ctx = context();
    let hinted = vec![Item {
        label: "Remove from this playlist".to_owned(),
        kind: Kind::Destructive,
        key: Some("Del"),
        submenu: false,
        verb: Verb::CopyLink,
    }];
    let mut menu = opened_with(Device::Pointer, hinted);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    assert!(
        texts(&drawn.output).iter().any(|(text, _)| text == "Del"),
        "the key hint is not drawn"
    );

    let bare = vec![Item {
        label: "Remove from this playlist".to_owned(),
        kind: Kind::Destructive,
        key: None,
        submenu: false,
        verb: Verb::CopyLink,
    }];
    let mut menu = opened_with(Device::Pointer, bare);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    assert!(
        !texts(&drawn.output).iter().any(|(text, _)| text == "Del"),
        "a hint for an unbound key was drawn"
    );
}

#[test]
fn a_chevron_draws_only_for_a_submenu_item() {
    let ctx = context();
    let submenu = vec![Item {
        label: "Add to playlist".to_owned(),
        kind: Kind::Plain,
        key: None,
        submenu: true,
        verb: Verb::AddToQueue,
    }];
    let mut menu = opened_with(Device::Pointer, submenu);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let card = card(&drawn.output);
    let row = row_rect(card, 0);
    let chevron = Rect::from_min_size(
        pos2(
            row.right() - crate::theme::space::S3 - crate::theme::icon::Size::SM.side,
            row.center().y - crate::theme::icon::Size::SM.side / 2.0,
        ),
        vec2(
            crate::theme::icon::Size::SM.side,
            crate::theme::icon::Size::SM.side,
        ),
    );
    assert!(
        glyph_into(&drawn.output, chevron),
        "the submenu row drew no chevron"
    );

    let plain = vec![Item {
        label: "Add to playlist".to_owned(),
        kind: Kind::Plain,
        key: None,
        submenu: false,
        verb: Verb::AddToQueue,
    }];
    let mut menu = opened_with(Device::Pointer, plain);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    assert!(
        !glyph_into(&drawn.output, chevron),
        "a row with no submenu drew a chevron"
    );
}

#[test]
fn a_long_label_elides_on_one_line() {
    let ctx = context();
    let long = vec![Item {
        label: "Add all twelve tracks from this record to the end of the queue".to_owned(),
        kind: Kind::Plain,
        key: None,
        submenu: false,
        verb: Verb::AddToQueue,
    }];
    let mut menu = opened_with(Device::Pointer, long);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());

    let galley = drawn
        .output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.clone()),
            _ => None,
        })
        .expect("the label is painted");
    assert!(galley.elided, "the long label is not elided");
    assert_eq!(galley.rows.len(), 1, "the label wrapped onto a second line");
    assert_eq!(
        card(&drawn.output).width(),
        extent::MENU_MAX_W,
        "a label longer than the cap widened the menu past it"
    );
}
