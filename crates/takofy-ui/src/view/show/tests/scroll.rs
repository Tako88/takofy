use egui::Context;

use super::super::*;
use super::test_support::*;
use crate::theme::{extent, stroke};

/// The bottom edge of the row whose title is `label`, read from the hairline
/// every row closes itself with.
fn row_bottom(output: &egui::FullOutput, label: &str) -> Option<f32> {
    let tokens = Tokens::DARK;
    let title = text_rect(output, label)?.top();
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == tokens.palette.border
                    && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.top() >= title =>
            {
                Some(shape.rect.bottom())
            }
            _ => None,
        })
        .reduce(f32::min)
}

/// The page's scroll offset, read through the id the page salts it with.
fn show_offset(ctx: &Context) -> Option<f32> {
    let mut id = None;
    run(ctx, input(Vec::new()), |ui| {
        // The area wraps its salt once before salting the frame's own ui with
        // it, so the probe hands over the same wrapped salt.
        id = Some(ui.make_persistent_id(egui::IdSalt::new(("show", SHOW))));
    });
    egui::scroll_area::State::load(ctx, id?).map(|state| state.clamped_offset().y)
}

#[test]
fn a_sort_keeps_the_scroll_area_inside_its_bounds() {
    let ctx = context();
    const EPISODES: usize = 12;
    // Episode 1 is the newest, so oldest-first reverses the wire's order.
    let episodes = (1..=EPISODES).map(|n| dated(n, n as u64 * 10)).collect();
    let mut library = library_with(detail(episodes));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, first) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let list_bottom = row_bottom(&first, "Episode 1").expect("the first row's edge")
        + (EPISODES - 1) as f32 * extent::EPISODE_ROW_H;
    let max_scroll = list_bottom - PROBE.y;

    let mut offset = show_offset(&ctx).expect("the page keeps its scroll offset");
    let mut settled = false;
    for _ in 0..400 {
        let _ = wheel(
            &ctx,
            -extent::EPISODE_ROW_H,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let next = show_offset(&ctx).expect("the page keeps its scroll offset");
        if (next - offset).abs() < 0.01 {
            offset = next;
            settled = true;
            break;
        }
        offset = next;
    }
    assert!(settled, "the scroll must come to rest");
    assert!(
        (offset - max_scroll).abs() < 0.5,
        "the scroll rests at {offset}, not the list's end {max_scroll}"
    );

    library.set_show_order(ShowOrder::Oldest);
    let mut output = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    for _ in 0..4 {
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

    let after = show_offset(&ctx).expect("the page keeps its scroll offset");
    assert!(
        (0.0..=max_scroll + 0.5).contains(&after),
        "the sorted page keeps {after} inside the list, whose end is {max_scroll}"
    );
    assert!(
        text_rect(&output, "Episode 1").is_some(),
        "the newest episode ends an oldest-first list, and the end is what the bottom shows"
    );
}

#[test]
fn a_culled_row_draws_nothing() {
    let ctx = context();
    let episodes = (1..=20).map(episode).collect();
    let mut library = library_with(detail(episodes));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);
    assert!(texts.contains(&"Episode 1"), "{texts:?}");
    assert!(
        !texts.contains(&"Episode 20"),
        "a row below the fold must not be laid out: {texts:?}"
    );
}

#[test]
fn the_last_episode_of_a_long_show_can_be_scrolled_to() {
    // A show is routinely hundreds of rows tall. The scroll area learns its
    // height from the list's rect, so a list reserved at the wrong row pitch
    // stops short of its own end.
    let ctx = context();
    const EPISODES: usize = 252;
    let episodes = (1..=EPISODES).map(episode).collect();
    let mut library = library_with(detail(episodes));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, first) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&first);
    assert!(texts.contains(&"Episode 1"), "{texts:?}");
    assert!(
        !texts.contains(&"Episode 252"),
        "a row below the fold must not be drawn before scrolling: {texts:?}"
    );

    // The list's height, read off the first row's own edge and the pitch the
    // page reserves rows at, so the bound does not come from the scroll state.
    let list_bottom = row_bottom(&first, "Episode 1").expect("the first row's edge")
        + (EPISODES - 1) as f32 * extent::EPISODE_ROW_H;
    let max_scroll = list_bottom - PROBE.y;

    let mut saw_last = false;
    for _ in 0..400 {
        let output = wheel(
            &ctx,
            -extent::EPISODE_ROW_H,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        if drawn_texts(&output).contains(&"Episode 252") {
            saw_last = true;
            break;
        }
    }
    assert!(saw_last, "the last episode must be reachable by scrolling");

    // The wheel keeps moving until the area stops: the end of the list must
    // be the end of the area, not where the viewport stopped drawing.
    let mut offset = show_offset(&ctx).expect("the page keeps its scroll offset");
    let mut settled = false;
    for _ in 0..50 {
        let _ = wheel(
            &ctx,
            -extent::EPISODE_ROW_H,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let next = show_offset(&ctx).expect("the page keeps its scroll offset");
        if (next - offset).abs() < 0.01 {
            offset = next;
            settled = true;
            break;
        }
        offset = next;
    }
    assert!(settled, "the scroll must come to rest");
    assert!(offset > 0.0, "the page must have scrolled: {offset}");
    assert!(
        (offset - max_scroll).abs() < 0.5,
        "the scroll rests at {offset}, not the list's end {max_scroll}"
    );
}
