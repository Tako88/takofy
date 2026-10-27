use super::*;
use crate::theme::Tokens;
use crate::view::test_support::{ring_shapes, rings_in, tab};
use egui::{Context, Event, Pos2, RawInput, Rect, Ui, Vec2};

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
        events,
        ..Default::default()
    }
}

/// Runs one pass with the focus frame opened, returning what it painted.
fn painted(ctx: &Context, input: RawInput, mut draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        draw(ui);
    });
    output.textures_delta.clear();
    output
}

/// Runs one pass and drops what it painted.
fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
    let _ = painted(ctx, input, draw);
}

/// The same input at a known instant, so an eased hover can finish.
fn timed(events: Vec<Event>, time: f64) -> RawInput {
    RawInput {
        time: Some(time),
        ..input(events)
    }
}

/// The tile's box in the tile tests.
fn tile_rect() -> Rect {
    Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::splat(extent::BUTTON_LG))
}

/// The fill a frame painted the tile with.
fn tile_fill(output: &egui::FullOutput, rect: Rect) -> Color32 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.rect == rect => Some(shape.fill),
            _ => None,
        })
        .expect("the tile paints a square")
}

/// One frame of the tile carrying `glyph`.
fn tile_frame(ctx: &Context, input: RawInput, glyph: Icon, live: bool) -> egui::FullOutput {
    let rect = tile_rect();
    let mut output = ctx.run_ui(input, |ui| {
        let _ = play_tile(
            ui,
            &Tokens::DARK,
            Control::PlayPause,
            rect,
            glyph,
            "Play",
            live,
        );
    });
    output.textures_delta.clear();
    output
}

/// The slot every ring test draws: one ready toggle in `rect`.
fn toggle(ui: &mut Ui, rect: Rect) -> Slot {
    slot(
        ui,
        Control::DrawerToggle,
        Role::Button,
        rect,
        "Toggle",
        &Tokens::DARK,
        true,
    )
}

fn cell(index: f32) -> Rect {
    Rect::from_min_size(Pos2::new(index * 40.0, 0.0), Vec2::new(28.0, 28.0))
}

#[test]
fn a_ready_control_is_focusable() {
    let ctx = Context::default();
    let mut sense = None;
    run(&ctx, input(Vec::new()), |ui| {
        let slot = slot(
            ui,
            Control::DrawerToggle,
            Role::Button,
            cell(0.0),
            "Toggle",
            &Tokens::DARK,
            true,
        );
        sense = Some(slot.response.sense);
    });
    let sense = sense.expect("the slot drew");
    assert!(sense.senses_click());
    assert!(sense.is_focusable());
}

#[test]
fn a_row_ring_stays_inside_its_row() {
    let ctx = Context::default();
    let row = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(200.0, 40.0));
    let output = painted(&ctx, input(Vec::new()), |ui| {
        let response = ui.interact(row, egui::Id::unique("row"), Sense::click());
        response.request_focus();
        focus_ring(ui, &response, row, radius::SM, &Tokens::DARK);
    });
    let rings = rings_in(&output);
    assert_eq!(rings, [row], "the ring lies on the row's own edge");
}

#[test]
fn a_rings_corners_follow_the_radius_it_is_given() {
    let ctx = Context::default();
    let rect = cell(0.0);
    let output = painted(&ctx, input(Vec::new()), |ui| {
        let response = ui.interact(rect, egui::Id::unique("tile"), Sense::click());
        response.request_focus();
        focus_ring(ui, &response, rect, radius::MD, &Tokens::DARK);
    });
    let strokes = ring_shapes(&output, &Tokens::DARK);
    assert_eq!(strokes.len(), 1, "one focused widget wears one ring");
    assert_eq!(strokes[0].corner_radius, radius::MD);
    assert_eq!(strokes[0].stroke_kind, StrokeKind::Inside);
}

#[test]
fn a_controls_ring_keeps_its_offset_from_the_control() {
    let ctx = Context::default();
    let rect = cell(0.0);
    let output = painted(&ctx, input(Vec::new()), |ui| {
        let id = slot_id(ui, Control::DrawerToggle, ());
        ui.ctx().memory_mut(|memory| memory.request_focus(id));
        let _ = toggle(ui, rect);
    });
    let rings = rings_in(&output);
    assert_eq!(rings.len(), 1, "the focused control wears one ring");
    assert_eq!(
        rings[0],
        rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE),
        "the ring keeps the band the control always had"
    );
}

