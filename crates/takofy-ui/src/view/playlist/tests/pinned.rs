use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

/// One frame of the tracklist with a wheel jump applied in the frame it
/// arrives, as the window's own draw pass does: `view::show` calls
/// `scroll::instant` before any scroll area.
fn instant_frame_in(
    ctx: &Context,
    size: Vec2,
    library: &mut Library,
    events: Vec<Event>,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    run(ctx, sized_input(size, events), |ui| {
        crate::scroll::instant(ui.ctx());
        let _ = tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    })
}

/// The filter field's drawn box at any width, read from the line under its
/// query.
///
/// A condensed page's field is wider than `filter_hairline`'s upper bound, so
/// the tests that check it pick the hairline below the field's own text
/// rather than the first one on the page.
fn field_box_under(output: &egui::FullOutput, label: &str) -> Rect {
    let tokens = Tokens::DARK;
    let text = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("the field paints its query");
    let line = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.height() - crate::theme::stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.top() >= text.bottom() - 0.01
                    && (shape.fill == tokens.palette.border
                        || shape.fill == tokens.palette.text) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .min_by(|left, right| left.top().total_cmp(&right.top()))
        .expect("the field paints its bottom hairline under its query");
    Rect::from_min_max(
        Pos2::new(line.left(), line.bottom() - extent::BUTTON_LG),
        Pos2::new(line.right(), line.bottom()),
    )
}

#[test]
fn the_filter_field_takes_enter_and_leaves_it_alone() {
    let ctx = context();
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let tokens = Tokens::DARK;

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

    let mut command = None;
    let output = run(&ctx, sized_input(ALL, vec![key(egui::Key::Enter)]), |ui| {
        command = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    });

    assert_eq!(command, None, "Enter in the field plays nothing");
    assert_eq!(painted_rows(&output), MIDNIGHT.to_vec());
    assert_eq!(library.track_filter.edit(PLAYLIST).as_str(), "midnight");
    // Enter is consumed and nothing else moves: the field keeps its text and
    // the keyboard it was given, so the hairline still shows the focus.
    assert!(
        drawn_texts(&output).contains(&"midnight"),
        "the field keeps its text: {:?}",
        drawn_texts(&output)
    );
    assert!(
        ctx.text_edit_focused(),
        "Enter leaves the keyboard in the field"
    );
    assert_eq!(
        filter_hairline(&output).1,
        tokens.palette.text,
        "the hairline still shows the field's focus"
    );
}

/// The top of the list's own viewport: `tracks_view` starts flush with the
/// window's top edge, and the scroll area fills what is left of it.
const VIEWPORT_TOP: f32 = 0.0;

/// Where a row that pins under the action row comes to rest: the viewport's
/// top edge plus the height the action row condenses to.
const PIN_TOP: f32 = VIEWPORT_TOP + crate::theme::actionbar::STUCK_H;

/// Every box a text shape with `label` was painted in, in paint order.
fn text_rects(output: &egui::FullOutput, label: &str) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// The AccessKit node a frame announced for `label` inside `rect`.
fn announced_in(
    output: &egui::FullOutput,
    label: &str,
    rect: Rect,
) -> Option<egui::accesskit::NodeId> {
    let update = output.platform_output.accesskit_update.as_ref()?;
    update.nodes.iter().find_map(|(id, node)| {
        let bounds = node.bounds()?;
        let bounds = Rect::from_min_max(
            Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
            Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
        );
        (node.label() == Some(label) && bounds.intersects(rect)).then_some(*id)
    })
}

/// Whether a frame's AccessKit update still contains `id`.
fn announced(output: &egui::FullOutput, id: egui::accesskit::NodeId) -> bool {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .is_some_and(|update| update.nodes.iter().any(|(node, _)| *node == id))
}

/// The action row's box, read from the surface it fills once it pins.
///
/// The row's condensed height is the only fill of this tone on the page that
/// tall, so a pinned frame has exactly one.
fn pinned_bar(output: &egui::FullOutput) -> Option<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == tokens.palette.surface
                    && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// The field's box on a stuck frame, solved from the row's own drawn box: a
