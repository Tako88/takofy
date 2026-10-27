use egui::{Rect, pos2, vec2};

use super::*;

/// A row that is `H` tall at `left`, `top` wide enough to be a band.
const H: f32 = 40.0;

fn flow(top: f32) -> Rect {
    Rect::from_min_size(pos2(24.0, top), vec2(600.0, H))
}

fn clip() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(900.0, 300.0))
}

fn band(top: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, top), vec2(900.0, H))
}

#[test]
fn a_band_below_the_floor_rests() {
    let stuck = stick(flow(400.0), 60.0, 900.0);
    assert_eq!(stuck.hold, Hold::Resting);
    assert_eq!(stuck.rect, flow(400.0), "the band keeps its slot");
}

#[test]
fn a_band_exactly_at_the_floor_rests() {
    let stuck = stick(flow(60.0), 60.0, 900.0);
    assert_eq!(stuck.hold, Hold::Resting);
    assert_eq!(stuck.rect, flow(60.0));
}

#[test]
fn a_band_above_the_floor_holds_at_it() {
    let stuck = stick(flow(-20.0), 60.0, 900.0);
    assert_eq!(stuck.hold, Hold::Holding);
    assert_eq!(stuck.rect.top(), 60.0, "the band stops at the floor");
    assert_eq!(stuck.rect.left(), 24.0, "and keeps its column");
    assert_eq!(stuck.rect.size(), vec2(600.0, H));
}

#[test]
fn it_holds_up_to_the_exact_end() {
    let stuck = stick(flow(-20.0), 60.0, 60.0 + H);
    assert_eq!(stuck.hold, Hold::Holding);
    assert_eq!(stuck.rect.bottom(), 60.0 + H, "the end is the band's floor");
}

#[test]
fn one_point_past_it_releases() {
    let end = 60.0 + H - 1.0;
    let stuck = stick(flow(-20.0), 60.0, end);
    assert_eq!(stuck.hold, Hold::Released);
    assert_eq!(stuck.rect.bottom(), end, "the band rides the end");
    assert!(stuck.rect.top() < 60.0, "and is above the floor");
}

#[test]
fn a_band_never_passes_its_end_or_leaves_its_slot_upward() {
    let floor = 60.0;
    let mut previous = f32::INFINITY;
    // From above the floor to well past the end of the block, so the clamp on
    // the end is exercised and not only the one on the floor.
    for step in 0..400 {
        let top = 400.0 - step as f32 * 2.0;
        let flow = flow(top);
        // A block that ends a little past the row it holds.
        let end = top + H + 120.0;
        let stuck = stick(flow, floor, end);
        assert!(
            stuck.rect.top() >= flow.top() - 0.01,
            "the band left its slot upward at {top}: {:?}",
            stuck.rect
        );
        assert!(
            stuck.rect.bottom() <= end + 0.01,
            "the band passed its end at {top}: {:?} against {end}",
            stuck.rect
        );
        assert!(
            stuck.rect.top() <= previous + 0.01,
            "the band moved down while the page scrolled up: {:?}",
            stuck.rect
        );
        previous = stuck.rect.top();
    }
}

#[test]
fn a_resting_band_leaves_the_clip_alone() {
    let body = body_clip(clip(), band(400.0), Hold::Resting);
    assert_eq!(body, clip(), "a band in its slot takes no strip");
}

#[test]
fn a_held_band_clips_the_body_from_its_bottom() {
    let body = body_clip(clip(), band(60.0), Hold::Holding);
    assert_eq!(body.min.y, 60.0 + H, "the rows stop at the band's bottom");
    assert_eq!(body.max, clip().max, "and keep the viewport's floor");
    assert_eq!(body.min.x, clip().min.x);
}

#[test]
fn a_released_band_clips_the_body_above_it() {
    let body = body_clip(clip(), band(20.0), Hold::Released);
    assert_eq!(body.max.y, 20.0, "the rows stop above the riding band");
    assert_eq!(body.min, clip().min);
}

#[test]
fn a_band_above_the_clip_leaves_no_body() {
    let body = body_clip(clip(), band(-80.0), Hold::Released);
    assert_eq!(body.max.y, clip().min.y, "the body is empty, not inverted");
    assert_eq!(body.height(), 0.0);
}

#[test]
fn a_body_clip_never_grows_the_clip() {
    for top in [-120.0, -40.0, 0.0, 20.0, 60.0, 120.0, 260.0, 400.0] {
        for hold in [Hold::Resting, Hold::Holding, Hold::Released] {
            let body = body_clip(clip(), band(top), hold);
            assert!(
                body.min.x >= clip().min.x
                    && body.min.y >= clip().min.y
                    && body.max.x <= clip().max.x
                    && body.max.y <= clip().max.y,
                "{hold:?} at {top} grew the clip: {body:?}"
            );
            assert!(body.min.x == clip().min.x, "the columns are the caller's");
            assert!(body.max.x == clip().max.x, "the columns are the caller's");
        }
    }
}

#[test]
fn stack_height_counts_only_the_gaps_between() {
    assert_eq!(stack_height(&[100.0, 50.0], 8.0), 158.0);
    assert_eq!(stack_height(&[], 8.0), 0.0, "nothing is zero high");
    assert_eq!(stack_height(&[42.0], 8.0), 42.0, "one block has no gap");
}