#[test]
fn a_clicked_control_wears_no_ring() {
    let ctx = Context::default();
    let rect = cell(0.0);
    let at = rect.center();
    let _ = painted(&ctx, input(Vec::new()), |ui| {
        let _ = toggle(ui, rect);
    });
    let mut focused = None;
    for event in crate::view::test_support::click_events(at) {
        let _ = painted(&ctx, input(vec![event]), |ui| {
            let clicked = toggle(ui, rect);
            if clicked.response.clicked() {
                clicked.response.request_focus();
            }
            focused = Some(clicked.response.id);
        });
    }
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        focused,
        "the click took the keyboard"
    );
    let output = painted(&ctx, input(Vec::new()), |ui| {
        let _ = toggle(ui, rect);
    });
    assert!(
        rings_in(&output).is_empty(),
        "a clicked control wears no ring"
    );
}

#[test]
fn a_tabbed_control_wears_the_ring() {
    let ctx = Context::default();
    let rect = cell(0.0);
    let draw = |ui: &mut Ui| {
        let _ = toggle(ui, rect);
    };
    let _ = painted(&ctx, input(Vec::new()), draw);
    let [press, release] = tab();
    let _ = painted(&ctx, input(vec![press]), draw);
    let output = painted(&ctx, input(vec![release]), draw);

    let rings = rings_in(&output);
    assert_eq!(rings.len(), 1, "the tabbed control wears the ring");
    assert_eq!(
        rings[0],
        rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE)
    );
}

/// A control whose view exists can still be unable to act — nothing is queued,
/// no track is loaded. It is offered exactly as a waiting one is: if it were
/// not, it would take Tab and announce itself as enabled while doing nothing.
#[test]
fn a_momentarily_inert_control_is_offered_like_a_waiting_one() {
    let ctx = Context::default();
    let mut offered = None;
    run(&ctx, input(Vec::new()), |ui| {
        let live = slot(
            ui,
            Control::DrawerToggle,
            Role::Button,
            cell(0.0),
            "Toggle",
            &Tokens::DARK,
            true,
        );
        let inert = slot(
            ui,
            Control::DrawerToggle,
            Role::Button,
            cell(1.0),
            "Toggle",
            &Tokens::DARK,
            false,
        );
        offered = Some((live.live, inert.live, inert.response.sense));
    });
    let (live, inert, sense) = offered.expect("both slots drew");
    assert!(live, "a control that can act is live");
    assert!(!inert, "one that cannot is not");
    assert!(!sense.senses_click());
    assert!(!sense.is_focusable());
}

/// A ready control the moment makes inert is offered as waiting is: announced
/// disabled, with the label of the thing that would happen.
#[test]
fn a_waiting_control_is_announced_disabled_with_its_label() {
    let ctx = Context::default();
    // AccessKit state exists only while the pass that built it is live.
    ctx.enable_accesskit();
    let mut node = None;
    run(&ctx, input(Vec::new()), |ui| {
        let slot = slot(
            ui,
            Control::PlayPause,
            Role::Button,
            cell(0.0),
            "Play",
            &Tokens::DARK,
            false,
        );
        assert!(ui.ctx().has_accesskit_node(slot.response.id));
        node = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
            (node.is_disabled(), node.label().map(str::to_owned))
        });
    });
    let (disabled, label) = node.expect("the slot has an AccessKit node");
    assert!(disabled, "a waiting control must be announced disabled");
    assert_eq!(label.as_deref(), Some("Play"));
}

