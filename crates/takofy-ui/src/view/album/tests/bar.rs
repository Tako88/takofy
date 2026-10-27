use egui::{
    Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, Rect, TouchPhase, Vec2,
};

use takofy_model::AlbumDetail;

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::theme::hero::PLAY;
use crate::theme::space;

/// The action row's play tile, solved the way the page solves it.
fn play_tile_rect_for(ctx: &Context) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        ui.add_space(space::S3);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let band = hero::Band {
                eyebrow: "Album",
                title: "Signal Decay",
                meta: Vec::new(),
                credit: None,
                description: "",
                cover: Cover::Missing,
                tint: None,
            };
            let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
            let row = actionbar::row_rect(ui);
            found = Some(
                crate::layout::actionbar::solve(
                    row,
                    Kind::Other,
                    crate::layout::actionbar::Parts {
                        shuffle: false,
                        field: false,
                        follow: None,
                    },
                )
                .play,
            );
        });
    });
    found.expect("the band and its action row lay out")
}

/// The action row's box, read from the surface it fills once it pins.
fn bar_rect(output: &egui::FullOutput) -> Option<Rect> {
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

/// One frame with the wheel moved by `points` at the foot of the list, which
/// is how the page scrolls.
fn wheel(
    ctx: &Context,
    points: f32,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let at = Pos2::new(PROBE.x / 2.0, PROBE.y - extent::ROW_H);
    frame(
        ctx,
        vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: Vec2::new(0.0, points),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            },
        ],
        library,
        transport,
        queue,
        art,
    )
    .1
}

/// Wheels until the action row pins, and answers that frame.
fn pinned_frame(
    ctx: &Context,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let mut output = frame(ctx, Vec::new(), library, transport, queue, art).1;
    for _ in 0..240 {
        if bar_rect(&output).is_some() {
            return output;
        }
        output = wheel(ctx, -extent::ROW_H, library, transport, queue, art);
    }
    panic!("the action row never pinned");
}

/// A record long enough to scroll the band away.
fn long_detail() -> AlbumDetail {
    detail(
        (0..40)
            .map(|n| {
                track(
                    &format!("spotify:track:{n}"),
                    &format!("T{n}"),
                    "Nils Aker",
                    222_000,
                )
            })
            .collect(),
    )
}

#[test]
fn the_album_bar_pins_and_rows_clear_it() {
    let ctx = context();
    let mut library = library_with(long_detail());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
    let bar = bar_rect(&pinned).expect("the action row is pinned");
    assert!(
        (bar.top() - 0.0).abs() < 0.01,
        "the row sits at the viewport's top edge: {bar:?}"
    );

    let mut painted = 0;
    for clipped in &pinned.shapes {
        let egui::epaint::Shape::Text(text) = &clipped.shape else {
            continue;
        };
        if !text.galley.text().starts_with('T') || text.galley.text() == "Tom Scott Media" {
            continue;
        }
        painted += 1;
        assert!(
            clipped.clip_rect.top() >= bar.bottom() - 0.01,
            "row `{}` may paint from {:?}, inside the pinned row {bar:?}",
            text.galley.text(),
            clipped.clip_rect
        );
    }
    assert!(painted > 0, "the list is drawn under the pinned row");
}

#[test]
fn the_album_bar_title_is_the_pages_name() {
    let ctx = context();
    let mut library = library_with(long_detail());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
    let bar = bar_rect(&pinned).expect("the action row is pinned");
    let in_bar: Vec<Rect> = pinned
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Signal Decay" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .filter(|rect| rect.top() >= bar.top() - 0.01 && rect.bottom() <= bar.bottom() + 0.01)
        .collect();
    assert_eq!(
        in_bar.len(),
        1,
        "the pinned row paints the album's name once: {in_bar:?}"
    );
    assert!(
        in_bar[0].left() > bar.left() + PLAY,
        "the name stands beside the play tile: {:?}",
        in_bar[0]
    );
}

