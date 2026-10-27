use super::super::*;
use super::test_support::*;
use crate::view::rows::Look;
use crate::view::track_row::State;
use egui::{Context, Event, Pos2, Rect, Vec2};
use takofy_model::{AddedAt, TrackEntry};

/// How far a row's words sit below the row's own box, read from the first
/// row the frame painted: the fold clips at the words, not at the box.
fn title_inset(ctx: &Context, count: usize, output: &egui::FullOutput) -> f32 {
    let first = painted_rows(output)[0];
    let (title, _) = clipped_title(output, &format!("t{first}")).expect("the first row's title");
    title.top() - row_rect_for(ctx, count, first).top()
}

#[test]
fn a_double_click_on_a_row_starts_that_track_through_the_queue() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    // The click is where the view draws row 0, not a probe's geometry.
    let centre = row_rect_for(&ctx, 2, 0).center();
    let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    assert!(
        queue.can_next(),
        "the double-clicked row must start the queue, not only the transport"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("a")
    );
}

#[test]
fn a_single_track_list_starts_its_only_track() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let centre = row_rect_for(&ctx, 1, 0).center();
    let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    assert!(
        !queue.can_next(),
        "a one-track queue has nothing after its only entry"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("a")
    );
}

#[test]
fn a_row_with_no_album_added_or_duration_still_lays_out() {
    let tokens = Tokens::DARK;
    let ctx = context();
    // An album or an artist context carries neither an album name nor a
    // playlist's added date, and a track may have no length. Solving its
    // columns and double-clicking must still lay the row out and activate it.
    let columns = table_layout::columns(900.0, Table::Full.columns());
    let rect = table_layout::row_rect(
        Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
        0,
        extent::ROW_H,
    );
    let row = Row {
        uri: "a",
        number: 1,
        title: "t0",
        artist: "",
        artist_uri: None,
        album: None,
        album_uri: None,
        added: None,
        duration: None,
        state: State::Resting,
        cover: Some(Cover::Missing),
        look: Look::default(),
        menu: false,
        liked: None,
    };

    run(&ctx, input(Vec::new()), |ui| {
        let _ = track_row::show(ui, rect, columns, &row, &tokens);
    });
    let mut clicked = false;
    for _ in 0..2 {
        for event in [
            Event::PointerMoved(rect.center()),
            crate::view::test_support::button(rect.center(), true),
            crate::view::test_support::button(rect.center(), false),
        ] {
            run(&ctx, input(vec![event]), |ui| {
                clicked |= track_row::show(ui, rect, columns, &row, &tokens) == Some(Click::Play);
            });
        }
    }
    assert!(
        clicked,
        "a row with no album, added or duration must still lay out and activate"
    );
}

#[test]
fn a_row_given_an_album_and_a_duration_draws_them() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let columns = table_layout::columns(900.0, Table::Full.columns());
    let rect = table_layout::row_rect(
        Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
        0,
        extent::ROW_H,
    );
    let row = Row {
        uri: "a",
        number: 1,
        title: "t0",
        artist: "Band",
        artist_uri: None,
        album: Some("Record"),
        album_uri: None,
        added: None,
        duration: Some("3:42"),
        state: State::Resting,
        cover: Some(Cover::Missing),
        look: Look::default(),
        menu: false,
        liked: None,
    };

    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = track_row::show(ui, rect, columns, &row, &tokens);
    });
    output.textures_delta.clear();
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"Record"),
        "the album column must draw its label: {texts:?}"
    );
    assert!(
        texts.contains(&"3:42"),
        "the duration column must draw its label: {texts:?}"
    );
}

#[test]
fn the_view_hands_a_tracks_album_and_length_to_its_row() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let mut library = library_with(vec![TrackEntry {
        uri: "a".to_owned(),
        title: "t0".to_owned(),
        artists: "Band".to_owned(),
        album: Some("Record".to_owned()),
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(222_000),
        named: true,
        cover: None,
    }]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = tracks_view(
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
    output.textures_delta.clear();
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"Record"),
        "the row must be handed the track's album: {texts:?}"
    );
    assert!(
        texts.contains(&"3:42"),
        "the row must be handed the track's length as m:ss: {texts:?}"
    );
}

#[test]
fn a_playlist_row_draws_its_added_label() {
    let ctx = context();
    let mut library = library_with(vec![dated("a", "t0", Some(date(NOW_MS - 4 * DAY)))]);

    let texts = painted_lines(&ctx, Table::Full, &mut library);
    assert!(
        texts.contains(&"ADDED".to_owned()),
        "the header must keep the added column: {texts:?}"
    );
    assert!(
        texts.contains(&"4 days ago".to_owned()),
        "the row must show when the track was added: {texts:?}"
    );
}

