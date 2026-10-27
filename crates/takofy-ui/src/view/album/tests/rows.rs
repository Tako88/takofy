use egui::{Context, Pos2};

use takofy_model::AlbumDetail;

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::message::Update;
use crate::theme::space;
use crate::view::track_list;

/// The centre of the like column of the `index`-th row, solved the way the
/// page solves its columns.
fn heart_point_for(ctx: &Context, index: usize, count: usize) -> Pos2 {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        ui.add_space(space::S3);
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
        actionbar::row_rect(ui);
        crate::view::inner(ui, |ui| {
            let columns =
                table_layout::columns(ui.available_width(), Table::Full.columns().for_plays());
            track_list::head(ui, columns, &tokens);
            let list = track_list::list_rect(ui, count);
            let row = table_layout::row_rect(list, index, extent::ROW_H);
            found = Some(columns.at(row.min).like.center());
        });
    });
    found.expect("the heart's column lays out")
}

/// Whether `album`'s own list holds `index` as part of its selection.
fn picked(ctx: &Context, album: &str, index: usize) -> bool {
    crate::view::carry::List::open(ctx, egui::Id::unique(("album-tracks", album)), 2)
        .look(index)
        .selected
}

#[test]
fn a_heart_click_posts_one_ask_with_the_rows_uri() {
    let ctx = context();
    let mut library = library_with(detail(vec![
        track("spotify:track:a", "One", "Band", 222_000),
        track("spotify:track:b", "Two", "Band", 198_000),
    ]));
    assert!(library.likes.request(true));
    library.likes.apply(&crate::likes::Event::Set(Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let point = heart_point_for(&ctx, 1, 2);
    // Warm up so the pointer is known to the hit test before the press.
    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let _ = frame(
        &ctx,
        vec![egui::Event::PointerMoved(point)],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let command = click(
        &ctx,
        point,
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(command, None, "a heart is not a command of its own");
    let ask = heart::take(&ctx).expect("an ask was posted");
    assert_eq!(ask.tracks, vec!["spotify:track:b".to_owned()]);
    assert!(ask.liked, "an unsaved row asks to be saved");
}

#[test]
fn a_right_click_posts_the_albums_menu() {
    let ctx = context();
    let mut library = library_with(detail(vec![
        track("spotify:track:a", "One", "Band", 222_000),
        track("spotify:track:b", "Two", "Band", 198_000),
    ]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let rect = row_rect_for(&ctx, 1, 2);

    for event in crate::view::test_support::secondary_click_events(rect.center()) {
        let _ = frame(
            &ctx,
            vec![event],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    let request = crate::view::context_menu::take_request(&ctx)
        .expect("a right-click on an album row posted a request");
    assert_eq!(request.targets.len(), 1);
    assert_eq!(request.targets[0].uri, "spotify:track:b");
    assert_eq!(request.page, Some(Page::Album(ALBUM.to_owned())));
    assert!(
        !request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::GoToAlbum),
        "the album this page already is is not one of its verbs"
    );
}

#[test]
fn a_single_click_selects_and_a_double_click_plays_it() {
    let tracks = || {
        vec![
            track("spotify:track:a", "One", "Band", 222_000),
            track("spotify:track:b", "Two", "Band", 198_000),
        ]
    };
    let ctx = context();
    let mut library = library_with(detail(tracks()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let rect = row_rect_for(&ctx, 1, 2);
    let command = click(
        &ctx,
        rect.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(command, None, "a single click plays nothing");
    assert!(transport.now_playing.is_none(), "and starts nothing");
    assert!(picked(&ctx, ALBUM, 1), "the row under the click is picked");
    assert!(!picked(&ctx, ALBUM, 0), "and only that row");

    // A fresh page for the double click: a third click inside the window is
    // a triple click, and that plays nothing.
    let ctx = context();
    let mut library = library_with(detail(tracks()));
    let mut transport = Transport::default();
    let command = click(
        &ctx,
        rect.center(),
        2,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:track:b".to_owned()
        }),
        "a double click starts the track"
    );
}

#[test]
fn a_selection_stays_on_the_album_it_was_made_on() {
    let tracks = || {
        vec![
            track("spotify:track:a", "One", "Band", 222_000),
            track("spotify:track:b", "Two", "Band", 198_000),
        ]
    };
    let ctx = context();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let mut first = library_with(detail(tracks()));
    let rect = row_rect_for(&ctx, 1, 2);
    let _ = click(
        &ctx,
        rect.center(),
        1,
        &mut first,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let mut second = Library::default();
    let _ = second.open(Page::Album(OTHER_ALBUM.to_owned()));
    second.apply(&Update::Album(AlbumDetail {
        uri: OTHER_ALBUM.to_owned(),
        ..detail(tracks())
    }));
    let _ = frame(
        &ctx,
        Vec::new(),
        &mut second,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert!(picked(&ctx, ALBUM, 1), "the first album keeps its pick");
    assert!(
        !picked(&ctx, OTHER_ALBUM, 1),
        "the second album does not show it against the same index"
    );
}

#[test]
fn a_double_click_on_a_row_plays_that_track() {
    let ctx = context();
    let tracks = vec![
        track("spotify:track:a", "One", "Band", 222_000),
        track("spotify:track:b", "Two", "Band", 198_000),
    ];
    let mut library = library_with(detail(tracks));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let rect = row_rect_for(&ctx, 1, 2);
    let command = click(
        &ctx,
        rect.center(),
        2,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:track:b".to_owned()
        }),
        "the row under the double click is what plays"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("spotify:track:b")
    );
}
