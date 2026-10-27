use super::super::*; // the module under test
use super::*; // the shared fixtures, from tests/mod.rs

#[test]
fn the_hero_draws_the_artists_name() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), Vec::new()));
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

    assert!(
        texts.contains(&"Vela Moreno"),
        "the hero's title: {texts:?}"
    );
    assert!(
        !texts.contains(&"ARTIST"),
        "the plate band carries no eyebrow: {texts:?}"
    );
}

/// Whether `artist`'s popular list holds `index` as part of its selection.
fn picked(ctx: &Context, artist: &str, index: usize) -> bool {
    crate::view::carry::List::open(ctx, egui::Id::unique(("artist-popular", artist)), 2)
        .look(index)
        .selected
}

#[test]
fn a_single_click_selects_and_a_double_click_plays_it() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let top = || {
        vec![
            track("spotify:track:a", "One", "Vela Moreno", 222_000),
            track("spotify:track:b", "Two", "Vela Moreno", 198_000),
        ]
    };
    let mut library = library_with(detail(top(), Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let title = popular_cells(&ctx, size, 1, 2).0;
    let command = click(
        &ctx,
        size,
        title.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(command, None, "a single click plays nothing");
    assert!(transport.now_playing.is_none(), "and starts nothing");
    assert!(picked(&ctx, ARTIST, 1), "the row under the click is picked");
    assert!(!picked(&ctx, ARTIST, 0), "and only that row");

    // A fresh page for the double click: a third click inside the window is
    // a triple click, and that plays nothing.
    let ctx = context();
    let mut library = library_with(detail(top(), Vec::new()));
    let mut transport = Transport::default();
    let command = click(
        &ctx,
        size,
        title.center(),
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
fn release_blocks_keep_their_own_selection_until_navigation() {
    let ctx = context();
    let mut library = library_with(detail(
        Vec::new(),
        vec![
            release("spotify:album:r1", "Album One", &["spotify:track:1"]),
            release("spotify:album:r2", "Album Two", &["spotify:track:2"]),
        ],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    // The blocks ask for their labels, then the answers land.
    let _ = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        library.releases.requests().len(),
        2,
        "both blocks ask for their labels"
    );
    for (release_uri, title) in [
        ("spotify:album:r1", "First One"),
        ("spotify:album:r2", "Second One"),
    ] {
        library.apply(&Update::ReleaseTracks {
            release: release_uri.to_owned(),
            tracks: vec![track(
                &format!("spotify:track:{release_uri}"),
                title,
                "Vela Moreno",
                200_000,
            )],
        });
    }

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(drawn(&output, "First One"), "the first block's row");
    assert!(drawn(&output, "Second One"), "the second block's row");
    let row = text_rect(&output, "Second One").expect("the second block's row");

    let command = click(
        &ctx,
        tall(),
        row.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "a single click plays nothing");
    assert!(
        release_picked(&ctx, "spotify:album:r2", 0),
        "the block under the click picks its row"
    );
    assert!(
        !release_picked(&ctx, "spotify:album:r1", 0),
        "the other block keeps its own selection"
    );

    let _ = library.open(Page::Album("spotify:album:other".to_owned()));
    crate::view::carry::follow_page(&ctx, library.page());

    assert!(
        !release_picked(&ctx, "spotify:album:r1", 0),
        "navigation forgets the first block's selection"
    );
    assert!(
        !release_picked(&ctx, "spotify:album:r2", 0),
        "and the second's"
    );
}

#[test]
fn a_release_heading_click_opens_the_album_page() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(
        Vec::new(),
        vec![release(
            "spotify:album:r1",
            "Album One",
            &["spotify:track:1"],
        )],
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
    let name = text_rect(&output, "Album One").expect("the release's name is painted");
    let command = click(
        &ctx,
        size,
        name.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Album("spotify:album:r1".to_owned())),
        "the release's heading does not open its album"
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: "spotify:album:r1".to_owned()
        })
    );
    assert!(!queue.can_next(), "opening a page must not start the queue");
}

#[test]
fn a_double_click_on_a_popular_row_plays_that_track() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let top = vec![
        track("spotify:track:a", "One", "Vela Moreno", 222_000),
        track("spotify:track:b", "Two", "Vela Moreno", 198_000),
    ];
    let mut library = library_with(detail(top, Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let title = popular_cells(&ctx, size, 1, 2).0;
    let command = click(
        &ctx,
        size,
        title.center(),
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

#[test]
fn the_bars_play_starts_the_top_list() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let top = vec![
        track("spotify:track:a", "One", "Vela Moreno", 222_000),
        track("spotify:track:b", "Two", "Vela Moreno", 198_000),
    ];
    let mut library = library_with(detail(top, Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (play, _) = bar_tiles(&ctx, size, "Follow");
    let command = click(
        &ctx,
        size,
        play.center(),
        1,
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
        "the hero plays the artist from the top of the list"
    );
    assert!(
        queue.can_next(),
        "the whole top list is queued, not just its first track"
    );
}

#[test]
fn the_follow_press_asks_for_the_negated_state() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(
        vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
        Vec::new(),
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, follow) = bar_tiles(&ctx, size, "Follow");
    let command = click(
        &ctx,
        size,
        follow.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::SetFollow {
            artist: ARTIST.to_owned(),
            followed: true,
        }),
        "an unconfirmed artist is followed, not unfollowed"
    );
    assert_eq!(
        library.follow.label(),
        "Follow",
        "the press alone must not flip the label"
    );
}

#[test]
fn the_follow_press_targets_unfollow_once_the_server_confirmed() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(Vec::new(), Vec::new()));
    library.apply(&Update::Followed {
        artist: ARTIST.to_owned(),
        followed: true,
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, follow) = bar_tiles(&ctx, size, "Following");
    let command = click(
        &ctx,
        size,
        follow.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::SetFollow {
            artist: ARTIST.to_owned(),
            followed: false,
        })
    );
}

#[test]
fn the_follow_press_writes_nothing_while_the_sidebar_removes_that_artist() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(Vec::new(), Vec::new()));
    library.apply(&Update::FollowedArtists(vec![FollowedArtist {
        uri: ARTIST.to_owned(),
        name: "Vela Moreno".to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }]));
    library.apply(&Update::Followed {
        artist: ARTIST.to_owned(),
        followed: true,
    });
    assert!(
        library
            .remove_from_library(&crate::menu::LibraryItem {
                kind: crate::sidebar_kind::SidebarKind::Artists,
                uri: ARTIST.to_owned(),
                name: "Vela Moreno".to_owned(),
            })
            .is_some(),
        "the sidebar removal never started"
    );
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, follow) = bar_tiles(&ctx, size, "Following");
    let command = click(
        &ctx,
        size,
        follow.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(command, None, "a follow write raced the sidebar's removal");
    assert!(
        !library.follow.pending(),
        "the page armed a write the sidebar already sent"
    );
    assert_eq!(
        library.follow.label(),
        "Following",
        "the label moved before the removal answered"
    );
}