/// Which chip filters the list is a selection, so it has to reach the
/// accessibility tree and not only the fill: the fill says nothing to a
/// screen reader.
#[test]
fn a_pressed_chip_is_announced_selected() {
    let ctx = Context::default();
    // AccessKit state exists only while the pass that built it is live.
    ctx.enable_accesskit();
    let mut states = None;
    run(&ctx, input(Vec::new()), |ui| {
        let pressed = chip_in(
            ui,
            &Tokens::DARK,
            Control::FilterPlaylists,
            cell(0.0),
            "Playlists",
            true,
        );
        let idle = chip_in(
            ui,
            &Tokens::DARK,
            Control::FilterArtists,
            cell(1.0),
            "Artists",
            false,
        );
        let read = |id| {
            ui.ctx()
                .accesskit_node_builder(id, |node| node.is_selected())
                .expect("a chip has an AccessKit node")
        };
        states = Some((read(pressed.response.id), read(idle.response.id)));
    });
    let (pressed, idle) = states.expect("both chips drew");
    assert_eq!(
        pressed,
        Some(true),
        "the pressed chip is announced selected"
    );
    assert_eq!(
        idle,
        Some(false),
        "the chips that do not filter the list are announced not selected"
    );
}

#[test]
fn tab_never_lands_on_a_waiting_control() {
    let ctx = Context::default();
    // A ready control made inert by the moment, named by a control no other
    // slot in this test uses.
    let controls = [
        (Control::DrawerToggle, "Toggle", true),
        (Control::PlayPause, "Play", false),
        (Control::TrailPlaylists, "Playlists", true),
    ];
    let [tab, _] = tab();

    // Focus traversal reads the previous pass, and one Tab pass per widget.
    run(&ctx, input(Vec::new()), |_ui| {});
    let mut focused = Vec::new();
    for _ in 0..6 {
        let focused = &mut focused;
        run(&ctx, input(vec![tab.clone()]), |ui| {
            for (index, (control, label, live)) in controls.iter().enumerate() {
                let slot = slot(
                    ui,
                    *control,
                    Role::Button,
                    cell(index as f32),
                    label,
                    &Tokens::DARK,
                    *live,
                );
                if slot.response.has_focus() {
                    focused.push(*control);
                }
            }
        });
    }

    assert!(
        !focused.contains(&Control::PlayPause),
        "a control that cannot act took focus: {focused:?}"
    );
    assert!(focused.contains(&Control::DrawerToggle));
    assert!(focused.contains(&Control::TrailPlaylists));
}

#[test]
fn a_click_on_a_waiting_control_does_nothing() {
    let ctx = Context::default();
    let waiting = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(28.0, 28.0));
    let ready = Rect::from_min_size(Pos2::new(200.0, 100.0), Vec2::new(28.0, 28.0));
    let mut clicked = (false, false);

    // A press and release need several passes to register.
    let tap = |ctx: &Context, at: Pos2, clicked: &mut (bool, bool)| {
        for event in crate::view::test_support::click_events(at) {
            run(ctx, input(vec![event]), |ui| {
                let waiting_slot = slot(
                    ui,
                    Control::PlayPause,
                    Role::Button,
                    waiting,
                    "Play",
                    &Tokens::DARK,
                    false,
                );
                let ready_slot = slot(
                    ui,
                    Control::DrawerToggle,
                    Role::Button,
                    ready,
                    "Toggle",
                    &Tokens::DARK,
                    true,
                );
                clicked.0 = waiting_slot.response.clicked();
                clicked.1 = ready_slot.response.clicked();
            });
        }
    };

    // Warm up so hit-testing has a previous pass to read.
    run(&ctx, input(Vec::new()), |_ui| {});
    tap(&ctx, waiting.center(), &mut clicked);
    assert!(!clicked.0, "a waiting control received a click");
    assert!(!clicked.1, "the waiting rect is not the ready one");

    tap(&ctx, ready.center(), &mut clicked);
    assert!(clicked.1, "the click harness never produced a click");
}

#[test]
fn keyed_slots_on_one_control_are_distinct_per_key() {
    let ctx = Context::default();
    let keys = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let mut ids = Vec::new();
    run(&ctx, input(Vec::new()), |ui| {
        for (index, key) in keys.iter().enumerate() {
            let slot = slot_keyed(
                ui,
                Control::TrackLike,
                *key,
                Role::Button,
                cell(index as f32),
                "Like",
                &Tokens::DARK,
                true,
            );
            ids.push(slot.response.id);
        }
    });
    assert_eq!(ids.len(), keys.len());
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert!(
                ids[i] != ids[j],
                "keys {} and {} share an id",
                keys[i],
                keys[j]
            );
        }
    }
}

