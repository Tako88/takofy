use super::super::*;
use super::test_support::*;
use crate::message::{Failure, Update};
use egui::{Context, Event, Pos2, RawInput, Vec2};
use takofy_model::TrackEntry;

/// The grid sits one gutter inside the pane, like every other page's content.
#[test]
fn the_grid_sits_one_gutter_inside_the_pane() {
    let ctx = context();
    let card = card_rect_for(&ctx, 0);
    assert!(
        (card.left() - space::GUTTER).abs() < 0.01,
        "the first card starts at {}, the pane's gutter is {}",
        card.left(),
        space::GUTTER
    );
}

/// The centre of the like column of the `index`-th row, solved the way the
/// view solves its columns.
fn heart_point_for(ctx: &Context, count: usize, index: usize) -> Pos2 {
    let tokens = Tokens::DARK;
    let table = Table::Full;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                let width = ui.available_width();
                actionbar::row_rect(ui);
                let columns = table_layout::columns(width, table.columns());
                head(ui, columns, &tokens);
                let list = list_rect(ui, count);
                let row = table_layout::row_rect(list, index, extent::ROW_H);
                found = Some(columns.at(row.min).like.center());
            });
        });
    });
    found.expect("the heart's column lays out")
}

#[test]
fn a_single_click_selects_and_a_double_click_plays_it() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let centre = row_rect_for(&ctx, 2, 1).center();
    let command = click(&ctx, centre, &mut library, &mut transport, &mut queue);

    assert_eq!(command, None, "a single click plays nothing");
    assert!(transport.now_playing.is_none(), "and starts nothing");
    assert!(
        picked(&ctx, "spotify:playlist:a", 2, 1),
        "the row under the click is picked"
    );
    assert!(
        !picked(&ctx, "spotify:playlist:a", 2, 0),
        "and only that row"
    );

    // A fresh page for the double click: a third click inside the window is
    // a triple click, and that plays nothing.
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "b".to_owned()
        }),
        "a double click starts the track"
    );
}

#[test]
fn a_selection_stays_on_the_playlist_it_was_made_on() {
    let ctx = context();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut first = library_with(vec![track("a", "t0"), track("b", "t1")]);
    let centre = row_rect_for(&ctx, 2, 1).center();
    let _ = click(&ctx, centre, &mut first, &mut transport, &mut queue);

    let mut second = Library::default();
    let _ = second.select("spotify:playlist:b");
    second.apply(&Update::Tracks {
        playlist: "spotify:playlist:b".to_owned(),
        tracks: vec![track("a", "t0"), track("b", "t1")],
        cover: None,
        access: None,
    });
    let tokens = Tokens::DARK;
    run(&ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut second,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            9,
            &tokens,
        );
    });

    assert!(
        picked(&ctx, "spotify:playlist:a", 2, 1),
        "the first playlist keeps its pick"
    );
    assert!(
        !picked(&ctx, "spotify:playlist:b", 2, 1),
        "the second playlist does not show it against the same index"
    );
}

#[test]
fn lifting_a_row_puts_that_track_in_the_payload() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let centre = row_rect_for(&ctx, 2, 1).center();
    lift_row(&ctx, centre, &mut library, &mut transport, &mut queue);

    let carried = crate::view::carry::carried(&ctx).expect("a carried track");
    assert_eq!(
        carried.source,
        crate::view::carry::Source::List(egui::Id::unique((
            "playlist-tracks",
            "spotify:playlist:a"
        ))),
        "the playlist names itself as the source"
    );
    assert_eq!(carried.rows, vec![1], "one row travels");
    assert_eq!(carried.entries.len(), 1);
    assert_eq!(carried.entries[0].uri, "b", "the row under the pointer");
    assert!(
        matches!(carried.ghost, crate::view::carry::Ghost::Table(_)),
        "a track row's own snapshot is the ghost"
    );
}

#[test]
fn a_carried_list_scrolls_at_its_edge() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let tracks: Vec<TrackEntry> = (0..40)
        .map(|index| track(&format!("t{index}"), &format!("t{index}")))
        .collect();
    let mut library = library_with(tracks);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let mut drawn = |raw: RawInput, library: &mut Library| -> Vec<String> {
        let output = run(&ctx, raw, |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                9,
                &tokens,
            );
        });
        drawn_texts(&output)
            .into_iter()
            .map(str::to_owned)
            .collect()
    };

    let before = drawn(sized_input(PROBE, Vec::new()), &mut library);
    egui::DragAndDrop::set_payload(&ctx, ());

    // The pointer holds the list's bottom edge for a quarter of a second.
    let edge = Pos2::new(PROBE.x / 2.0, PROBE.y - 1.0);
    let mut time = 1.0;
    for _ in 0..24 {
        time += 0.016;
        let mut raw = sized_input(PROBE, vec![Event::PointerMoved(edge)]);
        raw.time = Some(time);
        let _ = drawn(raw, &mut library);
    }
    let after = drawn(sized_input(PROBE, Vec::new()), &mut library);

    assert!(
        after
            .iter()
            .any(|line| line.starts_with('t') && !before.contains(line)),
        "the carried list scrolls at its edge: {after:?}"
    );
}

