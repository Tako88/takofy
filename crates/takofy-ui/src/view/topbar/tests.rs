use super::*;

/// A later `widget_info` on the text edit's own id refines the role egui's
/// builder gave it. Read inside the pass, because that is when an AccessKit
/// node exists.
#[test]
fn the_field_announces_itself_as_a_search_input() {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));
    ctx.enable_accesskit();

    let mut library = Library::default();
    let content = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
    let regions = layout::regions_for(content.width());
    let shell = layout::shell_for(&regions);
    let bar = layout::grid_for(content, &shell).topbar;

    let mut role = None;
    let input = egui::RawInput {
        screen_rect: Some(content),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        let topbar = show(ui, &mut library, regions.sidebar, bar, &tokens);
        assert!(topbar.search_field.is_some(), "the field is drawn");
        let id = ui.scope_id().with((Control::SearchField, ()));
        role = ui.ctx().accesskit_node_builder(id, |node| node.role());
    });
    output.textures_delta.clear();
    assert_eq!(role, Some(egui::Role::SearchInput));
}

/// Home is a page of its own, so on another page its crumb leads there.
#[test]
fn the_home_crumb_is_a_way_back_home() {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));

    let mut library = Library::default();
    library.open(crate::route::Page::Album("spotify:album:a".to_owned()));
    let crumbs = crate::trail::trail(&library);

    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 40.0));
    let at = Pos2::new(rect.left() + 2.0, rect.center().y);
    let mut back = None;
    for event in crate::view::test_support::click_events(at) {
        let input = egui::RawInput {
            screen_rect: Some(rect),
            events: vec![event],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            if let Some(clicked) = trail(ui, &crumbs, rect, &tokens) {
                back = Some(clicked);
            }
        });
        output.textures_delta.clear();
    }

    let (index, target) = back.expect("the Home crumb did not ask for Home");
    assert_eq!(index, 0, "Home is the first crumb");
    assert_eq!(target, Target::Page(crate::route::Page::Home));
}

/// The magnifier sits at the field's left edge, so the text has to start
/// after it. `TextEdit::margin` is ignored when a frame is given, which is
/// how the text came to be painted over the glyph — this reads the painted
/// galley rather than the numbers that build it.
#[test]
fn the_text_starts_after_the_magnifier() {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));

    let mut library = Library::default();
    library.search.text = "harbour".to_owned();
    let content = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
    let regions = layout::regions_for(content.width());
    let shell = layout::shell_for(&regions);
    let bar = layout::grid_for(content, &shell).topbar;
    let input = egui::RawInput {
        screen_rect: Some(content),
        ..Default::default()
    };
    // Two passes: the first sizes the edit, the second paints the text.
    let mut field = None;
    let mut first = ctx.run_ui(input.clone(), |ui| {
        let topbar = show(ui, &mut library, regions.sidebar, bar, &tokens);
        field = topbar.search_field.map(|drawn| drawn.rect);
    });
    first.textures_delta.clear();
    let mut output = ctx.run_ui(input, |ui| {
        let topbar = show(ui, &mut library, regions.sidebar, bar, &tokens);
        field = topbar.search_field.map(|drawn| drawn.rect);
    });
    output.textures_delta.clear();
    let field = field.expect("the field is drawn");

    fn text_at(shapes: &[egui::epaint::ClippedShape], wanted: &str) -> Option<f32> {
        fn walk(shape: &egui::Shape, wanted: &str, found: &mut Option<f32>) {
            match shape {
                egui::Shape::Text(text) => {
                    if text.galley.job.text == wanted {
                        *found = Some(text.pos.x);
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, wanted, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = None;
        for clipped in shapes {
            walk(&clipped.shape, wanted, &mut found);
        }
        found
    }

    let x = text_at(&output.shapes, "harbour").expect("the typed text is painted");
    let after_glyph = field.left() + space::S3 + Size::MD.side + space::S2;
    assert!(
        x >= after_glyph - 0.5,
        "the text starts at {x}, inside the magnifier (glyph ends at {after_glyph})"
    );
    assert!(
        x <= after_glyph + space::S5,
        "the text starts at {x}, far past the glyph"
    );
}

#[test]
fn the_placeholder_promises_artists_albums_and_tracks() {
    assert_eq!(PLACEHOLDER, "Artists, albums or tracks");
}

/// The bar the cluster tests draw into: wide enough that the layout leaves the
/// field its full width.
fn bar_rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 40.0))
}

/// The nav cluster's squares, left to right, with the border each was framed
/// in, which is how the current marker is read.
fn cluster(output: &egui::FullOutput) -> Vec<(Rect, egui::Color32)> {
    let mut drawn = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.width() - extent::BUTTON).abs() < 0.01
                    && (shape.rect.height() - extent::BUTTON).abs() < 0.01
                    && shape.rect.bottom() <= extent::TOPBAR_H =>
            {
                Some((shape.rect, shape.stroke.color))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    drawn.sort_by(|left, right| left.0.left().total_cmp(&right.0.left()));
    drawn
}

