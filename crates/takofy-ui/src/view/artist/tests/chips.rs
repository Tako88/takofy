use egui::Context;
use takofy_model::ReleaseKind;

use super::super::*;
use super::test_support::*;
use crate::discography::{Chip, chips};
use crate::message::Update;
use crate::theme::stroke;

#[test]
fn the_row_has_all_plus_the_kinds_the_artist_has() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);

    assert!(texts.contains(&"ALL"), "{texts:?}");
    assert!(texts.contains(&"ALBUMS"), "{texts:?}");
    assert!(texts.contains(&"EPS"), "{texts:?}");
    assert!(!texts.contains(&"SINGLES"), "no single: {texts:?}");
    assert!(
        !texts.contains(&"COMPILATIONS"),
        "no compilation: {texts:?}"
    );
}

#[test]
fn the_row_sits_between_the_head_and_the_first_block() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let head = text_rect(&output, "Discography").expect("the section head");
    let first_chip = chip_rect(&output, Chip::All).expect("the All chip");
    let first_block = text_rect(&output, "Album One").expect("the first block");
    assert!(
        head.bottom() < first_chip.top(),
        "the row is under the head: {head:?} against {first_chip:?}"
    );
    assert!(
        first_chip.bottom() < first_block.top(),
        "the row is above the first block: {first_chip:?} against {first_block:?}"
    );
}

#[test]
fn all_is_pressed_on_first_draw() {
    let ctx = context();
    let releases = mixed();
    let mut library = library_with(detail(Vec::new(), releases.clone()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(library.filter.chosen(), Chip::All);
    assert_eq!(
        library.filter.effective(&chips(&releases)),
        Chip::All,
        "the pressed chip is All, so every block draws"
    );
    for name in ["Album One", "An EP", "Album Two"] {
        assert!(drawn(&output, name), "{name} is drawn: {name}");
    }
}

#[test]
fn opening_another_artist_draws_all_even_when_it_has_the_same_kind() {
    let ctx = context();
    let releases = mixed();
    let mut library = library_with(detail(Vec::new(), releases.clone()));
    library.filter.choose(Chip::Kind(ReleaseKind::Ep));
    let mut other = detail(Vec::new(), releases);
    other.uri = OTHER_ARTIST.to_owned();
    library.open(Page::Artist(other.uri.clone()));
    library.apply(&Update::Artist(other));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.filter.chosen(),
        Chip::All,
        "another artist's page starts on All"
    );
    assert!(
        drawn(&output, "EPS"),
        "the other artist has an EP chip too: {:?}",
        drawn_texts(&output)
    );
    for name in ["Album One", "An EP", "Album Two"] {
        assert!(
            drawn(&output, name),
            "All filters nothing on the new artist: {name}"
        );
    }
}

#[test]
fn pressing_a_chip_filters_the_blocks_in_place() {
    let ctx = context();
    let size = tall();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, before) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
    let page = library.page().cloned();
    let can_back = library.can_back();

    let command = click(
        &ctx,
        size,
        albums.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(command, None, "a filter press asks for nothing");
    assert_eq!(library.page(), page.as_ref(), "it does not navigate");
    assert_eq!(library.can_back(), can_back, "it adds no history entry");
    assert!(
        matches!(library.artist, Load::Ready(_)),
        "the artist is not refetched"
    );
    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Album),
        "the pressed chip is the choice"
    );

    let (_, after) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(drawn(&after, "Album One"), "an album block stays");
    assert!(drawn(&after, "Album Two"), "every album block stays");
    assert!(
        !drawn(&after, "An EP"),
        "the EP block draws nothing at all: {:?}",
        drawn_texts(&after)
    );
}

#[test]
fn pressing_the_pressed_chip_does_nothing() {
    let ctx = context();
    let size = tall();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, before) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
    let _ = click(
        &ctx,
        size,
        albums.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let _ = click(
        &ctx,
        size,
        albums.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let (_, after) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Album),
        "the row is exclusive: a second press is not a way back out"
    );
    assert!(!drawn(&after, "An EP"), "the filter is still applied");
}