#[test]
fn an_editable_page_offers_its_rows() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
    let output = tracklist_frame_in(&ctx, TALL, &mut library);

    let offer = crate::view::carry::take_offer(&ctx).expect("an editable page offers its rows");
    assert_eq!(
        offer.list,
        egui::Id::unique(("playlist-tracks", "spotify:playlist:a")),
        "the offer names the list its rows lift under"
    );
    assert_eq!(offer.playlist, "spotify:playlist:a");
    assert_eq!(offer.zone.first, 0, "the page numbers its rows from zero");
    assert_eq!(offer.zone.count, 2);
    assert_eq!(offer.zone.row_h, extent::ROW_H);
    for row in ["t0", "t1"] {
        assert!(
            offer.zone.hit.contains(galley_at(&output, row)),
            "{row} is inside the drop target"
        );
    }
}

#[test]
fn a_read_only_page_offers_nothing() {
    let ctx = context();
    let mut library = read_only_library_with(vec![track("a", "t0"), track("b", "t1")]);
    let _ = tracklist_frame(&ctx, Table::Full, &mut library);

    assert_eq!(
        crate::view::carry::take_offer(&ctx),
        None,
        "a page the user may not write takes no reorder"
    );
}

#[test]
fn a_read_only_page_still_lifts_and_carries() {
    let ctx = context();
    let mut library = read_only_library_with(vec![track("a", "t0"), track("b", "t1")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let centre = row_rect_for(&ctx, 2, 1).center();
    lift_row(&ctx, centre, &mut library, &mut transport, &mut queue);

    let carried = crate::view::carry::carried(&ctx).expect("a read-only page still lifts");
    assert_eq!(carried.rows, vec![1]);
    assert_eq!(
        carried.entries[0].uri, "b",
        "the lifted track travels to the queue"
    );
}

#[test]
fn a_heart_click_posts_one_ask_with_the_rows_uri() {
    let ctx = context();
    let tracks = vec![
        track("spotify:track:a", "t0"),
        track("spotify:track:b", "t1"),
    ];
    let mut library = library_with(tracks);
    liked(&mut library, &[]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let point = heart_point_for(&ctx, 2, 1);

    let command = click(&ctx, point, &mut library, &mut transport, &mut queue);

    assert_eq!(command, None, "a heart is not a command of its own");
    let ask = crate::view::heart::take(&ctx).expect("an ask was posted");
    assert_eq!(ask.tracks, vec!["spotify:track:b".to_owned()]);
    assert!(ask.liked, "an unsaved row asks to be saved");
}

#[test]
fn a_heart_click_inside_a_pick_acts_on_that_row_only_and_keeps_the_pick() {
    let ctx = context();
    let tracks = vec![
        track("spotify:track:a", "t0"),
        track("spotify:track:b", "t1"),
    ];
    let mut library = library_with(tracks);
    liked(&mut library, &[]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let list = egui::Id::unique(("playlist-tracks", "spotify:playlist:a"));
    crate::view::carry::select_rows(&ctx, list, 0..2);
    let point = heart_point_for(&ctx, 2, 1);

    let _ = click(&ctx, point, &mut library, &mut transport, &mut queue);

    let ask = crate::view::heart::take(&ctx).expect("an ask was posted");
    assert_eq!(
        ask.tracks,
        vec!["spotify:track:b".to_owned()],
        "the pick is the menu's, not the heart's"
    );
    let kept = crate::view::carry::List::open(&ctx, list, 2);
    assert!(
        kept.look(0).selected && kept.look(1).selected,
        "the heart changed the pick"
    );
}

#[test]
fn a_track_listed_twice_shows_one_state_on_both_rows() {
    let ctx = context();
    let tracks = vec![
        track("spotify:track:a", "t0"),
        track("spotify:track:a", "t0 again"),
    ];
    let mut library = library_with(tracks);
    liked(&mut library, &["spotify:track:a"]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    for index in 0..2 {
        let point = heart_point_for(&ctx, 2, index);
        let _ = click(&ctx, point, &mut library, &mut transport, &mut queue);
        let ask = crate::view::heart::take(&ctx).expect("an ask was posted");
        assert_eq!(
            ask.tracks,
            vec!["spotify:track:a".to_owned()],
            "row {index}"
        );
        assert!(!ask.liked, "row {index} did not know the track was liked");
    }
}

#[test]
fn a_failed_like_draws_its_sentence_in_the_pages_slot() {
    let ctx = context();
    let tracks = vec![track("spotify:track:a", "t0")];
    let mut library = library_with(tracks);
    liked(&mut library, &[]);
    let change = library
        .likes
        .set(&["spotify:track:a".to_owned()], true)
        .expect("a change");
    library.likes.apply(&crate::likes::Event::Failed {
        tracks: change.tracks,
        failure: Failure::Save,
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let tokens = Tokens::DARK;

    let output = run(&ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            9,
            &tokens,
        );
    });

    assert!(
        drawn_texts(&output).contains(&"Could not save that change. Try again."),
        "the failed like drew no sentence: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn a_right_click_posts_the_menu_its_row_asked_for() {
    let ctx = context();
    let tracks = vec![track("a", "t0"), linkable("b")];
    let mut library = library_with(tracks.clone());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let centre = row_rect_for(&ctx, 2, 1).center();

    for event in crate::view::test_support::secondary_click_events(centre) {
        let _ = pass(&ctx, event, &mut library, &mut transport, &mut queue);
    }
    let request = crate::view::context_menu::take_request(&ctx)
        .expect("a right-click on a row posted a request");
    assert_eq!(request.targets.len(), 1);
    assert_eq!(request.targets[0].uri, "b", "the row under the pointer");
    assert_eq!(
        request.page,
        Some(crate::route::Page::Playlist(
            "spotify:playlist:a".to_owned()
        )),
        "the page it was opened on, so a page change can close it"
    );
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::GoToAlbum),
        "the row's own album is one of its verbs"
    );
}

#[test]
fn a_page_with_a_write_in_flight_posts_no_menu() {
    let ctx = context();
    let tracks = vec![track("a", "t0"), track("b", "t1")];
    let mut library = library_with(tracks.clone());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(
        library
            .move_tracks("spotify:playlist:a", &[0], 2, &[tracks[0].clone()])
            .is_some(),
        "the page takes the move first"
    );
    let centre = row_rect_for(&ctx, 2, 1).center();

    for event in crate::view::test_support::secondary_click_events(centre) {
        let _ = pass(&ctx, event, &mut library, &mut transport, &mut queue);
    }
    assert_eq!(
        crate::view::context_menu::take_request(&ctx),
        None,
        "a write in flight holds the rows still, so no menu opens over them"
    );
}

#[test]
fn a_page_without_access_offers_nothing() {
    let ctx = context();
    let mut library = library_selected(
        "spotify:playlist:a",
        vec![track("a", "t0"), track("b", "t1")],
    );
    let _ = tracklist_frame(&ctx, Table::Full, &mut library);

    assert_eq!(
        crate::view::carry::take_offer(&ctx),
        None,
        "a read that named no state offers no reorder"
    );
}

#[test]
fn a_reverted_move_says_so_above_the_rows() {
    let ctx = context();
    let tracks = vec![track("a", "t0"), track("b", "t1"), track("c", "t2")];
    let mut library = library_with(tracks.clone());
    assert!(
        library
            .move_tracks("spotify:playlist:a", &[0], 2, &[tracks[0].clone()])
            .is_some(),
        "the page takes the move first"
    );
    library.apply(&Update::MoveFailed {
        playlist: "spotify:playlist:a".to_owned(),
        failure: Failure::Save,
    });

    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let drawn = drawn_galleys(&output);
    let notice = crate::signin::failure_message(Failure::Save);
    assert!(
        galley_top(&drawn, notice) < galley_top(&drawn, "t0"),
        "the failure heads the rows it puts back"
    );
    assert_eq!(painted_rows(&output), vec![0, 1, 2], "the order is back");
}

#[test]
fn the_failed_page_draws_its_failure_and_a_retry_that_reloads() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0")]);
    library.apply(&Update::TracksFailed {
        playlist: "spotify:playlist:a".to_owned(),
        failure: Failure::Load,
    });
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let painted = drawn_texts(&output);
    assert!(
        painted.contains(&crate::signin::failure_message(Failure::Load)),
        "the failure is drawn: {painted:?}"
    );
    assert!(painted.contains(&"Retry"), "with the retry beside it");

    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let retry = galley_at(&output, "Retry");
    assert_eq!(
        click(
            &ctx,
            retry + Vec2::new(4.0, 4.0),
            &mut library,
            &mut transport,
            &mut queue
        ),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        }),
        "the retry asks for the page again"
    );
}

/// The same page, read at a revision the user may not write.
fn read_only_library_with(tracks: Vec<TrackEntry>) -> Library {
    let mut library = Library::default();
    library.select("spotify:playlist:a");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:a".to_owned(),
        tracks,
        cover: None,
        access: access(false),
    });
    library
}
