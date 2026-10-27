use super::super::*;
use super::test_support::*;
use egui::{Context, Pos2, Rect};
use takofy_model::ShowDetail;

use crate::theme::extent;

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
        output = wheel(ctx, -extent::EPISODE_ROW_H, library, transport, queue, art);
    }
    panic!("the action row never pinned");
}

/// A show with more episodes than one screen holds.
fn long_detail() -> ShowDetail {
    detail((0..12).map(episode).collect())
}

#[test]
fn the_show_bar_pins_and_rows_clear_it() {
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
    for number in 1..=12 {
        let Some(title) = text_rect(&pinned, &format!("Episode {number}")) else {
            continue;
        };
        painted += 1;
        assert!(
            title.top() >= bar.bottom() - 0.01,
            "episode {number} paints at {title:?}, inside the pinned row {bar:?}"
        );
    }
    assert!(painted > 0, "the list is drawn under the pinned row");
}

#[test]
fn the_show_bar_title_is_the_pages_name() {
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
            egui::epaint::Shape::Text(text) if text.galley.text() == "Lateral" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .filter(|rect| rect.top() >= bar.top() - 0.01 && rect.bottom() <= bar.bottom() + 0.01)
        .collect();
    assert_eq!(
        in_bar.len(),
        1,
        "the pinned row paints the show's name once: {in_bar:?}"
    );
    assert!(
        in_bar[0].left() > bar.left() + crate::theme::hero::PLAY,
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

    // The topmost episode the list draws, so a press on it lands on the row
    // and not in the strip the pinned row owns.
    let title = text_rect(&pinned, "Episode 1").expect("the first episode is drawn");
    let _ = click(
        &ctx,
        Pos2::new(title.left() + 2.0, title.center().y),
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

    // The topmost episode the list draws, so a press on its title lands on
    // the row and not in the strip the pinned row owns.
    let title = text_rect(&pinned, "Episode 1").expect("the first episode is drawn");
    let _ = click(
        &ctx,
        Pos2::new(title.left() + 2.0, title.center().y),
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
            text.starts_with("Episode ")
                && text_rect(&after, text).is_some_and(|box_| rect.contains_rect(box_))
        })
        .map(str::to_owned)
        .expect("the focused row paints its title");

    // One wheel jump of at least the pinned row and a whole row, and enough
    // to leave this row wholly above the strip, so the frame after it draws
    // nothing at the focused row: the cull skips it and `rows::body`, which
    // is a drawn row's own reveal, never runs.
    let strip = bar_rect(&after).expect("the row is pinned").bottom();
    let jump =
        (rect.bottom() - strip).max(crate::theme::actionbar::STUCK_H) + extent::EPISODE_ROW_H;
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