#[test]
fn all_restores_every_block() {
    let ctx = context();
    let size = tall();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, before) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
    let all = chip_rect(&before, Chip::All).expect("the All chip");
    let _ = click(
        &ctx,
        size,
        albums.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let _ = click(
        &ctx,
        size,
        all.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let (_, after) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(library.filter.chosen(), Chip::All);
    for name in ["Album One", "An EP", "Album Two"] {
        assert!(drawn(&after, name), "{name} is back");
    }
}

#[test]
fn a_hidden_block_draws_nothing_and_reserves_nothing() {
    let ctx = context();
    let size = tall();
    let mut unfiltered = library_with(detail(Vec::new(), mixed()));
    let mut filtered = library_with(detail(Vec::new(), mixed()));
    filtered.filter.choose(Chip::Kind(ReleaseKind::Ep));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, before) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut unfiltered,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let (_, after) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut filtered,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&after);

    assert!(!texts.contains(&"Album One"), "{texts:?}");
    assert!(!texts.contains(&"Album Two"), "{texts:?}");

    let first_unfiltered = text_rect(&before, "Album One").expect("the first block");
    let first_filtered = text_rect(&after, "An EP").expect("the only block");
    assert!(
        (first_filtered.top() - first_unfiltered.top()).abs() < 0.01,
        "the hidden blocks reserved no space: {} against {}",
        first_filtered.top(),
        first_unfiltered.top()
    );
}

#[test]
fn a_hidden_block_has_no_wants() {
    let ctx = context();
    let size = tall();
    // The shelf takes the oldest, the middle and the newest covered
    // release, so putting the two blocks at the unpicked middle positions
    // keeps their art out of the band: a fetch of it can only be the
    // block's own. The years run in wire order, so the sort keeps that order
    // and the picks are indices 0, 2 and 4.
    let mut releases = vec![
        release("spotify:album:zero", "Zero", &["spotify:track:0"]),
        release("spotify:album:r1", "Album One", &["spotify:track:1"]),
        release("spotify:album:two", "Two", &["spotify:track:2"]),
        release_of_kind(
            "spotify:ep:r2",
            "An EP",
            ReleaseKind::Ep,
            &["spotify:track:3"],
        ),
        release("spotify:album:four", "Four", &["spotify:track:4"]),
    ];
    for (index, year, tail) in [
        (0, 2018, "zero"),
        (1, 2019, "album"),
        (2, 2020, "two"),
        (3, 2021, "ep"),
        (4, 2022, "four"),
    ] {
        releases[index].year = Some(year);
        releases[index].cover = Some(cover(tail));
    }
    let now = Instant::now();

    // The control: unfiltered, both blocks are on screen and ask.
    let mut open = library_with(detail(Vec::new(), releases.clone()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    art.begin_frame();
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut open,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(drawn(&output, "An EP"), "the EP block is on screen");
    assert!(drawn(&output, "Album One"), "the album block is on screen");
    let wanted_labels = open.releases.requests();
    let wanted_covers = art.requests(now);
    assert!(
        wanted_labels
            .iter()
            .any(|command| matches!(command, Command::LoadReleaseTracks { release, .. } if release == "spotify:album:r1")),
        "the control frame asks for the album's labels: {wanted_labels:?}"
    );
    assert!(
        wanted_covers
            .iter()
            .any(|command| matches!(command, Command::Fetch { url } if *url == cover_url("album"))),
        "the control frame asks for the album's cover: {wanted_covers:?}"
    );

    // The hidden album asks for neither.
    let mut hidden = library_with(detail(Vec::new(), releases));
    hidden.filter.choose(Chip::Kind(ReleaseKind::Ep));
    let mut art = Art::default();
    art.begin_frame();
    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut hidden,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let wanted_labels = hidden.releases.requests();
    let wanted_covers = art.requests(now);
    assert!(
        !wanted_labels.iter().any(|command| matches!(
            command,
            Command::LoadReleaseTracks { release, .. } if release == "spotify:album:r1"
        )),
        "a hidden block asks for no labels: {wanted_labels:?}"
    );
    assert!(
        !wanted_covers
            .iter()
            .any(|command| matches!(command, Command::Fetch { url } if *url == cover_url("album"))),
        "a hidden block fetches no cover: {wanted_covers:?}"
    );
    assert!(
        wanted_covers
            .iter()
            .any(|command| matches!(command, Command::Fetch { url } if *url == cover_url("ep"))),
        "the block the filter keeps still fetches its cover: {wanted_covers:?}"
    );
}

#[test]
fn an_on_screen_matching_block_still_asks_for_labels() {
    let ctx = context();
    let size = tall();
    let mut library = library_with(detail(Vec::new(), mixed()));
    library.filter.choose(Chip::Kind(ReleaseKind::Ep));
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

    assert!(drawn(&output, "An EP"));
    assert_eq!(
        library.releases.requests(),
        vec![Command::LoadReleaseTracks {
            release: "spotify:ep:r2".to_owned(),
            tracks: vec!["spotify:track:2".to_owned()],
        }],
        "the block the filter keeps is still labelled"
    );
}

#[test]
fn the_aside_keeps_the_artists_totals_while_filtered() {
    let ctx = context();
    let size = tall();
    let mut library = library_with(detail(Vec::new(), mixed()));
    library.filter.choose(Chip::Kind(ReleaseKind::Album));
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

    let aside = discography_meta(3, 3).to_uppercase();
    assert!(
        drawn(&output, &aside),
        "the aside describes the artist, not the filter: {aside}"
    );
}

#[test]
fn a_choice_with_no_chip_after_a_reload_draws_all() {
    let ctx = context();
    let size = tall();
    let mut library = library_with(detail(Vec::new(), mixed()));
    library.filter.choose(Chip::Kind(ReleaseKind::Single));

    // A reload of the same artist: home, then Back, then the reply.
    library.to_playlists();
    let _ = library.back();
    library.apply(&Update::Artist(detail(Vec::new(), mixed())));
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

    for name in ["Album One", "An EP", "Album Two"] {
        assert!(
            drawn(&output, name),
            "{name} draws under the fallback to All"
        );
    }
    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Single),
        "the choice is remembered, not replaced"
    );
}

