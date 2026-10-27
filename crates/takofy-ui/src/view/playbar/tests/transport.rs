use super::super::*;
use super::test_support::*;
use egui::{Event, RawInput};

/// The bar has nothing to play before the first track, so its tile is the
/// same tile the band draws with nothing to act on: a filled, inert square.
#[test]
fn an_idle_playbar_paints_the_disabled_tile() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let bar = heart_bar();
    let cols = layout::playbar_for(bar, Playbar::Full).columns(bar);
    let mut transport = Transport::default();
    assert_eq!(transport.state, TransportState::Idle);
    let mut queue = Queue::default();
    assert!(!queue.can_toggle(), "an idle bar has nothing to play");

    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            ..Default::default()
        },
        |ui| {
            let _ = show(
                ui,
                &mut transport,
                &mut queue,
                0,
                Playbar::Full,
                Bar::default(),
                None,
                bar,
                Cover::Missing,
                &tokens,
            );
        },
    );
    output.textures_delta.clear();

    let tiles: Vec<Rect> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.text_disabled
                    && (shape.rect.height() - extent::BUTTON_LG).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(tiles.len(), 1, "an idle bar draws exactly one grey tile");
    assert!(
        (tiles[0].center().x - cols[1].center().x).abs() < 0.01,
        "the tile at {:?} is not on its column",
        tiles[0].center()
    );

    let glyph: Vec<egui::Color32> = output
        .shapes
        .iter()
        .filter(|clipped| tiles[0].contains(clipped.shape.visual_bounding_rect().center()))
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Path(path) if path.fill != egui::Color32::TRANSPARENT => Some(path.fill),
            egui::Shape::Circle(circle) => Some(circle.fill),
            _ => None,
        })
        .collect();
    assert!(!glyph.is_empty(), "the tile paints its glyph");
    for colour in glyph {
        assert_eq!(colour, tokens.palette.text_inverse);
    }
}

/// The button a hover at `at` paints, which is the button's own rect. The
/// clock only moves forward, so the eased hover finishes.
fn hovered_button(
    ctx: &egui::Context,
    tokens: &Tokens,
    form: Playbar,
    queue: &mut Queue,
    at: Pos2,
    clock: &mut f64,
) -> Rect {
    *clock += 0.1;
    let moved = vec![Event::PointerMoved(at)];
    let _ = bar_pass(ctx, tokens, 1200.0, form, queue, moved, *clock);
    *clock += 1.0;
    let (_, output) = bar_pass(ctx, tokens, 1200.0, form, queue, Vec::new(), *clock);
    *transport_boxes(&output, heart_bar())
        .iter()
        .find(|rect| (rect.height() - extent::BUTTON).abs() < 0.01)
        .expect("the hovered button paints its fill")
}

/// The transport's painted boxes, left to right. Only its runs and its tile
/// sit above the bar's centre line, so the other columns' buttons are not
/// mistaken for them.
fn transport_boxes(output: &egui::FullOutput, bar: Rect) -> Vec<Rect> {
    let mut rects: Vec<Rect> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.rect.center().y < bar.center().y - 1.0
                    && ((shape.rect.height() - extent::BUTTON).abs() < 0.01
                        || (shape.rect.height() - extent::BUTTON_LG).abs() < 0.01) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
    rects
}

/// The tile among `boxes`, which is the one `BUTTON_LG` square.
fn painted_tile(boxes: &[Rect]) -> Rect {
    *boxes
        .iter()
        .find(|rect| (rect.height() - extent::BUTTON_LG).abs() < 0.01)
        .expect("the transport paints its tile")
}

#[test]
fn the_tile_sits_twelve_from_both_neighbours_in_both_forms() {
    let tokens = Tokens::DARK;
    for form in [Playbar::Full, Playbar::Reduced] {
        // One context per form, because the eased hover needs a clock that
        // only moves forward.
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let (_, output) = bar_at(&ctx, &tokens, 1200.0, form, &mut queue);
        let tile = painted_tile(&transport_boxes(&output, heart_bar()));
        let mut clock = 0.0;

        // The pointer lands in the middle of the run, which is where a
        // neighbour is whatever the clearance is.
        let previous = hovered_button(
            &ctx,
            &tokens,
            form,
            &mut queue,
            Pos2::new(tile.left() - 26.0, tile.center().y),
            &mut clock,
        );
        assert!(
            (tile.left() - previous.right() - 12.0).abs() < 0.01,
            "{form:?}: the tile is {} from the down run",
            tile.left() - previous.right()
        );

        let next = hovered_button(
            &ctx,
            &tokens,
            form,
            &mut queue,
            Pos2::new(tile.right() + 26.0, tile.center().y),
            &mut clock,
        );
        assert!(
            (next.left() - tile.right() - 12.0).abs() < 0.01,
            "{form:?}: the tile is {} from the up run",
            next.left() - tile.right()
        );
    }
}

#[test]
fn the_tile_is_centred_on_the_columns_centre_in_both_forms() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    for form in [Playbar::Full, Playbar::Reduced] {
        let (cols, output) = bar_at(&ctx, &tokens, 1200.0, form, &mut queue_of(4));
        let tile = painted_tile(&transport_boxes(&output, heart_bar()));
        assert!(
            (tile.center().x - cols[1].center().x).abs() < 0.01,
            "{form:?}: the tile at {:?} is not on the column's centre {}",
            tile.center(),
            cols[1].center().x
        );
    }
}

/// The hover test proves the tile is painted where it is; this proves the
/// press lands there too.
#[test]
fn a_click_on_the_painted_tile_toggles_the_queue_in_both_forms() {
    let tokens = Tokens::DARK;
    for form in [Playbar::Full, Playbar::Reduced] {
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let (_, output) = bar_at(&ctx, &tokens, 1200.0, form, &mut queue);
        let tile = painted_tile(&transport_boxes(&output, heart_bar()));

        let replies = click_at(
            &ctx,
            &tokens,
            &mut queue,
            form,
            Bar::default(),
            tile.center(),
        );
        assert!(
            replies
                .iter()
                .any(|reply| reply.command == Some(Command::Pause)),
            "{form:?}: a click at {:?} did not pause the playing queue",
            tile.center()
        );
    }
}

#[test]
fn the_transport_fits_its_column_at_every_supported_width() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    for width in 640..=2000 {
        let width = width as f32;
        let form = layout::regions_for(width).playbar;
        let (cols, output) = bar_at(&ctx, &tokens, width, form, &mut queue_of(4));
        let column = cols[1];
        let tile = painted_tile(&transport_boxes(&output, heart_bar()));

        // The buttons paint no frame of their own, so the run either side of
        // the painted tile is where its own widths put it.
        let left = tile.left() - (space::S1 + space::S2) - run_width(&DOWN);
        let right = tile.right() + space::S2 + space::S1 + run_width(up_controls(form));
        assert!(
            left >= column.left() - 1e-3 && right <= column.right() + 1e-3,
            "{width}: {form:?} runs from {left} to {right} outside {column:?}"
        );
    }
}

#[test]
fn the_play_glyph_follows_the_transport() {
    assert_eq!(play_glyph(TransportState::Playing), Icon::Pause);
    for state in [
        TransportState::Idle,
        TransportState::Loading,
        TransportState::Paused,
        TransportState::Stopped,
        TransportState::Unavailable,
        TransportState::Reconnecting,
    ] {
        assert_eq!(play_glyph(state), Icon::Play, "{state:?}");
    }
}