#[test]
fn a_focused_row_partly_under_the_bar_is_scrolled_clear_of_it() {
    let ctx = context();
    let mut library = library_with(long_detail());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
    let bar = bar_rect(&pinned).expect("the action row is pinned");

    // The first row the list draws is wholly below the pinned row, so a
    // press on it lands on the row and not in the strip the row owns.
    let title = text_rect(&pinned, "T0").expect("the row paints its title");
    let _ = click(
        &ctx,
        Pos2::new(title.left() + 2.0, title.center().y),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("the row took the keyboard");

    // Walk the list up until the row's top is inside the strip the pinned
    // row covers, then let what the reveal asked for settle.
    let mut covered = false;
    for _ in 0..80 {
        let row = ctx.read_response(focused).expect("the row is drawn").rect;
        if row.top() < bar.bottom() - 0.5 {
            covered = true;
            break;
        }
        let _ = wheel(
            &ctx,
            -6.0,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    assert!(covered, "the row never slid under the pinned row");

    let mut output = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    let mut settled = false;
    for _ in 0..240 {
        if let Some(row) = ctx.read_response(focused)
            && row.rect.top() >= bar.bottom() - 0.01
        {
            settled = true;
            break;
        }
        output = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
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
    let bar = bar_rect(&output).expect("the row is still pinned");
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
    let mut library = library_with(long_detail());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
    assert!(
        bar_rect(&pinned).is_some(),
        "the action row is pinned before the press"
    );

    // The first row the list draws, so a press on its title lands on the row
    // and not in the strip the pinned row owns.
    let title = text_rect(&pinned, "T0").expect("the row paints its title");
    let _ = click(
        &ctx,
        Pos2::new(title.left() + 2.0, title.center().y),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("the row took the keyboard");
    // The press lands on whichever row was under the pointer when the click
    // resolved, so the focused row is read from the frame it was drawn in.
    let after = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    let rect = ctx
        .read_response(focused)
        .expect("the focused row is drawn")
        .rect;
    let label = drawn_texts(&after)
        .into_iter()
        .find(|text| {
            text.strip_prefix('T')
                .and_then(|tail| tail.parse::<usize>().ok())
                .is_some()
                && text_rect(&after, text).is_some_and(|box_| rect.contains_rect(box_))
        })
        .map(str::to_owned)
        .expect("the focused row paints its title");

    // One wheel jump of at least the pinned row and a whole row, and enough
    // to leave this row wholly above the strip, so the frame after it draws
    // nothing at the focused row: the cull skips it and `rows::body`, which
    // is a drawn row's own reveal, never runs.
    let strip = bar_rect(&after).expect("the row is pinned").bottom();
    let jump = (rect.bottom() - strip).max(crate::theme::actionbar::STUCK_H) + extent::ROW_H;
    let mut output = wheel(
        &ctx,
        -jump,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let mut culled = false;
    let mut settled = false;
    for _ in 0..240 {
        let strip = bar_rect(&output).expect("the row is still pinned").bottom();
        if text_rect(&output, &label).is_none() {
            culled = true;
        } else if culled
            && ctx
                .read_response(focused)
                .is_some_and(|row| row.rect.top() >= strip - 0.01)
        {
            settled = true;
            break;
        }
        output = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
    }
    assert!(
        culled,
        "the jump never put the focused row wholly under the pinned row"
    );
    assert!(
        settled,
        "the focused row never came out from under the pinned row"
    );
    let bar = bar_rect(&output).expect("the row is still pinned");
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
fn the_bands_play_starts_the_first_track() {
    let ctx = context();
    let tracks = vec![
        track("spotify:track:a", "One", "Band", 222_000),
        track("spotify:track:b", "Two", "Band", 198_000),
    ];
    let mut library = library_with(detail(tracks));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let tile = play_tile_rect_for(&ctx);
    let command = click(
        &ctx,
        tile.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:track:a".to_owned()
        }),
        "the band plays the album from its first track"
    );
    assert!(
        queue.can_next(),
        "the whole album is queued, not just its first track"
    );
}