#[test]
fn a_single_kind_artist_still_gets_all_and_that_kind() {
    let ctx = context();
    let mut library = library_with(detail(
        Vec::new(),
        vec![release("spotify:album:r1", "One", &["spotify:track:1"])],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert!(drawn(&output, "ALL"), "{:?}", drawn_texts(&output));
    assert!(drawn(&output, "ALBUMS"), "{:?}", drawn_texts(&output));
    assert!(!drawn(&output, "EPS"), "{:?}", drawn_texts(&output));
}

#[test]
fn an_artist_with_releases_always_has_the_row_and_the_field() {
    let ctx = context();
    let mut library = library_with(detail(
        Vec::new(),
        vec![release_of_kind(
            "spotify:audiobook:r1",
            "A Book",
            ReleaseKind::Other,
            &["spotify:track:1"],
        )],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert!(
        drawn(&output, "ALL"),
        "a single kind still gets the row: {:?}",
        drawn_texts(&output)
    );
    assert!(
        drawn(&output, "FILTER"),
        "the field must exist even when there is nothing to chip between: {:?}",
        drawn_texts(&output)
    );
    assert!(
        drawn(&output, "A Book"),
        "the Other release stays reachable under All"
    );
}

#[test]
fn no_releases_draws_no_row_and_no_field() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);

    assert!(texts.contains(&"No releases."), "{texts:?}");
    assert!(
        !texts.contains(&"ALL"),
        "there is nothing to filter: {texts:?}"
    );
    assert!(
        !texts.contains(&"FILTER"),
        "a discography with no releases has no field: {texts:?}"
    );
}

#[test]
fn the_chip_row_starts_at_the_gutter() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        tall(),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let all = chip_rect(&output, Chip::All).expect("the All chip");
    assert!(
        (all.left() - space::GUTTER).abs() < 0.01,
        "the row starts at {}, the gutter is {}",
        all.left(),
        space::GUTTER
    );
}

#[test]
fn at_a_wide_pane_the_row_is_one_line() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), every_kind()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let sizes = chip_sizes(&ctx);
    let needed: f32 =
        sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
    let wide = needed
        + 2.0 * space::GUTTER
        + crate::theme::cluster::FIELD_W
        + crate::theme::cluster::GAP
        + 2.0 * space::GUTTER;

    let (_, output) = frame(
        &ctx,
        Vec2::new(wide, 1500.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let tops: Vec<f32> = [
        Chip::All,
        Chip::Kind(ReleaseKind::Album),
        Chip::Kind(ReleaseKind::Ep),
        Chip::Kind(ReleaseKind::Single),
        Chip::Kind(ReleaseKind::Compilation),
    ]
    .into_iter()
    .filter_map(|chip| chip_rect(&output, chip))
    .map(|rect| rect.top())
    .collect();
    assert_eq!(tops.len(), 5, "every chip is drawn");
    for top in &tops {
        assert!(
            (top - tops[0]).abs() < 0.01,
            "one line: {tops:?} at {wide} wide"
        );
    }
}

#[test]
fn at_a_narrow_pane_the_row_wraps_inside_the_gutters() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), every_kind()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let sizes = chip_sizes(&ctx);
    let needed: f32 =
        sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
    let narrow = 2.0 * space::GUTTER + needed / 2.0;

    let (_, output) = frame(
        &ctx,
        Vec2::new(narrow, 1500.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let rects: Vec<Rect> = [
        Chip::All,
        Chip::Kind(ReleaseKind::Album),
        Chip::Kind(ReleaseKind::Ep),
        Chip::Kind(ReleaseKind::Single),
        Chip::Kind(ReleaseKind::Compilation),
    ]
    .into_iter()
    .filter_map(|chip| chip_rect(&output, chip))
    .collect();
    assert_eq!(rects.len(), 5, "every chip is drawn");
    let first = rects[0].top();
    assert!(
        rects.iter().any(|rect| (rect.top() - first).abs() > 0.01),
        "the row wraps at {narrow}: {rects:?}"
    );
    for rect in &rects {
        assert!(
            rect.left() >= space::GUTTER - 0.01,
            "inside the near gutter: {rect:?}"
        );
        assert!(
            rect.right() <= narrow - space::GUTTER + 0.01,
            "inside the far gutter: {rect:?}"
        );
    }
}

#[test]
fn the_row_grows_with_its_wraps_and_pushes_the_blocks_down() {
    let ctx = context();
    let sizes = chip_sizes(&ctx);
    let needed: f32 =
        sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
    // One point short of fitting on one line: the row wraps a chip, and the
    // blocks keep a legible width.
    let narrow = needed + 2.0 * space::GUTTER + crate::theme::cluster::FIELD_W + space::S1 - 1.0;
    let wide = narrow + 4.0;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let mut wide_library = library_with(detail(Vec::new(), every_kind()));
    let (_, wide_out) = frame(
        &ctx,
        Vec2::new(wide, 1500.0),
        Vec::new(),
        &mut wide_library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let mut narrow_library = library_with(detail(Vec::new(), every_kind()));
    let (_, narrow_out) = frame(
        &ctx,
        Vec2::new(narrow, 1500.0),
        Vec::new(),
        &mut narrow_library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let chips_on_narrow: Vec<Rect> = [
        Chip::All,
        Chip::Kind(ReleaseKind::Album),
        Chip::Kind(ReleaseKind::Ep),
        Chip::Kind(ReleaseKind::Single),
        Chip::Kind(ReleaseKind::Compilation),
    ]
    .into_iter()
    .filter_map(|chip| chip_rect(&narrow_out, chip))
    .collect();
    let mut tops: Vec<f32> = chips_on_narrow.iter().map(Rect::top).collect();
    tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    let rows = tops.len();
    assert!(rows >= 2, "the narrow row wraps: {tops:?}");

    let chip_height = chips_on_narrow[0].height();
    let expected = (rows - 1) as f32 * (chip_height + space::S1);
    let band_bottom = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == Tokens::DARK.palette.hero_wash =>
                {
                    Some(shape.rect.bottom())
                }
                _ => None,
            })
            .expect("the artist band paints its wash")
    };
    let wide_block = text_rect(&wide_out, "An Album").expect("the first block");
    let narrow_block = text_rect(&narrow_out, "An Album").expect("the first block");
    // The stacked phone tier makes the band itself taller at the narrower
    // window, so only the move past the band's own foot belongs to the row.
    let moved =
        narrow_block.top() - wide_block.top() - (band_bottom(&narrow_out) - band_bottom(&wide_out));
    assert!(
        (moved - expected).abs() < 0.01,
        "the blocks move down by the wrapped rows: {moved} against {expected} for {rows} rows"
    );
}

