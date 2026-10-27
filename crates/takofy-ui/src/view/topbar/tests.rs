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

/// Home is the library root, so on a page its crumb leads back there.
#[test]
fn the_home_crumb_is_a_way_back_to_the_root() {
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

    let (index, target) = back.expect("the Home crumb did not ask for the root");
    assert_eq!(index, 0, "Home is the first crumb");
    assert_eq!(target, Target::Root(RootKind::All));
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