/// stuck field paints no hairline of its own to read it from.
fn stuck_field(output: &egui::FullOutput, size: Vec2) -> Rect {
    let bar = pinned_bar(output).expect("the row is pinned");
    crate::layout::actionbar::solve(
        bar,
        crate::layout::actionbar::Kind::Playlist(crate::layout::actionbar::Form::at(
            size.x,
            Table::Full,
        )),
        crate::layout::actionbar::Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    )
    .field
    .expect("the pinned row has a field")
}

/// One frame with the wheel moved by `points` at `at`, which is how the
/// list scrolls.
fn wheel(
    ctx: &Context,
    size: Vec2,
    at: Pos2,
    points: f32,
    library: &mut Library,
) -> egui::FullOutput {
    wheel_in(ctx, size, Table::Full, at, points, library)
}

/// The same, on a page handed `table`.
fn wheel_in(
    ctx: &Context,
    size: Vec2,
    table: Table,
    at: Pos2,
    points: f32,
    library: &mut Library,
) -> egui::FullOutput {
    table_frame_in(
        ctx,
        size,
        table,
        library,
        vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, points),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            },
        ],
    )
}

/// Wheels down until the action row pins, answering the frame it pinned on.
fn wheel_until_pinned(
    ctx: &Context,
    size: Vec2,
    table: Table,
    library: &mut Library,
) -> Option<egui::FullOutput> {
    let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
    for _ in 0..240 {
        let output = wheel_in(ctx, size, table, at, -extent::ROW_H, library);
        if pinned_bar(&output).is_some() {
            return Some(output);
        }
    }
    None
}

/// The first frame whose action row is pinned to the top of the list.
fn pinned_frame(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
    pinned_frame_in(ctx, size, Table::Full, library)
}

/// The same, on a page handed `table`.
fn pinned_frame_in(
    ctx: &Context,
    size: Vec2,
    table: Table,
    library: &mut Library,
) -> egui::FullOutput {
    wheel_until_pinned(ctx, size, table, library).unwrap_or_else(|| {
        panic!(
            "the action row never pinned in a {}x{} window",
            size.x, size.y
        )
    })
}