/// The row reserves the cluster's whole height, not the chip's: the first
/// block sits below the band by the field's height and the step under it.
#[test]
fn the_row_reserves_its_height_above_the_first_block() {
    let ctx = context();
    let size = Vec2::new(900.0, 1500.0);
    let mut library = library_with(detail(
        Vec::new(),
        vec![release("spotify:album:r1", "A Book", &["spotify:track:1"])],
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

    let head = text_rect(&output, "Discography").expect("the section head");
    let chip = chip_rect(&output, Chip::All).expect("the All chip");
    let row_top = chip.center().y - crate::theme::cluster::FIELD_H / 2.0;
    let block_top = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == Tokens::DARK.palette.border
                    && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.top() > row_top + crate::theme::cluster::FIELD_H =>
            {
                Some(shape.rect.top())
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min);
    let above_block = block_top - row_top;
    let expected = crate::theme::cluster::FIELD_H + space::S4 + space::S2;

    assert!(
        (above_block - expected).abs() < 0.01,
        "the row takes its cluster's height and the step under it: {above_block} against {expected}"
    );
    assert!(
        row_top > head.bottom(),
        "the row still starts under the section head"
    );
}

#[test]
fn the_wrapped_row_never_overlaps_the_first_block() {
    let ctx = context();
    let sizes = chip_sizes(&ctx);
    let needed: f32 =
        sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
    let narrow = 2.0 * space::GUTTER + needed - 1.0;
    let mut library = library_with(detail(Vec::new(), every_kind()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec2::new(narrow, 1500.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    let first_block = text_rect(&output, "An Album").expect("the first block");
    for chip in [
        Chip::All,
        Chip::Kind(ReleaseKind::Album),
        Chip::Kind(ReleaseKind::Ep),
        Chip::Kind(ReleaseKind::Single),
        Chip::Kind(ReleaseKind::Compilation),
    ] {
        let rect = chip_rect(&output, chip).expect("every chip is drawn");
        assert!(
            rect.bottom() < first_block.top(),
            "{chip:?} ends at {}, the first block starts at {}",
            rect.bottom(),
            first_block.top()
        );
    }
}

/// One release of every kind, so the row has all five chips.
fn every_kind() -> Vec<Release> {
    vec![
        release_of_kind("spotify:a", "An Album", ReleaseKind::Album, &["t1"]),
        release_of_kind("spotify:b", "An EP", ReleaseKind::Ep, &["t2"]),
        release_of_kind("spotify:c", "A Single", ReleaseKind::Single, &["t3"]),
        release_of_kind(
            "spotify:d",
            "A Compilation",
            ReleaseKind::Compilation,
            &["t4"],
        ),
    ]
}

/// The sizes the five chips need, as the row measures them.
fn chip_sizes(ctx: &Context) -> Vec<Vec2> {
    let tokens = Tokens::DARK;
    let row = chips(&every_kind());
    let mut sizes = Vec::new();
    run(ctx, input(Vec2::new(900.0, 800.0), Vec::new()), |ui| {
        sizes = row
            .iter()
            .map(|chip| {
                crate::view::controls::chip_size(ui, &tokens, crate::format::filter_label(*chip))
            })
            .collect();
    });
    sizes
}