#[test]
fn the_unit_key_reproduces_the_existing_slot_id() {
    let ctx = Context::default();
    let mut ids = (None, None, None);
    run(&ctx, input(Vec::new()), |ui| {
        let keyed = slot_keyed(
            ui,
            Control::TrackLike,
            (),
            Role::Button,
            cell(0.0),
            "Like",
            &Tokens::DARK,
            true,
        );
        let plain = slot(
            ui,
            Control::TrackLike,
            Role::Button,
            cell(1.0),
            "Like",
            &Tokens::DARK,
            true,
        );
        let raw = ui.scope_id().with(Control::TrackLike);
        ids = (Some(keyed.response.id), Some(plain.response.id), Some(raw));
    });
    let (keyed, plain, raw) = ids;
    assert_eq!(keyed, plain, "slot no longer delegates with the unit key");
    assert_eq!(keyed, raw, "the keyed id moved the existing control ids");
}

#[test]
fn a_text_button_box_grows_with_its_label_and_pads_the_text() {
    let ctx = Context::default();
    let mut measured = None;
    run(&ctx, input(Vec::new()), |ui| {
        let short = text_button_size(ui, &Tokens::DARK, "Go");
        let long = text_button_size(ui, &Tokens::DARK, "Authorize");
        let bare = line(
            ui,
            "Authorize",
            text_button_font(),
            Tokens::DARK.palette.text,
            0.0,
        );
        measured = Some((short, long, bare.size().y));
    });
    let (short, long, bare) = measured.expect("the buttons measured");
    assert!(
        long.x > short.x,
        "a longer label must need a wider box: {long:?} vs {short:?}"
    );
    assert!(
        long.y > bare,
        "the box must be taller than its bare text: {long:?} vs {bare}"
    );
}

#[test]
fn a_text_button_that_is_not_live_takes_no_click() {
    let ctx = Context::default();
    let rect = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(120.0, 30.0));
    let mut clicked = false;

    // A press and release need several passes to register.
    let tap = |ctx: &Context, at: Pos2, clicked: &mut bool| {
        for event in crate::view::test_support::click_events(at) {
            run(ctx, input(vec![event]), |ui| {
                let slot = text_button(
                    ui,
                    &Tokens::DARK,
                    Control::Authorize,
                    rect,
                    "Authorize",
                    false,
                );
                *clicked = slot.response.clicked();
            });
        }
    };

    // Warm up so hit-testing has a previous pass to read.
    run(&ctx, input(Vec::new()), |_ui| {});
    tap(&ctx, rect.center(), &mut clicked);
    assert!(!clicked, "a waiting text button received a click");
}

#[test]
fn the_play_tile_paints_the_accent_fill_and_the_inverse_glyph_for_both_glyphs() {
    let tokens = Tokens::DARK;
    let rect = tile_rect();
    for glyph in [Icon::Play, Icon::Pause] {
        let ctx = Context::default();
        let output = tile_frame(&ctx, timed(Vec::new(), 0.0), glyph, true);
        assert_eq!(
            tile_fill(&output, rect),
            tokens.palette.accent,
            "{glyph:?}: the tile is not the accent"
        );
        let colours = glyph_colours(&output);
        assert!(!colours.is_empty(), "{glyph:?}: the tile paints no glyph");
        for colour in colours {
            assert_eq!(colour, tokens.palette.text_inverse, "{glyph:?}");
        }

        // The pointer's hover moves the fill toward the hovered accent.
        let _ = tile_frame(
            &ctx,
            timed(vec![Event::PointerMoved(rect.center())], 0.1),
            glyph,
            true,
        );
        let hovered = tile_frame(&ctx, timed(Vec::new(), 1.0), glyph, true);
        let fill = tile_fill(&hovered, rect);
        assert!(
            fill.g() > tokens.palette.accent.g() && fill.g() <= tokens.palette.accent_hover.g(),
            "{glyph:?}: the hovered tile is {fill:?}"
        );
    }
}

#[test]
fn a_tile_that_cannot_play_paints_the_disabled_fill() {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    let output = tile_frame(&ctx, timed(Vec::new(), 0.0), Icon::Play, false);
    assert_eq!(
        tile_fill(&output, tile_rect()),
        tokens.palette.text_disabled
    );
    let colours = glyph_colours(&output);
    assert!(!colours.is_empty(), "the inert tile paints no glyph");
    for colour in colours {
        assert_eq!(colour, tokens.palette.text_inverse);
    }
}