/// The rule's mark as painted, found by its hairline width and its height.
fn drawn_rule(output: &egui::FullOutput, tokens: &Tokens) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.width() - stroke::HAIRLINE).abs() < 0.01
                    && (shape.rect.height() - space::S4).abs() < 0.01
                    && shape.fill == tokens.palette.border =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the cluster draws its rule")
}

fn assert_close(left: f32, right: f32, what: &str) {
    assert!((left - right).abs() < 0.01, "{what}: {left} is not {right}");
}

/// One frame of the topbar, with no input, so the buttons are drawn unhovered.
fn draw(library: &mut Library, tokens: &Tokens) -> egui::FullOutput {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(crate::theme::style(tokens));
    let input = egui::RawInput {
        screen_rect: Some(bar_rect()),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        let _ = show(ui, library, Sidebar::Full, bar_rect(), tokens);
    });
    output.textures_delta.clear();
    output
}

/// The nav a click at `pos` asks for, run through the same pass a real press
/// goes through.
fn nav_at(library: &mut Library, tokens: &Tokens, pos: Pos2) -> Option<Nav> {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(crate::theme::style(tokens));
    let mut nav = None;
    for event in crate::view::test_support::click_events(pos) {
        let input = egui::RawInput {
            screen_rect: Some(bar_rect()),
            events: vec![event],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let bar = show(ui, library, Sidebar::Full, bar_rect(), tokens);
            if bar.nav.is_some() {
                nav = bar.nav;
            }
        });
        output.textures_delta.clear();
    }
    nav
}

/// The indexes of the cluster's squares framed as current, which is the
/// reference's `aria-current`.
fn current(buttons: &[(Rect, egui::Color32)], tokens: &Tokens) -> Vec<usize> {
    buttons
        .iter()
        .enumerate()
        .filter(|(_, (_, color))| *color == tokens.palette.border_strong)
        .map(|(index, _)| index)
        .collect()
}

#[test]
fn the_home_button_opens_home() {
    let tokens = Tokens::DARK;
    let mut library = Library::default();
    let buttons = cluster(&draw(&mut library, &tokens));
    assert_eq!(buttons.len(), 4, "Home, Your Library, back and forward");

    assert_eq!(
        nav_at(&mut library, &tokens, buttons[0].0.center()),
        Some(Nav::Home)
    );
}

#[test]
fn the_library_button_opens_the_root() {
    let tokens = Tokens::DARK;
    let mut library = Library::starting_home(0);
    let buttons = cluster(&draw(&mut library, &tokens));

    assert_eq!(
        nav_at(&mut library, &tokens, buttons[1].0.center()),
        Some(Nav::Library)
    );
}

#[test]
fn the_current_marker_is_on_home_on_home_and_on_the_library_at_the_root() {
    let tokens = Tokens::DARK;

    let mut home = Library::starting_home(0);
    let on_home = cluster(&draw(&mut home, &tokens));
    assert_eq!(current(&on_home, &tokens), vec![0], "Home is where you are");

    let mut root = Library::default();
    let at_root = cluster(&draw(&mut root, &tokens));
    assert_eq!(
        current(&at_root, &tokens),
        vec![1],
        "Your Library is where you are"
    );
}

/// The rule's slot is the width the layout reserves for it: the mark's `space-1`
/// margin on either side plus the `space-1` gap before and after the slot. Read
/// on the drawn buttons, so a slot that grew or shrank shows up as the two
/// margins failing to land on them.
#[test]
fn the_rule_sits_in_the_slot_the_layout_reserves() {
    let tokens = Tokens::DARK;
    let mut library = Library::default();
    let output = draw(&mut library, &tokens);
    let buttons = cluster(&output);
    assert_eq!(buttons.len(), 4);
    let rule = drawn_rule(&output, &tokens);

    let before = buttons[1].0.right();
    let after = buttons[2].0.left();
    assert_close(
        after - before,
        layout::HISTORY_RULE + 2.0 * space::S1,
        "the slot and its two gaps",
    );
    assert_close(
        rule.left() - before,
        2.0 * space::S1,
        "the gap and the margin before the mark",
    );
    assert_close(
        after - rule.right(),
        2.0 * space::S1,
        "the margin and the gap after the mark",
    );
    assert_close(rule.width(), stroke::HAIRLINE, "the mark stays a hairline");
}

#[test]
fn the_separator_stands_between_the_destinations_and_the_history_pair() {
    let tokens = Tokens::DARK;
    let mut library = Library::default();
    let output = draw(&mut library, &tokens);
    let buttons = cluster(&output);
    let rule = drawn_rule(&output, &tokens);

    assert!(
        rule.center().x > buttons[1].0.right(),
        "the rule is clear of Your Library"
    );
    assert!(
        rule.center().x < buttons[2].0.left(),
        "the rule is clear of Back"
    );
    assert!(
        (rule.center().y - bar_rect().center().y).abs() < 0.01,
        "the rule is centred on the row"
    );
}
