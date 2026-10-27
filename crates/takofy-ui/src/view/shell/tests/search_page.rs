use super::super::*; // the module under test — what `use super::*` gave the flat module
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn the_search_page_renders_its_heading_and_count() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    answered(&mut window, "never gonna", 20);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        painted_at(&frame, "Results for “never gonna”").is_some(),
        "the heading names the query"
    );
    assert!(
        painted_at(&frame, "TOP 20 TRACKS").is_some(),
        "the page's note states the cap once"
    );
    assert!(
        painted_at(&frame, "10 OF 20").is_some(),
        "the track block's head says how much of it is shown"
    );
    assert!(
        painted_at(&frame, "t0").is_some(),
        "the answer's tracks are the page's table"
    );
}

/// The page holds its shape while the read is out, says so when the answer
/// is empty, and offers a Retry when the read failed — the three states the
/// table has nothing to draw in.
#[test]
fn the_search_page_holds_skeleton_empty_and_failure_states() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = window.library.open(Page::Search("never gonna".to_owned()));

    // Idle: nothing true to draw yet, so the page holds rows open.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(painted_at(&frame, "Results for “never gonna”").is_some());
    assert!(
        skeleton_rects(&frame, view_rect(&window)) > 0,
        "a pending page holds its rows open"
    );
    assert!(
        painted_at(&frame, "t0").is_none(),
        "and draws no answer it does not have"
    );

    // Failed: the failure's own words and a Retry that asks once.
    window.library.apply(&Update::SearchFailed {
        query: "never gonna".to_owned(),
        failure: Failure::Search,
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, crate::signin::failure_message(Failure::Search)).is_some(),
        "the failure is one muted line, not an error box"
    );
    let retry = painted_at(&frame, "Retry").expect("the failure offers a Retry");
    assert_eq!(
        window.click(&mut transport, &mut queue, retry + Vec2::new(4.0, 4.0)),
        Some(Command::Search {
            query: "never gonna".to_owned()
        }),
        "Retry is the same read, asked again"
    );
    assert_eq!(
        window.library.search.page_answer("never gonna"),
        Some(&Load::Loading)
    );

    // Empty: the page head with nothing to count, then the two-line empty
    // state, nothing to press, and no rows held open any more.
    window.library.apply(&Update::Search {
        query: "never gonna".to_owned(),
        tracks: Vec::new(),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "0 TRACKS").is_some(),
        "an empty answer still says how many tracks it found"
    );
    assert!(
        painted_at(&frame, "Nothing in the catalogue matches that").is_some(),
        "an empty answer says so"
    );
    assert!(
        painted_at(&frame, "Retry").is_none(),
        "an empty answer offers nothing to retry"
    );
    assert_eq!(
        skeleton_rects(&frame, view_rect(&window)),
        0,
        "an answered page holds nothing open"
    );
}

#[test]
fn a_click_on_a_result_row_queues_the_whole_answer_at_that_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    answered(&mut window, "never gonna", 20);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "t3").expect("row t3 is painted");

    assert_eq!(
        window.double_click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
        Some(Command::Play {
            uri: "t3".to_owned()
        }),
        "a row plays itself"
    );
    assert_eq!(
        queue.upcoming().len(),
        16,
        "the queue is the whole answer, entered at the row"
    );
    assert_eq!(
        queue.next(&mut transport),
        Some(Command::Play {
            uri: "t4".to_owned()
        }),
        "next walks the answer, not just the row that was clicked"
    );
}

