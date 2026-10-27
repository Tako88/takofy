use super::super::*; // the module under test
use super::*; // the shared fixtures, from tests/mod.rs
use crate::releases::Labels;

/// A Popular row reveals through the page's seam: a row the pinned bar
/// partly covers is asked for clear of the bar, not just into the scroll
/// area's own clip.
#[test]
fn a_focused_popular_row_partly_under_the_bar_is_scrolled_clear_of_it() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    // Enough rows that the row can be walked up under the pinned bar and
    // still have somewhere to scroll back from.
    let top: Vec<TrackEntry> = (0..40)
        .map(|index| {
            track(
                &format!("spotify:track:p{index}"),
                &format!("P{index}"),
                "Vela Moreno",
                200_000,
            )
        })
        .collect();
    let mut library = library_with(detail(top, Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let mut output = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    for _ in 0..40 {
        if bar_rect(&output).is_some() {
            break;
        }
        output = wheel(
            &ctx,
            size,
            -extent::ROW_H,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    let bar = bar_rect(&output).expect("the artist's row never pinned");

    // The first row is wholly below the pinned row, so a press on it lands
    // on the row and not in the strip the row owns.
    let title = text_rect(&output, "P0").expect("the row paints its title");
    let _ = click(
        &ctx,
        size,
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

    // Walk the row up until its top is inside the strip the pinned row
    // covers, then let what the reveal asked for settle.
    let mut covered = false;
    for _ in 0..80 {
        let row = ctx.read_response(focused).expect("the row is drawn").rect;
        if row.top() < bar.bottom() - 0.5 {
            covered = true;
            break;
        }
        let _ = wheel(
            &ctx,
            size,
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
        size,
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
            size,
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

/// A Popular row the cull skipped is still asked for through the page's
/// seam: a row wholly above the strip the pinned bar owns draws nothing, so
/// it has no reveal of its own and the section has to ask for it.
#[test]
fn a_focused_popular_row_wholly_under_the_bar_is_still_brought_out() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    // Enough rows that the row can be jumped wholly above the pinned bar
    // and still have somewhere to scroll back from.
    let top: Vec<TrackEntry> = (0..40)
        .map(|index| {
            track(
                &format!("spotify:track:p{index}"),
                &format!("P{index}"),
                "Vela Moreno",
                200_000,
            )
        })
        .collect();
    let mut library = library_with(detail(top, Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let mut output = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    for _ in 0..40 {
        if bar_rect(&output).is_some() {
            break;
        }
        output = wheel(
            &ctx,
            size,
            -extent::ROW_H,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    assert!(
        bar_rect(&output).is_some(),
        "the artist's row is pinned before the press"
    );

    // The first row the list draws, so a press on its title lands on the row
    // and not in the strip the pinned row owns.
    let title = text_rect(&output, "P0").expect("the row paints its title");
    let _ = click(
        &ctx,
        size,
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
        size,
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
            text.strip_prefix('P')
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
        size,
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
            size,
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
fn an_off_screen_block_asks_for_no_labels() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(
        Vec::new(),
        vec![
            release("spotify:album:r1", "One", &["spotify:track:1"]),
            release("spotify:album:r2", "Two", &["spotify:track:2"]),
            release("spotify:album:r3", "Three", &["spotify:track:3"]),
        ],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    for absent in ["spotify:album:r2", "spotify:album:r3"] {
        assert_eq!(
            library.releases.look(absent),
            Labels::Idle,
            "{absent} is off screen and must not be wanted"
        );
    }
}

/// One frame asks for the on-screen block's labels and nothing else: the
/// want set is filled by the draw and drained once, afterwards.
#[test]
fn one_frame_asks_for_exactly_the_on_screen_labels() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(
        Vec::new(),
        vec![
            release("spotify:album:r1", "One", &["spotify:track:1"]),
            release("spotify:album:r2", "Two", &["spotify:track:2"]),
            release("spotify:album:r3", "Three", &["spotify:track:3"]),
        ],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let requests = library.releases.requests();

    assert_eq!(
        requests,
        vec![Command::LoadReleaseTracks {
            release: "spotify:album:r1".to_owned(),
            tracks: vec!["spotify:track:1".to_owned()],
        }],
        "exactly the block that is on screen is asked for: {requests:?}"
    );
    assert!(
        library.releases.requests().is_empty(),
        "the drain clears the want set, so one frame is one request"
    );
}

/// A realistic discography: forty-eight releases, far more than fits. The
/// frame must draw, and must ask only for the blocks it can show.
#[test]
fn a_long_discography_asks_only_for_what_is_on_screen() {
    let ctx = context();
    // 900 tall, not 800: the filter row takes its own height above the
    // blocks, and at 800 only one block fits, which would not exercise the
    // two-request cap.
    let size = Vec2::new(900.0, 900.0);
    let releases: Vec<Release> = (1..=48)
        .map(|index| {
            release(
                &format!("spotify:album:r{index}"),
                &format!("Release {index}"),
                &["spotify:track:a", "spotify:track:b"],
            )
        })
        .collect();
    let mut library = library_with(detail(Vec::new(), releases));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert!(
        drawn_texts(&output).contains(&"Release 1"),
        "the first block's head is drawn"
    );
    let requests = library.releases.requests();
    assert_eq!(
        requests.len(),
        2,
        "the top of the discography is what fits, and the cap holds: {requests:?}"
    );
    for command in &requests {
        let Command::LoadReleaseTracks { release, .. } = command else {
            panic!("a label drain emits only label requests: {command:?}");
        };
        assert!(
            release == "spotify:album:r1" || release == "spotify:album:r2",
            "a block far below the fold was asked for: {release}"
        );
    }
}
