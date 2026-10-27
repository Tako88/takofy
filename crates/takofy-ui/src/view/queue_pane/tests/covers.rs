use std::collections::HashSet;

use egui::Pos2;
use takofy_model::{CoverSet, CoverUrl};

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::message::Command;
use crate::player::Transport;

/// A cover offering 64, 300 and 640 squares, with their URLs.
fn sized(tail: &str) -> (CoverSet, CoverUrl, CoverUrl, CoverUrl) {
    let small =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-64")).expect("a cover url");
    let middle =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-300")).expect("a cover url");
    let large =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-640")).expect("a cover url");
    let set = CoverSet::from_sizes([
        (Some((64, 64)), small.clone()),
        (Some((300, 300)), middle.clone()),
        (Some((640, 640)), large.clone()),
    ])
    .expect("a cover set");
    (set, small, middle, large)
}

#[test]
fn only_visible_rows_ask_for_a_cover() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    // Twenty entries with distinct covers in a pane showing two whole rows
    // and part of a third, fewer than the fetch cap: the requested set must
    // be exactly the visible entries' set, so a row laid out past the fold
    // shows up as an extra request instead of hiding under the cap.
    let tracks: Vec<TrackEntry> = (0..20).map(track_with_cover).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut Transport::default());

    let pane = Rect::from_min_size(
        Pos2::ZERO,
        Vec2::new(PANE.width(), CAPTION_H + 2.0 * ROW_H + ROW_H / 2.0),
    );
    art.begin_frame();
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = show(ui, pane, &queue, &mut art, now, &tokens);
    });
    output.textures_delta.clear();

    let visible: HashSet<usize> = painted_rows(&output).into_iter().collect();
    let requested: HashSet<usize> = art
        .requests(now)
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(cover_row(&url)),
            _ => None,
        })
        .collect();

    assert_eq!(
        requested,
        HashSet::from([1, 2, 3]),
        "a row past the fold must not ask for a cover"
    );
    assert_eq!(
        visible,
        HashSet::from([1, 2, 3]),
        "the pane lays out exactly the rows that meet its fold"
    );
}

#[test]
fn a_queue_row_at_one_x_asks_for_the_small_size() {
    let ctx = context();
    ctx.set_pixels_per_point(1.0);
    let mut art = Art::default();
    let now = Instant::now();
    let mut queue = Queue::default();
    let (set, small, _, large) = sized("q");
    let tracks = vec![
        track(0),
        TrackEntry {
            cover: Some(set),
            ..track(1)
        },
    ];
    queue.start(&tracks, 0, None, &mut Transport::default());

    let _ = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let requested: Vec<CoverUrl> = art
        .requests(now)
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(url),
            _ => None,
        })
        .collect();

    assert!(
        requested.contains(&small),
        "a 40pt row takes the 64: {requested:?}"
    );
    assert!(
        !requested.contains(&large),
        "a queue row must not fetch the 640: {requested:?}"
    );
}

#[test]
fn a_row_hands_the_art_store_its_own_cover() {
    // A row with a cover it does not hold yet is `Loading`, not `Missing`,
    // and the answer is what puts the picture in the box.
    let ctx = context();
    let mut art = Art::default();
    let now = Instant::now();
    let mut queue = Queue::default();
    let tracks = vec![track(0), track_with_cover(1)];
    queue.start(&tracks, 0, None, &mut Transport::default());

    let _ = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let requested: Vec<CoverUrl> = art
        .requests(now)
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(url),
            _ => None,
        })
        .collect();
    assert_eq!(requested.len(), 1, "only the row with a cover asks for one");
    assert_eq!(cover_row(&requested[0]), 1);
    assert_eq!(art.look(tracks[1].cover.as_ref(), 64, now), Cover::Loading);
}