#[test]
fn a_playlist_without_dates_keeps_its_rows_and_header() {
    // The column is a playlist's, not a date's: a playlist whose dates are
    // all absent still heads it and still draws every row.
    let ctx = context();
    let mut library = library_with(vec![dated("a", "t0", None), dated("b", "t1", None)]);

    let texts = painted_lines(&ctx, Table::Full, &mut library);
    assert!(
        texts.contains(&"ADDED".to_owned()),
        "the header must stay with no dates loaded: {texts:?}"
    );
    assert!(texts.contains(&"t0".to_owned()), "row 0 must draw");
    assert!(texts.contains(&"t1".to_owned()), "row 1 must draw");
    assert!(
        !texts
            .iter()
            .any(|text| text == "Today" || text.ends_with(" ago")),
        "an absent date must paint no cell: {texts:?}"
    );
}

#[test]
fn a_future_date_draws_no_cell() {
    let ctx = context();
    let mut library = library_with(vec![dated("a", "t0", Some(date(NOW_MS + DAY)))]);

    let texts = painted_lines(&ctx, Table::Full, &mut library);
    assert!(texts.contains(&"t0".to_owned()), "the row must still draw");
    assert!(
        !texts
            .iter()
            .any(|text| text == "Today" || text.ends_with(" ago")),
        "a date after now is not an age: {texts:?}"
    );
}

#[test]
fn a_context_that_is_not_a_playlist_draws_no_added_column() {
    let ctx = context();
    // Long enough to elide at the probe width in both contexts, so the
    // visible run is a measure of the column the title was given.
    let title = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends \
Plays the Music of the Twentieth Century for an Audience of Patient Listeners";
    let rows = || vec![dated("a", title, Some(date(NOW_MS - 2 * DAY)))];

    let mut playlist = library_selected("spotify:playlist:a", rows());
    let playlist_texts = painted_runs(&ctx, Table::Full, &mut playlist);

    let mut album = library_selected("spotify:album:a", rows());
    let album_texts = painted_runs(&ctx, Table::Full, &mut album);

    assert!(
        playlist_texts.contains(&"ADDED".to_owned()),
        "a playlist heads the date column: {playlist_texts:?}"
    );
    assert!(
        !album_texts.contains(&"ADDED".to_owned()),
        "an album must not head a date column: {album_texts:?}"
    );
    assert!(
        !album_texts
            .iter()
            .any(|text| text == "Today" || text.ends_with(" ago")),
        "an album's rows must show no date even when they carry one: {album_texts:?}"
    );

    // The dropped column's width goes to the title: the same title shows
    // more of itself in the album than in the playlist.
    let painted_title = |texts: &[String]| {
        texts
            .iter()
            .find(|text| text.starts_with("The Extremely"))
            .cloned()
            .expect("the row paints its title")
    };
    let playlist_title = painted_title(&playlist_texts);
    let album_title = painted_title(&album_texts);
    assert!(
        playlist_title.ends_with('…'),
        "the playlist's title must be squeezed: {playlist_title:?}"
    );
    assert!(
        album_title.chars().count() > playlist_title.chars().count(),
        "the album's title must take the freed width: {album_title:?} vs {playlist_title:?}"
    );
}

#[test]
fn the_same_track_twice_shows_each_rows_own_date() {
    let ctx = context();
    let mut library = library_with(vec![
        dated("a", "t0", Some(date(NOW_MS - 4 * DAY))),
        dated("a", "t0", Some(date(NOW_MS - 14 * DAY))),
    ]);

    let texts = painted_lines(&ctx, Table::Full, &mut library);
    assert!(
        texts.contains(&"4 days ago".to_owned()),
        "the first row must show its own date: {texts:?}"
    );
    assert!(
        texts.contains(&"2 weeks ago".to_owned()),
        "the second row must show its own date: {texts:?}"
    );
}