#[test]
fn a_follow_failure_draws_its_copy_beside_the_confirmed_label() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    let mut library = library_with(detail(Vec::new(), Vec::new()));
    library.apply(&Update::Followed {
        artist: ARTIST.to_owned(),
        followed: true,
    });
    let _ = library.follow.pressed();
    library.apply(&Update::FollowFailed {
        artist: ARTIST.to_owned(),
        failure: Failure::Save,
    });
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
    let texts = drawn_texts(&output);
    let copy = crate::signin::failure_message(Failure::Save);
    assert!(
        texts.contains(&copy),
        "the failed write's copy is drawn: {texts:?}"
    );
    assert!(
        texts.contains(&"Following"),
        "the label stays where the server last left it: {texts:?}"
    );

    // The row's name box, in front of the control: the row has no second
    // line, so a message may not add one.
    let (play, follow) = bar_tiles(&ctx, size, "Following");
    let hint = text_rect(&output, copy).expect("the failure's copy is painted");
    assert!(
        hint.left() >= play.right() + crate::theme::actionbar::GAP - 0.01,
        "the copy starts where the name's box starts: {hint:?} against {play:?}"
    );
    assert!(
        hint.right() <= follow.left() - crate::theme::actionbar::GAP + 0.01,
        "the copy stops short of the control: {hint:?} against {follow:?}"
    );
    assert!(
        (hint.center().y - follow.center().y).abs() < 1.0,
        "the copy shares the control's line: {hint:?} against {follow:?}"
    );

    // The row keeps its resting height, so the message moved nothing below
    // it: the sections sit where they sit without one.
    let head = text_rect(&output, "Popular").expect("the Popular head is painted");
    let mut quiet = library_with(detail(Vec::new(), Vec::new()));
    quiet.apply(&Update::Followed {
        artist: ARTIST.to_owned(),
        followed: true,
    });
    let (_, plain) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut quiet,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        head,
        text_rect(&plain, "Popular").expect("the Popular head is painted without one"),
        "a failure may not push the page down"
    );
}

#[test]
fn the_follow_press_still_writes_when_the_bar_is_pinned() {
    let ctx = context();
    let size = Vec2::new(900.0, 800.0);
    // A discography long enough to scroll the bar out of the viewport.
    let releases: Vec<Release> = (1..=12)
        .map(|index| {
            release(
                &format!("spotify:album:r{index}"),
                &format!("Release {index}"),
                &["spotify:track:a", "spotify:track:b"],
            )
        })
        .collect();
    let mut library = library_with(detail(
        vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
        releases,
    ));
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
    let follow = crate::layout::actionbar::solve(bar, Kind::Other, bar_parts(&ctx, "Follow"))
        .follow
        .expect("the pinned bar keeps its text control");

    let command = click(
        &ctx,
        size,
        follow.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::SetFollow {
            artist: ARTIST.to_owned(),
            followed: true,
        }),
        "a pinned bar's Follow writes what it wrote in the band"
    );
}

/// The bar's play tile and its text control, solved by the row's own solver.
fn bar_tiles(ctx: &Context, size: Vec2, label: &str) -> (Rect, Rect) {
    let placed =
        crate::layout::actionbar::solve(bar_row(ctx, size), Kind::Other, bar_parts(ctx, label));
    (
        placed.play,
        placed.follow.expect("the artist's bar has a text slot"),
    )
}
