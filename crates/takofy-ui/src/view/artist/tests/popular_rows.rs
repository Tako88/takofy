use egui::{Event, Pos2};
use takofy_model::TrackEntry;

use super::super::*;
use super::test_support::*;

/// A track that names the record it is on, which is the cell's credit.
fn on_album(uri: &str, title: &str, album: &str) -> TrackEntry {
    TrackEntry {
        album: Some(album.to_owned()),
        album_uri: Some(uri.to_owned()),
        ..track("spotify:track:a", title, "Vela Moreno", 200_000)
    }
}

#[test]
fn a_popular_row_asks_for_its_own_cover() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let top = vec![
        TrackEntry {
            cover: Some(cover("a")),
            ..track("spotify:track:a", "One", "Vela Moreno", 222_000)
        },
        TrackEntry {
            cover: Some(cover("b")),
            ..track("spotify:track:b", "Two", "Vela Moreno", 198_000)
        },
    ];
    let mut library = library_with(detail(top, Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();

    art.begin_frame();
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let requests = art.requests(now);

    assert_eq!(
        requests.len(),
        2,
        "one lookup per on-screen row: {requests:?}"
    );
    assert!(
        requests
            .iter()
            .all(|command| matches!(command, Command::Fetch { .. })),
        "{requests:?}"
    );

    let (title_cell, _, _) = popular_cells(&ctx, size, 0, 2);
    let name = text_rect(&output, "One").expect("the row's name");
    assert_eq!(
        name.left(),
        title_cell.left() + extent::LIBRARY_ART + space::S3,
        "the name starts past the row's own art box and its gutter"
    );
}

#[test]
fn a_popular_row_opens_its_album() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(
        vec![on_album("spotify:album:slow-tide", "One", "Slow Tide")],
        Vec::new(),
    ));
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
    let words = text_rect(&output, "Slow Tide").expect("the credit is painted");
    let command = click(
        &ctx,
        size,
        words.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Album("spotify:album:slow-tide".to_owned())),
        "the credit opens the record it names"
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: "spotify:album:slow-tide".to_owned()
        })
    );
    assert!(!queue.can_next(), "opening a page must not start the queue");
}

#[test]
fn a_click_past_the_credit_selects_the_row_and_a_double_click_plays() {
    // The link is the painted words, not the credit cell: a click past them
    // belongs to the row.
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(
        vec![on_album("spotify:album:slow-tide", "One", "Slow Tide")],
        Vec::new(),
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, credit, row) = popular_cells(&ctx, size, 0, 1);
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let words = text_rect(&output, "Slow Tide").expect("the credit is painted");
    let past = Pos2::new(
        words.right() + (credit.right() - words.right()) / 2.0,
        row.center().y,
    );
    assert!(
        words.right() < past.x && past.x < credit.right(),
        "the probe at x={} must be past the credit, which ends at x={}",
        past.x,
        words.right()
    );

    let command = click(
        &ctx,
        size,
        past,
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a click past the words is the row's own");
    assert_eq!(
        library.page(),
        Some(&Page::Artist(ARTIST.to_owned())),
        "a click past the credit must not navigate"
    );
    assert!(!queue.can_next(), "a click past the words must not play");

    // A fresh page, so the double click is not the third of a sequence.
    let ctx = context();
    let mut library = library_with(detail(
        vec![on_album("spotify:album:slow-tide", "One", "Slow Tide")],
        Vec::new(),
    ));
    let command = click(
        &ctx,
        size,
        past,
        2,
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
        "a double click past the credit plays the row"
    );
    assert_eq!(
        library.page(),
        Some(&Page::Artist(ARTIST.to_owned())),
        "playing the row stays on the artist"
    );
}

#[test]
fn a_popular_row_without_an_album_still_plays_on_a_double_click() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    // No album on the entry, so the credit cell is empty and takes no press.
    let mut library = library_with(detail(
        vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
        Vec::new(),
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, credit, _) = popular_cells(&ctx, size, 0, 1);
    let command = click(
        &ctx,
        size,
        credit.center(),
        2,
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
        "an empty credit cell is not a link, so the press reaches the row"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("spotify:track:a")
    );
}

#[test]
fn an_open_does_not_overwrite_a_same_frame_command() {
    let ctx = context();
    // Keep the focused row's focus while the pointer presses elsewhere, so
    // one frame can answer both the keyboard and the credit's click.
    ctx.options_mut(|options| {
        options.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never;
    });
    let size = Vec2::new(900.0, 800.0);
    let top = vec![
        track("spotify:track:a", "One", "Vela Moreno", 222_000),
        on_album("spotify:album:slow-tide", "Two", "Slow Tide"),
    ];
    let mut library = library_with(detail(top, Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    // Both solves run before the focus is taken: a solve is a frame of its
    // own, and a frame that does not draw the focused row drops it.
    let (title, _, _) = popular_cells(&ctx, size, 0, 2);
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let at = text_rect(&output, "Slow Tide")
        .expect("the second row's credit is painted")
        .center();

    // Focus the first row, so Enter on it asks for a play.
    let _ = click(
        &ctx,
        size,
        title.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        ctx.memory(|memory| memory.focused()).is_some(),
        "the row took the keyboard"
    );
    // Press on the second row's credit first, so the release that reports
    // the click lands in the frame that also answers Enter on the first.
    let _ = frame(
        &ctx,
        size,
        vec![
            Event::PointerMoved(at),
            Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let events = vec![
        Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        },
        Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        },
        Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let (command, _) = frame(
        &ctx,
        size,
        events,
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
        "the same-frame play is not replaced by the open"
    );
    assert_eq!(
        library.page(),
        Some(&Page::Artist(ARTIST.to_owned())),
        "the deferred open did not navigate"
    );
}

#[test]
fn the_head_names_the_album_column() {
    let ctx = context();
    let mut library = library_with(detail(
        vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
        Vec::new(),
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec2::new(900.0, 800.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);

    assert_eq!(
        texts.iter().filter(|text| **text == "ARTIST").count(),
        0,
        "the hero's eyebrow is gone: {texts:?}"
    );
    assert_eq!(
        texts.iter().filter(|text| **text == "ALBUM").count(),
        1,
        "the popular table's column heading: {texts:?}"
    );
}

#[test]
fn the_loading_skeleton_and_the_loaded_popular_put_the_title_at_the_same_left() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = crate::library::Library::default();
    library.open(Page::Artist(ARTIST.to_owned()));
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
    let loading = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.fill == Tokens::DARK.palette.skeleton_rule
                    && (rect.rect.height() - extent::SKELETON_LINE).abs() < 0.01 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .next()
        .expect("the loading list holds a title bar open");

    let ctx = context();
    let mut library = library_with(detail(
        vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
        Vec::new(),
    ));
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
    let loaded = text_rect(&output, "One").expect("the row's name");

    assert_eq!(
        loading.left(),
        loaded.left(),
        "the title does not shift when Popular lands"
    );
}