#[test]
fn fraction_at_clamps_to_the_track() {
    assert_eq!(fraction_at(100.0, 100.0, 200.0), 0.0);
    assert_eq!(fraction_at(150.0, 100.0, 200.0), 0.25);
    assert_eq!(fraction_at(300.0, 100.0, 200.0), 1.0);
    assert_eq!(fraction_at(400.0, 100.0, 200.0), 1.0);
    assert_eq!(fraction_at(50.0, 100.0, 200.0), 0.0);
    assert_eq!(fraction_at(150.0, 100.0, 0.0), 0.0);
}

/// The colours a frame put into glyph shapes. The wash and the claim's fill
/// are `Shape::Rect`, so only the two kinds a glyph emits are read and the
/// backgrounds are excluded by kind.
fn glyph_colours(output: &egui::FullOutput) -> Vec<Color32> {
    let mut colours = Vec::new();
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::Shape::Path(path) => {
                if path.fill != Color32::TRANSPARENT {
                    colours.push(path.fill);
                }
                if let egui::epaint::ColorMode::Solid(color) = &path.stroke.color
                    && path.stroke.width > 0.0
                {
                    colours.push(*color);
                }
            }
            egui::Shape::Circle(circle) => colours.push(circle.fill),
            _ => {}
        }
    }
    colours
}

#[test]
fn a_claimed_button_fills_with_the_selection_and_is_inert() {
    let tokens = Tokens::DARK;
    let rect = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::splat(28.0));

    let ctx = Context::default();
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let slot = icon_button_claimed(
            ui,
            &tokens,
            Control::Queue,
            rect,
            Icon::Queue,
            Size::MD,
            "Queue",
        );
        assert!(!slot.live, "a claimed button never acts");
        assert!(
            !slot.response.sense.senses_click(),
            "a claimed button takes no click; only the drop is live"
        );
    });
    output.textures_delta.clear();

    let fills: Vec<Rect> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.fill == tokens.palette.row_selected => {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(fills, vec![rect], "the claim fills exactly the button");

    let colours = glyph_colours(&output);
    assert!(!colours.is_empty(), "the claimed button paints its glyph");
    for color in colours {
        assert_eq!(color, tokens.palette.text, "a claimed glyph is lit");
        assert_ne!(color, tokens.palette.text_disabled);
    }

    // The same control with nothing to act on keeps the disabled tone: the
    // claim, not the control, is what lights the glyph.
    let plain = Context::default();
    let mut output = plain.run_ui(input(Vec::new()), |ui| {
        let _ = icon_button(
            ui,
            &tokens,
            Control::Queue,
            rect,
            Icon::Queue,
            Size::MD,
            "Queue",
            false,
        );
    });
    output.textures_delta.clear();
    let colours = glyph_colours(&output);
    assert!(!colours.is_empty(), "the plain button paints its glyph");
    for color in colours {
        assert_eq!(color, tokens.palette.text_disabled);
    }
}

/// A claimed button is inert by construction, so a control the availability
/// table lets act still takes no click and no focus ring while a carry claims
/// it.
#[test]
fn a_claimed_button_is_inert_even_when_its_control_is_ready() {
    assert_eq!(
        availability(Control::DrawerToggle),
        Availability::Ready,
        "the test needs a control the availability table lets act"
    );
    let ctx = Context::default();
    let mut offered = None;
    run(&ctx, input(Vec::new()), |ui| {
        let slot = icon_button_claimed(
            ui,
            &Tokens::DARK,
            Control::DrawerToggle,
            cell(0.0),
            Icon::Queue,
            Size::MD,
            "Toggle",
        );
        offered = Some((slot.live, slot.hover, slot.response.sense));
    });
    let (live, hover, sense) = offered.expect("the claimed button drew");
    assert!(!live, "a claimed button is never live");
    assert_eq!(hover, 0.0, "a claimed button has no hover to paint");
    assert!(!sense.senses_click());
    assert!(!sense.is_focusable());
}