#[test]
fn the_second_of_two_identical_entries_starts_the_second() {
    // Two entries holding one uri: keyed by the uri alone, both rows would
    // share one egui id, so one click would drive both and the higher index
    // would win. The command's uri is `a` whichever row is taken, so the
    // queue's position is the only thing that tells the two apart: starting
    // the second leaves nothing after it, while the first would leave entry
    // two, so `can_next` is the sharp assertion.
    //
    // Index 1 also exercises `row_rect`'s pitch: the other tests only ever
    // click index 0.
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let first = row_rect_for(&ctx, 2, 0);
    let second = row_rect_for(&ctx, 2, 1);
    assert!(
        (second.top() - first.bottom()).abs() < 0.01,
        "the second row must start where the first ends: {first:?} then {second:?}"
    );
    let command = double_click(
        &ctx,
        second.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    assert!(
        !queue.can_next(),
        "the second of two identical entries was started, not the first"
    );
}

#[test]
fn a_double_click_on_the_first_of_two_identical_entries_starts_the_first() {
    // The mirror of the test above. When the two rows shared one id, the
    // higher index overwrote the lower, so clicking the first row started
    // the second; this pins that the first is the one that answers, and it
    // fails against the uri-only key where the other test alone would not.
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let first = row_rect_for(&ctx, 2, 0).center();
    let command = double_click(&ctx, first, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    assert!(
        queue.can_next(),
        "the first of two identical entries was started, not the second"
    );
}

#[test]
fn a_row_outside_the_viewport_takes_no_click() {
    let ctx = context();
    // Sixty rows at `extent::ROW_H` each put row forty far below the 400-point
    // probe window, so `ui.clip_rect()` cannot contain it and the view skips
    // laying it out. A shorter list would leave the clicked row on screen and
    // the test would prove nothing.
    let tracks: Vec<TrackEntry> = (0..60)
        .map(|i| track(&format!("t{i}"), &format!("t{i}")))
        .collect();
    let mut library = library_with(tracks);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let offscreen = row_rect_for(&ctx, 60, 40);
    assert!(
        offscreen.top() > 400.0,
        "the culled row must sit below the probe's viewport: {offscreen:?}"
    );
    let command = double_click(
        &ctx,
        offscreen.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(command, None, "a culled row must not activate");
}

#[test]
fn an_unnamed_row_draws_the_count_inside_the_viewport() {
    let ctx = context();
    let count = 60;
    let mut tracks: Vec<TrackEntry> = (0..count)
        .map(|index| track(&format!("t{index}"), &format!("t{index}")))
        .collect();
    tracks[0].named = false;
    let mut library = library_with(tracks);
    let viewport = Rect::from_min_size(Pos2::ZERO, PROBE);
    let last = row_rect_for(&ctx, count, count - 1);
    assert!(
        !viewport.intersects(last),
        "the last row must sit below the probe's viewport: {last:?}"
    );

    let output = tracklist_frame_in(&ctx, PROBE, &mut library);
    let note = "1 of 60 tracks could not be named";
    let painted = galley_of(&output, note);
    let rect = Rect::from_min_size(galley_at(&output, note), painted.size());
    assert!(
        viewport.contains_rect(rect),
        "the count is drawn inside the viewport, not after the rows: {rect:?}"
    );

    // The strip takes its room from the list: no row draws over the count,
    // and the rows stop at its top edge instead of running under it.
    let rows = painted_rows(&output);
    for index in &rows {
        let (title, _) = clipped_title(&output, &format!("t{index}"))
            .unwrap_or_else(|| panic!("row {index} was painted without its title"));
        assert!(
            !title.intersects(rect),
            "row {index} draws over the count: {title:?} against {rect:?}"
        );
    }
    let highest = *rows.last().expect("the list is drawn");
    let inset = title_inset(&ctx, count, &output);
    assert_eq!(
        rows,
        (0..=highest).collect::<Vec<_>>(),
        "the rows stop where the count starts"
    );
    assert!(
        row_rect_for(&ctx, count, highest).top() + inset < rect.top(),
        "the last row's words start above the count"
    );
    assert!(
        row_rect_for(&ctx, count, highest + 1).top() + inset >= rect.top(),
        "the row under the count would draw its words past the strip"
    );
}

#[test]
fn a_note_costs_the_list_one_row_that_a_named_list_keeps() {
    let ctx = context();
    let count = 60;
    let tracks: Vec<TrackEntry> = (0..count)
        .map(|index| track(&format!("t{index}"), &format!("t{index}")))
        .collect();
    let mut numbered_tracks = tracks.clone();
    numbered_tracks[0].named = false;
    let mut named = library_with(tracks);
    let mut numbered = library_with(numbered_tracks);

    let full = tracklist_frame_in(&ctx, PROBE, &mut named);
    let counted = tracklist_frame_in(&ctx, PROBE, &mut numbered);

    assert!(
        !drawn_texts(&full)
            .iter()
            .any(|text| text.contains("could not be named")),
        "a fully named list draws no count"
    );

    // Without a strip the list uses the whole window: it paints every row
    // that starts above the fold, and nothing below it.
    let all_rows = painted_rows(&full);
    let last_fitting = (0..count)
        .take_while(|index| row_rect_for(&ctx, count, *index).top() < PROBE.y)
        .last()
        .expect("the window holds rows");
    assert_eq!(
        all_rows,
        (0..=last_fitting).collect::<Vec<_>>(),
        "the named list paints every row the window holds"
    );

    let note = "1 of 60 tracks could not be named";
    let strip = Rect::from_min_size(galley_at(&counted, note), galley_of(&counted, note).size());
    let fewer = painted_rows(&counted);
    assert!(
        fewer.len() < all_rows.len(),
        "the count costs the list a row: {} rows with it, {} without",
        fewer.len(),
        all_rows.len()
    );
    let highest = *fewer.last().expect("the list is drawn");
    assert_eq!(
        highest + 1,
        *all_rows.last().expect("the list is drawn"),
        "the count costs exactly the one row it covers"
    );
    let inset = title_inset(&ctx, count, &full);
    assert!(
        row_rect_for(&ctx, count, highest).top() + inset < strip.top(),
        "the last row's words start above the count"
    );
    assert!(
        row_rect_for(&ctx, count, highest + 1).top() + inset >= strip.top(),
        "the row the count covers would draw its words past the strip"
    );
}

#[test]
fn the_count_survives_a_filter_that_hides_the_unnamed_row() {
    let ctx = context();
    let mut tracks = vec![track("hidden", "hidden")];
    tracks[0].named = false;
    tracks.extend((1..10).map(|index| track(&format!("t{index}"), "keep")));
    let mut library = library_with(tracks);
    library
        .track_filter
        .edit("spotify:playlist:a")
        .push_str("keep");

    let output = tracklist_frame_in(&ctx, PROBE, &mut library);

    assert!(
        drawn_texts(&output).contains(&"1 of 10 tracks could not be named"),
        "the count covers the whole loaded list, not the visible hits: {:?}",
        drawn_texts(&output)
    );
    assert!(
        !painted_rows(&output).contains(&0),
        "the unnamed row is hidden by the query"
    );
}

#[test]
fn a_row_that_only_touches_the_fold_asks_for_no_cover() {
    // The fold is the viewport's bottom edge, and a row whose top sits on it
    // shares only an edge with the viewport. It is not on screen, so it must
    // not be laid out and must not fetch a cover nothing draws.
    let count = 20;
    let row = 3;
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let tracks: Vec<TrackEntry> = (0..count).map(track_with_cover).collect();
    let mut library = library_with(tracks);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    // Solved the way the view solves it, then read back from the row's own
    // hover wash: the wash is painted at the row's rect.
    let rect = row_rect_for(&ctx, count, row);
    let mut washed = None;
    for _ in 0..12 {
        art.begin_frame();
        let mut output = ctx.run_ui(input(vec![Event::PointerMoved(rect.center())]), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        washed = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == tokens.palette.row_hover => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .or(washed);
    }
    assert_eq!(
        washed,
        Some(rect),
        "a row washes at the rect it is laid out in"
    );

    // The window's height is that row's top, so the fold is its top edge.
    art.begin_frame();
    let mut output = ctx.run_ui(
        sized_input(Vec2::new(PROBE.x, rect.top()), Vec::new()),
        |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        },
    );
    output.textures_delta.clear();

    let painted: std::collections::HashSet<usize> = painted_rows(&output).into_iter().collect();
    assert_eq!(
        painted,
        (0..row).collect(),
        "only the rows above the fold are laid out"
    );
    let requested: std::collections::HashSet<usize> = art
        .requests(now)
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(cover_row(&url)),
            _ => None,
        })
        .collect();
    assert!(
        !requested.contains(&row),
        "the row that only touches the fold asked for a cover: {requested:?}"
    );
    assert_eq!(
        requested, painted,
        "the covers asked for are the rows the window shows"
    );
}

/// One day in milliseconds.
const DAY: u64 = 86_400_000;

fn date(millis: u64) -> AddedAt {
    AddedAt::from_wire(&millis.to_string()).expect("a date above the floor")
}

fn dated(uri: &str, title: &str, added_at: Option<AddedAt>) -> TrackEntry {
    TrackEntry {
        added_at,
        ..track(uri, title)
    }
}

/// The visible run of every painted line: an elided line is read as what it
/// shows, not as the string it was given.
fn painted_runs(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
    tracklist_frame(ctx, table, library)
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => text.galley.rows.first(),
            _ => None,
        })
        .map(|row| row.row.text().to_owned())
        .collect()
}