#[test]
fn the_bar_pins_to_the_top_of_the_list() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    // Short enough that sixty-two rows scroll well past the band.
    let size = Vec2::new(900.0, 400.0);

    let layout = tracklist_frame_in(&ctx, size, &mut library);
    assert!(
        pinned_bar(&layout).is_none(),
        "the row starts in its place in the flow"
    );
    let flow = field_at(&layout);
    assert!(
        flow.top() > VIEWPORT_TOP,
        "the field starts below the viewport top: {flow:?}"
    );

    let pinned = pinned_frame(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    assert!(
        (bar.top() - VIEWPORT_TOP).abs() < 0.01,
        "the row sits at the viewport's top edge: {bar:?}"
    );
    assert!(
        (bar.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01,
        "the row condenses to the height the rows clear: {bar:?}"
    );
    let field = stuck_field(&pinned, size);
    assert!(
        (field.center().y - bar.center().y).abs() < 0.01,
        "the field sits on the pinned row's centre line: {field:?} against {bar:?}"
    );

    // One field, one placeholder: a second copy left at the row's place in
    // the flow would be a second `FILTER`, above the pinned one.
    let placeholders: Vec<Rect> = text_rects(&pinned, "FILTER");
    assert_eq!(placeholders.len(), 1, "the row is painted once, at the top");
    assert!(
        placeholders[0].top() >= bar.top() - 0.01,
        "no `FILTER` paints above the pinned row: {:?} against {bar:?}",
        placeholders[0]
    );

    // Still there while the list keeps scrolling under it: a dozen more rows
    // must not drift the bar or paint a second copy of the field.
    let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
    let mut settled = pinned;
    for _ in 0..12 {
        settled = wheel(&ctx, size, at, -extent::ROW_H, &mut library);
    }
    let bar = pinned_bar(&settled).expect("the row stays pinned");
    assert!(
        (bar.top() - VIEWPORT_TOP).abs() < 0.01,
        "the pinned row stays at the viewport top: {bar:?}"
    );
    assert!(
        (bar.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01,
        "the pinned row keeps its height: {bar:?}"
    );
    let placeholders: Vec<Rect> = text_rects(&settled, "FILTER");
    assert_eq!(
        placeholders.len(),
        1,
        "one field is painted, not a second copy left in the flow"
    );
    assert!(
        placeholders[0].top() >= bar.top() - 0.01,
        "no `FILTER` paints above the pinned row: {:?} against {bar:?}",
        placeholders[0]
    );

    // The wheel back hands the row its place in the flow again.
    let mut restored = None;
    for _ in 0..240 {
        let output = wheel(&ctx, size, at, extent::ROW_H, &mut library);
        if pinned_bar(&output).is_none() && (field_at(&output).top() - flow.top()).abs() < 0.01 {
            restored = Some(output);
            break;
        }
    }
    let restored = restored.expect("the row returns to its place in the flow");
    assert!(
        field_at(&restored).top() > VIEWPORT_TOP + 0.01,
        "back at the top of the scroll, the row is in its flow again"
    );
    assert_eq!(
        text_rects(&restored, "FILTER").len(),
        1,
        "the row is still painted once"
    );
}

#[test]
fn a_pinned_field_still_edits_in_the_same_frame() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let size = Vec2::new(1000.0, 400.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let pinned = pinned_frame(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    let field = stuck_field(&pinned, size);
    assert!(
        (field.center().y - bar.center().y).abs() < 0.01,
        "the field is vertically centred on the pinned row: {field:?} against {bar:?}"
    );
    let before = painted_rows(&pinned);
    assert!(!before.is_empty(), "the list is drawn under the pin");

    let _ = clicks(
        size,
        &ctx,
        field.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
    );
    let typed = frame_in(
        &ctx,
        size,
        &mut library,
        vec![Event::Text("midnight".to_owned())],
    );

    let rows = painted_rows(&typed);
    assert!(!rows.is_empty(), "the narrowed list is still drawn");
    // The scroll offset is untouched, so the run of kept rows starts below
    // the top; what matters is that every painted row is a kept row, in the
    // list's order, where the whole list was painted before.
    let start = MIDNIGHT
        .iter()
        .position(|row| *row == rows[0])
        .unwrap_or_else(|| panic!("a kept row is painted first: {rows:?}"));
    assert_eq!(
        rows,
        MIDNIGHT[start..start + rows.len()].to_vec(),
        "the pinned field narrows the list on the keystroke's own frame: {before:?} was the whole list"
    );
    let typed_field = stuck_field(&typed, size);
    let count = text_rect(&typed, "12 OF 62 TRACKS").expect("the count is painted");
    assert!(
        count.right() <= typed_field.left() - crate::theme::actionbar::TOOLS_GAP + 0.01,
        "the count sits one tools gap left of the pinned field: {count:?} against {typed_field:?}"
    );
    assert!(
        drawn_texts(&typed).contains(&"midnight"),
        "the pinned field paints the query: {:?}",
        drawn_texts(&typed)
    );
}

#[test]
fn no_row_paints_over_the_pinned_bar() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let size = Vec2::new(900.0, 400.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let pinned = pinned_frame(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");

    let mut painted = 0;
    for clipped in &pinned.shapes {
        let egui::epaint::Shape::Text(text) = &clipped.shape else {
            continue;
        };
        let Some(index) = text.galley.text().strip_prefix('t') else {
            continue;
        };
        if index.parse::<usize>().is_err() {
            continue;
        }
        painted += 1;
        assert!(
            clipped.clip_rect.top() >= bar.bottom() - 0.01,
            "row `{}` may paint from {:?}, above the pinned row's bottom {:?}",
            text.galley.text(),
            clipped.clip_rect,
            bar.bottom()
        );
    }
    assert!(painted > 0, "the list is still drawn under the pinned row");
}

#[test]
fn the_pinned_field_keeps_its_keyboard() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let size = Vec2::new(900.0, 400.0);

    let layout = tracklist_frame_in(&ctx, size, &mut library);
    let field = field_at(&layout);
    let _ = clicks(
        size,
        &ctx,
        field.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
    );
    assert!(
        ctx.text_edit_focused(),
        "the click gave the field the keyboard"
    );

    let pinned = pinned_frame(&ctx, size, &mut library);
    assert!(
        pinned_bar(&pinned).is_some(),
        "the field is pinned with the row: {:?}",
        pinned_bar(&pinned)
    );
    assert!(
        ctx.text_edit_focused(),
        "a scroll frame leaves the pinned field its keyboard"
    );

    let cleared = frame_in(&ctx, size, &mut library, vec![key(egui::Key::Escape)]);

    let rows = painted_rows(&cleared);
    assert!(!rows.is_empty(), "the list is drawn after the Escape");
    assert_eq!(
        rows,
        (0..rows.len()).collect::<Vec<_>>(),
        "Escape clears the query from the pinned position, restoring the whole list in order"
    );
    assert!(library.track_filter.edit(PLAYLIST).is_empty());
    assert!(
        drawn_texts(&cleared).contains(&"FILTER"),
        "the pinned field shows its placeholder again: {:?}",
        drawn_texts(&cleared)
    );
    assert!(
        !ctx.text_edit_focused(),
        "Escape gives the keyboard back from the pinned position"
    );
}

/// The row index, box and clip of the first title the frame painted.
fn first_drawn_row(output: &egui::FullOutput) -> Option<(usize, Rect, Rect)> {
    let first = *painted_rows(output).first()?;
    let (title, clip) = clipped_title(output, &format!("t{first}"))?;
    Some((first, title, clip))
}

/// The title the frame painted inside `rect`, with the row it names.
fn title_in(output: &egui::FullOutput, rect: Rect) -> Option<(usize, Rect, Rect)> {
    output.shapes.iter().find_map(|clipped| {
        let egui::epaint::Shape::Text(text) = &clipped.shape else {
            return None;
        };
        let index = text
            .galley
            .text()
            .strip_prefix('t')?
            .parse::<usize>()
            .ok()?;
        let title = Rect::from_min_size(text.pos, text.galley.size());
        rect.contains(title.center())
            .then_some((index, title, clipped.clip_rect))
    })
}

/// Idle frames until the rows stop moving, answering the frame they rest on.
fn settled_frame(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
    let mut output = frame_in(ctx, size, library, Vec::new());
    let mut last = f32::NAN;
    for _ in 0..120 {
        let top = first_drawn_row(&output).map_or(f32::NAN, |(_, title, _)| title.top());
        if (top - last).abs() < 0.01 {
            break;
        }
        last = top;
        output = frame_in(ctx, size, library, Vec::new());
    }
    output
}

/// The resting pinned frame whose first drawn row has its title cut off by
/// the pin, and that row's true index.
///
/// The finding needs a row the pin partly covers: an offset that stops a row
/// exactly at the pin's bottom would test nothing, so the wheel is nudged in
/// six-point steps and the list let rest until one does.
fn pinned_over_a_row(
    ctx: &Context,
    size: Vec2,
    library: &mut Library,
) -> (egui::FullOutput, usize) {
    let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
    let mut output = settled_frame(ctx, size, library);
    for _ in 0..80 {
        if let Some(&hidden) = painted_rows(&output).first()
            && let Some((title, clip)) = clipped_title(&output, &format!("t{hidden}"))
            && title.top() < clip.top() - 0.5
            && title.bottom() < clip.bottom()
        {
            return (output, hidden);
        }
        let _ = wheel(ctx, size, at, -6.0, library);
        output = settled_frame(ctx, size, library);
    }
    panic!("the pin never covered a row's title");
}

/// Tabs until the keyboard lands on a row, answering the row's id.
///
/// The bar's field, then its tools, come before the rows, so the first
/// `ROW_H`-tall widget after the field is a row of the list.
fn tab_to_a_row(ctx: &Context, size: Vec2, library: &mut Library) -> egui::Id {
    let [press, release] = crate::view::test_support::tab();
    for _ in 0..8 {
        let _ = frame_in(ctx, size, library, vec![press.clone()]);
        let focused = ctx.memory(|memory| memory.focused());
        let _ = frame_in(ctx, size, library, vec![release.clone()]);
        if let Some(id) = focused
            && ctx
                .read_response(id)
                .is_some_and(|row| (row.rect.height() - extent::ROW_H).abs() < 0.01)
        {
            return id;
        }
    }
    panic!("the keyboard never reached a row");
}

#[test]
fn a_focused_row_is_not_left_under_the_pinned_bar() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let size = Vec2::new(900.0, 400.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let (pinned, hidden) = pinned_over_a_row(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");

    // The field takes the keyboard, and Tab walks past the row's tools and
    // into the list: the first row is the one the pin covers.
    let field = stuck_field(&pinned, size);
    let _ = clicks(
        size,
        &ctx,
        field.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
    );
    assert!(ctx.text_edit_focused(), "the field has no keyboard");
    let focused = tab_to_a_row(&ctx, size, &mut library);
    let row = ctx
        .read_response(focused)
        .expect("the focused row is drawn");
    assert!(
        row.rect.top() < bar.bottom(),
        "the pin covers the row the keyboard reached: {:?} against {bar:?}",
        row.rect
    );

    // The row asks to be brought into view; the pin stays at the top. The
    // scroll is animated, so the row is read until it stops moving, and
    // what it settles on is what the pin allows.
    let mut output = frame_in(&ctx, size, &mut library, Vec::new());
    let mut row = ctx
        .read_response(focused)
        .expect("the focused row is drawn");
    let mut last = f32::NAN;
    for _ in 0..120 {
        let bar = pinned_bar(&output).expect("the row stays pinned");
        if (row.rect.top() - last).abs() < 0.01 && row.rect.top() >= bar.bottom() - 0.01 {
            break;
        }
        last = row.rect.top();
        output = frame_in(&ctx, size, &mut library, Vec::new());
        row = ctx
            .read_response(focused)
            .expect("the focused row keeps being drawn");
    }
    let bar = pinned_bar(&output).expect("the row stays pinned");
    assert!(
        (bar.top() - VIEWPORT_TOP).abs() < 0.01,
        "the row stays pinned: {bar:?}"
    );
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(focused),
        "the row keeps the keyboard"
    );
    assert!(
        row.rect.top() >= bar.bottom() - 0.01,
        "the focused row rests below the pin: {:?} against {bar:?}",
        row.rect
    );
    let (index, title, clip) =
        title_in(&output, row.rect).expect("the focused row paints its title");
    assert_eq!(index, hidden, "the focused row is the one the pin covered");
    assert!(
        title.top() >= bar.bottom() - 0.01,
        "the focused row's title is below the pin: {title:?} against {bar:?}"
    );
    assert!(
        clip.top() <= title.top() + 0.01 && clip.bottom() >= title.bottom() - 0.01,
        "the pin cuts none of the title off: {clip:?} against {title:?}"
    );
    assert!(
        row.interact_rect.height() > 0.0
            && row.interact_rect.top() <= title.top() + 0.01
            && row.interact_rect.bottom() >= title.bottom() - 0.01,
        "the focused row's body is still reachable: {:?}",
        row.interact_rect
    );
}

#[test]
fn the_pinned_bar_owns_the_space_it_covers() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let size = Vec2::new(1000.0, 400.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let (pinned, _hidden) = pinned_over_a_row(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    let drawn = painted_rows(&pinned);
    assert!(!drawn.is_empty(), "rows are drawn under the pin");

    // A pick on a kept row that is off screen, so a press on any row under
    // the pin would replace it rather than look like it.
    let list = egui::Id::unique(("playlist-tracks", PLAYLIST));
    let kept = *MIDNIGHT
        .iter()
        .find(|row| !drawn.contains(row))
        .expect("a kept row is off screen");
    crate::view::carry::select_rows(&ctx, list, kept..kept + 1);

    // The count sits left of the field and on the pinned row's own line,
    // over the first row drawn under it: a press there is the bar's.
    let field = stuck_field(&pinned, size);
    let count = text_rect(&pinned, "12 OF 62 TRACKS").expect("the count is painted");
    assert!(
        count.top() >= bar.top() - 0.01 && count.bottom() <= bar.bottom() + 0.01,
        "the count is on the pinned row's line: {count:?} against {bar:?}"
    );
    assert!(
        count.right() <= field.left(),
        "the count sits left of the field: {count:?} against {field:?}"
    );

    let mut pressed = Vec::new();
    for at in [count.center(), bar.center()] {
        pressed.push(clicks(
            size,
            &ctx,
            at,
            1,
            &mut library,
            &mut transport,
            &mut queue,
        ));
    }

    assert!(
        pressed.iter().all(Option::is_none),
        "a press in the pinned row played a track: {pressed:?}"
    );
    assert!(
        queue.current().is_none(),
        "a press in the pinned row queued a track"
    );
    assert!(
        picked(&ctx, PLAYLIST, 62, kept),
        "a press in the pinned row changed the selection"
    );
    for row in 0..62 {
        if row != kept {
            assert!(
                !picked(&ctx, PLAYLIST, 62, row),
                "row {row} under the pinned row was pressed"
            );
        }
    }
}

#[test]
fn the_playlist_bar_pins_and_rows_clear_it() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let size = Vec2::new(900.0, 400.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let pinned = pinned_frame(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    assert!(
        (bar.top() - VIEWPORT_TOP).abs() < 0.01,
        "the row sits at the viewport's top edge: {bar:?}"
    );
    assert!(
        (bar.bottom() - PIN_TOP).abs() < 0.01,
        "the row condenses to the height the rows clear: {bar:?}"
    );

    let mut painted = 0;
    for index in painted_rows(&pinned) {
        let (_, clip) = clipped_title(&pinned, &format!("t{index}"))
            .unwrap_or_else(|| panic!("row {index} paints a title"));
        painted += 1;
        assert!(
            clip.top() >= bar.bottom() - 0.01,
            "row {index} paints from {clip:?}, inside the pinned row {bar:?}"
        );
    }
    assert!(painted > 0, "the list is drawn under the pinned row");
}

#[test]
fn the_playlist_bar_title_is_the_pages_name() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let size = Vec2::new(900.0, 400.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let pinned = pinned_frame(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    let labels: Vec<Rect> = text_rects(&pinned, "Late Night Drive");
    let label = labels
        .iter()
        .find(|rect| rect.top() >= bar.top() - 0.01 && rect.bottom() <= bar.bottom() + 0.01)
        .unwrap_or_else(|| {
            panic!("the pinned row paints the playlist's name: {labels:?} in {bar:?}")
        });
    assert!(
        label.left() > bar.left() + crate::theme::space::GUTTER,
        "the name stands beside the play tile, not in the gutter: {label:?}"
    );
    assert!(
        labels.len() > 1,
        "the band keeps its own copy of the name above the row: {labels:?}"
    );
}

#[test]
fn a_focused_row_partly_under_the_bar_is_scrolled_clear_of_it() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let size = Vec2::new(900.0, 500.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let pinned = pinned_frame(&ctx, size, &mut library);
    assert!(
        pinned_bar(&pinned).is_some(),
        "the action row is pinned before the press"
    );

    // The first row the list draws is wholly below the pinned row, so a
    // press on it lands on the row and not in the strip the row owns.
    let first = *painted_rows(&pinned)
        .first()
        .expect("the list is drawn under the pinned row");
    let title = clipped_title(&pinned, &format!("t{first}"))
        .expect("the row paints its title")
        .0;
    let _ = clicks(
        size,
        &ctx,
        Pos2::new(title.left() + 2.0, title.center().y),
        1,
        &mut library,
        &mut transport,
        &mut queue,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("the row took the keyboard");
    // Walk the list up until the row's top is inside the strip everything
    // pinned above the list covers, and let what the reveal asks for settle.
    let pinned_under = pinned_bar(&pinned).expect("the row is pinned").bottom();
    let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
    let mut covered = false;
    for _ in 0..80 {
        let row = ctx.read_response(focused).expect("the row is drawn").rect;
        if row.top() < pinned_under - 0.5 {
            covered = true;
            break;
        }
        let _ = wheel(&ctx, size, at, -6.0, &mut library);
    }
    assert!(covered, "the row never slid under the pinned row");

    let mut output = frame_in(&ctx, size, &mut library, Vec::new());
    let mut settled = false;
    for _ in 0..240 {
        if let Some(row) = ctx.read_response(focused)
            && row.rect.top() >= pinned_under - 0.01
        {
            settled = true;
            break;
        }
        output = frame_in(&ctx, size, &mut library, Vec::new());
    }
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(focused),
        "the row keeps the keyboard"
    );
    assert!(
        settled,
        "the focused row never came out from under the pinned row"
    );

    let bar = pinned_bar(&output).expect("the row is still pinned");
    let row = ctx
        .read_response(focused)
        .expect("the row is drawn again")
        .rect;
    assert!(
        row.top() >= bar.bottom() - 0.01,
        "the focused row rests below the pinned row: {row:?} against {bar:?}"
    );
}

#[test]
fn a_focused_row_wholly_under_the_bar_is_still_brought_out() {
    let ctx = context();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let size = Vec2::new(900.0, 500.0);

    let _ = tracklist_frame_in(&ctx, size, &mut library);
    let pinned = pinned_frame(&ctx, size, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    assert!(
        (bar.top() - VIEWPORT_TOP).abs() < 0.01,
        "the action row is pinned before the press"
    );

    // The first row the list draws is wholly below the pinned row, so a
    // press on its title lands on the row and not in the strip.
    let first = *painted_rows(&pinned)
        .first()
        .expect("the list is drawn under the pinned row");
    let title = clipped_title(&pinned, &format!("t{first}"))
        .expect("the row paints its title")
        .0;
    let _ = clicks(
        size,
        &ctx,
        Pos2::new(title.left() + 2.0, title.center().y),
        1,
        &mut library,
        &mut transport,
        &mut queue,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("the row took the keyboard");
    // The press lands on whichever row was under the pointer when the click
    // resolved, so the focused row is read from the frame it was drawn in
    // rather than assumed to be the one the pinned frame drew first.
    let after_click = frame_in(&ctx, size, &mut library, Vec::new());
    let focused_rect = ctx.read_response(focused).expect("the row is drawn").rect;
    let focused_row = title_in(&after_click, focused_rect)
        .expect("the focused row paints its title")
        .0;

    // One wheel jump of at least the pinned row and a whole row, and enough
    // to leave this row wholly above the strip, so the frame after it draws
    // nothing at the focused row: the cull skips it and `rows::body`, which
    // is a drawn row's own reveal, never runs.
    let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
    let strip = pinned_bar(&after_click)
        .expect("the row is pinned")
        .bottom();
    let jump =
        (focused_rect.bottom() - strip).max(crate::theme::actionbar::STUCK_H) + extent::ROW_H;
    let mut output = instant_frame_in(
        &ctx,
        size,
        &mut library,
        vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -jump),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            },
        ],
    );

    let mut culled = false;
    let mut settled = false;
    for _ in 0..240 {
        let strip = pinned_bar(&output).expect("the row stays pinned").bottom();
        if !painted_rows(&output).contains(&focused_row) {
            culled = true;
        } else if culled
            && ctx
                .read_response(focused)
                .is_some_and(|row| row.rect.top() >= strip - 0.01)
        {
            settled = true;
            break;
        }
        // Every frame runs the wheel the way the window's own pass does, so
        // the notch's tail cannot keep moving the list under the reveal.
        output = instant_frame_in(&ctx, size, &mut library, Vec::new());
    }
    assert!(
        culled,
        "the jump never put the focused row wholly under the pinned row"
    );
    assert!(
        settled,
        "the focused row never came out from under the pinned row"
    );
    let strip = pinned_bar(&output).expect("the row stays pinned").bottom();
    let row = ctx
        .read_response(focused)
        .expect("the row is drawn again")
        .rect;
    assert!(
        row.top() >= strip - 0.01,
        "the focused row rests below the pinned row: {row:?} against {strip}"
    );
}

#[test]
fn a_playlist_the_library_cannot_name_still_filters() {
    let ctx = context();
    // No entry: the library has no name for this playlist, so there is no
    // band and no title, but the tracks are there to filter and to play.
    let mut library = library_with(midnight_tracks());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let layout = tracklist_frame_in(&ctx, ALL, &mut library);
    let field = field_at(&layout);
    assert!(
        field.width() > 0.0,
        "the row is drawn for a playlist the library cannot name"
    );
    assert!(
        drawn_texts(&layout).contains(&"FILTER"),
        "the field shows its placeholder: {:?}",
        drawn_texts(&layout)
    );
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
    assert_eq!(
        painted_rows(&typed),
        MIDNIGHT.to_vec(),
        "typing narrows the list on a page with no name"
    );
    assert!(
        drawn_texts(&typed).contains(&"12 OF 62 TRACKS"),
        "and the count is painted: {:?}",
        drawn_texts(&typed)
    );

    // The play tile works on the tracks the page has, even with no name.
    let row = crate::layout::actionbar::solve(
        Rect::from_min_max(
            Pos2::new(0.0, field.top() - crate::theme::actionbar::PAD_REST),
            Pos2::new(ALL.x, field.bottom() + crate::theme::actionbar::PAD_REST),
        ),
        crate::layout::actionbar::Kind::Playlist(crate::layout::actionbar::Form::at(
            ALL.x,
            Table::Full,
        )),
        crate::layout::actionbar::Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    );
    let command = click(
        &ctx,
        row.play.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t0".to_owned()
        }),
        "a page with no name still plays its first track"
    );
}

#[test]
fn at_760_the_stuck_title_and_heart_are_gone_and_the_field_fills() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut library = library_named(midnight_tracks(), "Late Night Drive");
    library.track_filter.edit(PLAYLIST).push_str("midnight");
    let size = Vec2::new(760.0, 400.0);

    // The field the page actually draws is wider than the wide form's fixed
    // width: it has taken the room the count and the heart left.
    let layout = table_frame_in(&ctx, size, Table::Narrow, &mut library, Vec::new());
    let field = field_box_under(&layout, "midnight");
    assert!(
        field.width() > crate::theme::actionbar::FIELD_W,
        "the drawn field fills to the icons: {field:?}"
    );

    let pinned = pinned_frame_in(&ctx, size, Table::Narrow, &mut library);
    let bar = pinned_bar(&pinned).expect("the action row is pinned");
    assert!(
        !text_rects(&pinned, "Late Night Drive")
            .iter()
            .any(|label| label.top() >= bar.top() - 0.01 && label.bottom() <= bar.bottom() + 0.01),
        "a stuck narrow row paints no name: {:?}",
        text_rects(&pinned, "Late Night Drive")
    );
    for label in ["Save to your library", "Remove from your library"] {
        assert!(
            announced_in(&pinned, label, bar).is_none(),
            "a narrow row creates no heart control: {label} is announced in {bar:?}"
        );
    }

    // The page's own table is what condenses it: handed the full table, the
    // same width keeps the heart and the name.
    let mut wide = library_named(midnight_tracks(), "Late Night Drive");
    wide.track_filter.edit(PLAYLIST).push_str("midnight");
    let _ = table_frame_in(&ctx, size, Table::Full, &mut wide, Vec::new());
    let pinned_wide = pinned_frame_in(&ctx, size, Table::Full, &mut wide);
    let wide_bar = pinned_bar(&pinned_wide).expect("the action row is pinned");
    let heart = ["Save to your library", "Remove from your library"]
        .iter()
        .find_map(|label| announced_in(&pinned_wide, label, wide_bar))
        .expect("a wide row creates its heart control");
    assert!(
        !announced(&pinned, heart),
        "the narrow row never creates that control: {heart:?}"
    );
    assert!(
        text_rects(&pinned_wide, "Late Night Drive")
            .iter()
            .any(|label| label.top() >= wide_bar.top() - 0.01
                && label.bottom() <= wide_bar.bottom() + 0.01),
        "a wide row keeps the name on the pin"
    );
}