#[test]
fn back_and_forward_walk_the_search_pages() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    answered(&mut window, "a", 3);
    answered(&mut window, "b", 3);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(painted_at(&frame, "Results for “b”").is_some());
    let buttons = topbar_buttons(&frame);

    // Back onto a's page: its answer was dropped when b replaced it, so the
    // page asks again — and never paints b's rows under a's heading.
    assert_eq!(
        window.click(&mut transport, &mut queue, buttons[1].center()),
        None,
        "a search page never settles with a command"
    );
    assert_eq!(
        window.library.page_search_query(),
        Some("a"),
        "Back walks the history"
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(painted_at(&frame, "Results for “a”").is_some());
    assert!(
        painted_at(&frame, "t0").is_none(),
        "another query's answer must not be drawn"
    );
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "a".to_owned()
        })
    );
    window.library.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: vec![track(0, None), track(1, None), track(2, None)],
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "t0").is_some(),
        "the re-asked answer is the page's table"
    );

    // Forward onto b's page, which is asked for the same way.
    let buttons = topbar_buttons(&frame);
    window.click(&mut transport, &mut queue, buttons[2].center());
    assert_eq!(window.library.page_search_query(), Some("b"));
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "b".to_owned()
        })
    );
    window.library.apply(&Update::Search {
        query: "b".to_owned(),
        tracks: vec![track(0, None), track(1, None), track(2, None)],
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(painted_at(&frame, "Results for “b”").is_some());
}

/// The scroll area is salted with the query, so a new query starts at the
/// top rather than where the last one was left.
///
/// Ten rows fit the window, so a page only scrolls once its reveal is taken
/// — which is also the press that grows it.
#[test]
fn a_new_query_starts_at_the_top_of_the_results() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    answered(&mut window, "a", 40);
    reveal(
        &mut window,
        &mut transport,
        &mut queue,
        "Show 30 more tracks",
    );

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let first = painted_at(&frame, "t0").expect("row t0 is painted").y;
    let top = painted_at(&frame, "t5").expect("row t5 is painted").y;
    // The wheel only reaches the area the pointer is over.
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(view_rect(&window).center())],
    );

    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    // A wheel notch is smoothed over a few passes, and the page lays out
    // with the offset it was left on, so the movement shows a pass later.
    for _ in 0..4 {
        let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
    }
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let scrolled = painted_at(&frame, "t5").expect("row t5 is still painted").y;
    assert!(
        scrolled < top - 50.0,
        "the wheel moves this query's page: {scrolled} against {top}"
    );

    // The offset is stored under this query's own id: another query's page
    // is a different area, so it starts at the top.
    answered(&mut window, "b", 40);
    reveal(
        &mut window,
        &mut transport,
        &mut queue,
        "Show 30 more tracks",
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let fresh = painted_at(&frame, "t5").expect("row t5 is painted").y;
    assert!(
        (fresh - top).abs() < 0.01,
        "a new query starts at the top: {fresh} against {top}"
    );

    // Back onto a, whose answer b dropped: the page is answered again and
    // its blocks start at ten, because the read the reveal belonged to is
    // gone. It is a fresh page, so it starts at the top too — not where a
    // was left, and not at the shortened page's bottom.
    assert_eq!(window.library.back(), None, "a search page never settles");
    assert_eq!(window.library.page_search_query(), Some("a"));
    window.library.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: (0..40).map(|index| track(index, None)).collect(),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(painted_at(&frame, "Results for “a”").is_some());
    assert!(
        painted_at(&frame, "Results for “b”").is_none(),
        "b's page is gone"
    );
    assert!(
        painted_at(&frame, "Show 30 more tracks").is_some(),
        "a's blocks start at ten again"
    );
    assert!(
        painted_at(&frame, "t39").is_none(),
        "so only ten of a's rows are drawn"
    );
    let back = painted_at(&frame, "t0")
        .expect("a's first row is painted")
        .y;
    assert!(
        (back - first).abs() < 0.01,
        "a page that starts fresh starts at the top: {back} against {first}"
    );
}

// -----------------------------------------------------------------------
// The field, the menu and the frame order.
// -----------------------------------------------------------------------

/// The skeleton bars painted inside `area`.
fn skeleton_rects(output: &egui::FullOutput, area: Rect) -> usize {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) => {
                shape.fill == tokens.palette.skeleton_rule && area.contains_rect(shape.rect)
            }
            _ => false,
        })
        .count()
}
