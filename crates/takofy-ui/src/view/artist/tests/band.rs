use super::super::*; // the module under test
use super::*; // the shared fixtures, from tests/mod.rs
use crate::theme::hero;

// -----------------------------------------------------------------------
// The chip row as a band: it holds under the bar, paints its own surface,
// lets go at the end of the discography and joins the reveal seam.
// -----------------------------------------------------------------------

/// Answers every release's label request, walking the page so each block is
/// drawn and asked for, then walks back up to where the band still rests.
///
/// The store only accepts a reply for a release it has asked about, so a
/// label cannot simply be handed over for every release up front.
fn label_every_release(
    ctx: &Context,
    size: Vec2,
    releases: &[Release],
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) {
    let mut labelled = 0;
    let mut output = frame(ctx, size, Vec::new(), library, transport, queue, art).1;
    for _ in 0..400 {
        for command in library.releases.requests() {
            let Command::LoadReleaseTracks { release, tracks } = command else {
                continue;
            };
            labelled += 1;
            let tracks = tracks
                .iter()
                .map(|uri| track(uri, uri, "Vela Moreno", 200_000))
                .collect();
            library.apply(&Update::ReleaseTracks { release, tracks });
        }
        if labelled >= releases.len() {
            break;
        }
        output = wheel(ctx, size, -extent::ROW_H, library, transport, queue, art);
    }
    assert_eq!(
        labelled,
        releases.len(),
        "not every release was drawn and labelled"
    );
    // Back to where the band is in its slot, so the caller can scroll down
    // into the band it is about.
    for _ in 0..400 {
        if band_rect(&output).is_none() {
            return;
        }
        output = wheel(ctx, size, extent::ROW_H, library, transport, queue, art);
    }
    panic!("the page never scrolled back above the band");
}

/// The rect a shape paints into.
fn shape_rect(shape: &egui::epaint::Shape) -> Option<Rect> {
    match shape {
        egui::epaint::Shape::Rect(rect) => Some(rect.rect),
        egui::epaint::Shape::Text(text) => Some(Rect::from_min_size(text.pos, text.galley.size())),
        _ => None,
    }
}

/// The shape painting `label`, with the clip it was drawn through.
fn clipped_text(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(clipped.clip_rect)
            }
            _ => None,
        })
}

/// The band's hairline: the edge it wears while it is out of its slot.
fn band_edge(output: &egui::FullOutput, band: Rect) -> Option<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == tokens.palette.border
                    && (shape.rect.top() - (band.bottom() - stroke::HAIRLINE)).abs() < 0.01
                    && (shape.rect.width() - band.width()).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// The name of a track row of the fixture, if `label` is one.
fn row_title(label: &str) -> bool {
    label.strip_prefix('T').is_some_and(|tail| {
        !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '.')
    })
}

#[test]
fn the_wheel_pins_the_artist_bar() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(Vec::new(), eight()));
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
    assert!(
        bar_rect(&output).is_none(),
        "nothing is pinned at the top of the page"
    );
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
        "the wheel scrolls the artist page and its bar pins"
    );
}

#[test]
fn the_artist_bar_pins_at_the_top_with_the_name_beside_play() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(Vec::new(), eight()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let bar = bar_rect(&output).expect("the action row is pinned");
    assert!(
        (bar.top() - space::S3).abs() < 0.01,
        "the row pins at the viewport's top: {}",
        bar.top()
    );
    let name = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Vela Moreno" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .find(|rect| bar.contains_rect(*rect))
        .expect("the pinned row names the artist");
    assert!(
        name.left() > bar.left() + hero::PLAY,
        "the name stands beside the play tile: {name:?} in {bar:?}"
    );
}

#[test]
fn the_chip_row_pins_at_sixty_under_the_bar() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(Vec::new(), eight()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let bar = bar_rect(&output).expect("the action row is pinned");
    let band = band_rect(&output).expect("the chip row is filled while it holds");

    assert!(
        (band.top() - bar.bottom()).abs() < 0.01,
        "the band starts where the bar ends: {} against {}",
        band.top(),
        bar.bottom()
    );
    assert!(
        (band.width() - size.x).abs() < 0.01 && band.left().abs() < 0.01,
        "the surface bleeds to both pane edges: {band:?}"
    );
    let all = chip_rect(&output, Chip::All).expect("the All chip is drawn at the floor");
    assert!(
        all.left() >= space::GUTTER - 0.01 && all.top() >= band.top() - 0.01,
        "the chips sit inside the surface, not off its edge: {all:?} in {band:?}"
    );
    assert!(
        band_edge(&output, band).is_some(),
        "the held band wears an edge at its bottom"
    );

    // The surface is painted before the chips, because the rows sliding under
    // the band are painted after it: anything left clear would show their ink.
    let (surface, surface_clip) = output
        .shapes
        .iter()
        .enumerate()
        .find_map(|(index, clipped)| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == Tokens::DARK.palette.surface
                    && (shape.rect.height() - band.height()).abs() < 0.01 =>
            {
                Some((index, clipped.clip_rect))
            }
            _ => None,
        })
        .expect("the band's surface is in the frame");
    assert!(
        surface_clip.width() >= band.width() - 0.01,
        "the surface is painted through the gutter's clip, not the pane's: {surface_clip:?}"
    );
    let first_chip = output
        .shapes
        .iter()
        .position(|clipped| {
            matches!(&clipped.shape, egui::epaint::Shape::Text(text) if text.galley.text() == "ALL")
        })
        .expect("the All chip's label is in the frame");
    assert!(
        surface < first_chip,
        "the surface is painted before the chips: {surface} against {first_chip}"
    );
}

#[test]
fn the_row_rests_in_its_slot_before_it_reaches_the_bar() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(Vec::new(), eight()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    assert!(
        band_rect(&output).is_none(),
        "a row in its slot paints no surface of its own"
    );
    let head = text_rect(&output, "Discography").expect("the section head");
    let chip = chip_rect(&output, Chip::All).expect("the All chip");
    let first = text_rect(&output, "Album One").expect("the first block");
    assert!(
        head.bottom() < chip.top() && chip.bottom() < first.top(),
        "the row keeps the slot between the head and the blocks: {chip:?}"
    );
    assert_eq!(
        pin::inset(&ctx),
        0.0,
        "a row in its slot speaks for nothing"
    );
}

#[test]
fn rows_never_paint_under_the_held_band() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(Vec::new(), eight()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let band = band_rect(&output).expect("the chip row is held");

    let mut drawn = 0;
    for clipped in &output.shapes {
        let egui::epaint::Shape::Text(text) = &clipped.shape else {
            continue;
        };
        let label = text.galley.text();
        if !eight().iter().any(|release| release.name == label) {
            continue;
        }
        drawn += 1;
        assert!(
            clipped.clip_rect.top() >= band.bottom() - 0.01,
            "{label} is drawn through a clip that starts above the band: {:?}",
            clipped.clip_rect
        );
    }
    assert!(drawn > 0, "a block is drawn under the held band");
}

#[test]
fn a_press_on_a_chip_while_held_chooses_it_and_reaches_no_row() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let releases = eight();
    let mut library = library_with(detail(Vec::new(), releases.clone()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    label_every_release(
        &ctx,
        size,
        &releases,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let albums =
        chip_rect(&output, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip at the floor");

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
    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Album),
        "the press on the held band chooses the chip"
    );
    for release in &releases {
        assert!(
            !release_picked(&ctx, &release.uri, 0),
            "the press fell through the band onto a row"
        );
    }
}

#[test]
fn the_band_releases_at_the_end_of_the_discography() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let mut library = with_fans(eight());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = released_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let band = band_rect(&output).expect("the chip row rides the end");
    let fans = text_rect(&output, "Fans also like").expect("the likeness head is drawn");
    assert!(
        (band.bottom() - (fans.top() - space::S2 - space::S6)).abs() < 0.01,
        "the band ends where the discography ends: {} against {}",
        band.bottom(),
        fans.top() - space::S2 - space::S6
    );
}

#[test]
fn rows_above_the_released_band_stay_visible_and_fans_is_not_clipped() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let releases = eight();
    let mut library = with_fans(releases.clone());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    label_every_release(
        &ctx,
        size,
        &releases,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let output = released_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let bar = bar_rect(&output).expect("the action row is pinned");
    let band = band_rect(&output).expect("the chip row rides the end");
    assert!(
        band.top() < bar.bottom() - 0.5,
        "the band is riding the end, not holding the floor"
    );

    // No row of the discography paints inside the strip the riding band
    // covers: the body is drawn through a clip that stops above it.
    let fixture = eight();
    let names: Vec<&str> = fixture
        .iter()
        .map(|release| release.name.as_str())
        .collect();
    for clipped in &output.shapes {
        let Some(rect) = shape_rect(&clipped.shape) else {
            continue;
        };
        let egui::epaint::Shape::Text(text) = &clipped.shape else {
            continue;
        };
        let label = text.galley.text();
        if !names.contains(&label) && !row_title(label) {
            continue;
        }
        assert!(
            rect.bottom() <= band.top() + 0.01 || rect.top() >= band.bottom() - 0.01,
            "{label} paints inside the riding band: {rect:?} against {band:?}"
        );
    }

    // The likeness row below it is drawn whole: the body's clip went back
    // before the section that is not part of the discography.
    let fans = text_rect(&output, "Fans also like").expect("the likeness head is drawn");
    let clip = clipped_text(&output, "Fans also like").expect("the likeness head has a clip");
    assert!(
        clip.contains_rect(fans),
        "the likeness head is clipped away: {fans:?} through {clip:?}"
    );
    let card = text_rect(&output, "Other Artist").expect("the card is drawn");
    let clip = clipped_text(&output, "Other Artist").expect("the card has a clip");
    assert!(
        clip.contains_rect(card),
        "the likeness card is clipped away: {card:?} through {clip:?}"
    );
}

#[test]
fn the_hairline_stays_while_the_band_rides_up() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let mut library = with_fans(eight());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = released_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let band = band_rect(&output).expect("the chip row rides the end");
    assert!(
        band_edge(&output, band).is_some(),
        "a band out of its slot keeps the edge that says rows pass under it"
    );
}

#[test]
fn the_inset_is_the_lowest_bottom() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let mut library = library_with(detail(Vec::new(), eight()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let bar = bar_rect(&output).expect("the action row is pinned");
    let band = band_rect(&output).expect("the chip row is held");
    assert!(
        (pin::inset(&ctx) - (band.bottom() - bar.top())).abs() < 0.01,
        "the seam holds the lowest band's bottom: {} against {}",
        pin::inset(&ctx),
        band.bottom() - bar.top()
    );
}

#[test]
fn a_focused_release_row_comes_out_from_under_the_held_band() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    let releases = eight();
    let mut library = library_with(detail(Vec::new(), releases.clone()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    label_every_release(
        &ctx,
        size,
        &releases,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let band = band_rect(&output).expect("the chip row is held");

    // The first row drawn wholly below the band, so the press lands on the row
    // and not in the strip the band owns.
    let (label, title) = drawn_texts(&output)
        .into_iter()
        .filter(|text| row_title(text))
        .filter_map(|text| text_rect(&output, text).map(|rect| (text.to_owned(), rect)))
        .find(|(_, rect)| rect.top() >= band.bottom() + 1.0)
        .expect("a row is drawn below the held band");
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

    // Walk the row up until its top is inside the strip the band covers, then
    // let what the reveal asked for settle.
    let mut covered = false;
    for _ in 0..120 {
        let row = ctx.read_response(focused).expect("the row is drawn").rect;
        if row.top() < band.bottom() - 0.5 {
            covered = true;
            break;
        }
        let _ = wheel(
            &ctx,
            size,
            -space::S2,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    assert!(covered, "the row never slid under the held band");

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
            && row.rect.top() >= band.bottom() - 0.01
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
        "the focused row never came out from under the held band: {label}"
    );
    let band = band_rect(&output).expect("the chip row is still held");
    let row = ctx
        .read_response(focused)
        .expect("the row is drawn again")
        .rect;
    assert!(
        row.top() >= band.bottom() - 0.01,
        "the focused row rests below the band: {row:?} against {band:?}"
    );
}

#[test]
fn a_focused_row_in_the_last_block_asks_for_no_scroll_while_the_band_is_released() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let releases = eight();
    let mut library = with_fans(releases.clone());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    label_every_release(
        &ctx,
        size,
        &releases,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    // The last row of the last block, drawn below the band while it still
    // holds the floor: the row a reader would reach for at the end.
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
    let mut last = None;
    for _ in 0..800 {
        let Some(band) = band_rect(&output) else {
            output = wheel(
                &ctx,
                size,
                -space::S2,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            continue;
        };
        if let Some(rect) = text_rect(&output, "T7.2")
            && rect.top() >= band.bottom() + 1.0
            && rect.bottom() <= size.y - 1.0
        {
            last = Some(rect);
            break;
        }
        output = wheel(
            &ctx,
            size,
            -space::S2,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    let title = last.expect("the last row is drawn below the held band");
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
        .expect("the last row took the keyboard");

    // Scroll on until the discography's end carries the band up: the row is
    // behind the bar now, and a band measured from the viewport's top would
    // drag the page upward every frame.
    let output = released_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let bar = bar_rect(&output).expect("the action row is pinned");
    let band = band_rect(&output).expect("the chip row rides the end");
    assert!(
        band.top() < bar.bottom() - 0.5,
        "the band is riding the end, not holding the floor"
    );
    assert_eq!(
        pin::inset(&ctx),
        crate::theme::actionbar::STUCK_H,
        "a released band leaves the seam to the bar"
    );

    // The page does not move on its own: the row it still holds the keyboard
    // for asks for nothing. Let whatever the last wheel left pending settle
    // first, so the comparison is of a quiet page.
    let mut output = output;
    for _ in 0..6 {
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
    let fans = text_rect(&output, "Fans also like").expect("the likeness head is drawn");
    let mut still = output;
    for _ in 0..8 {
        still = frame(
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
    let moved = text_rect(&still, "Fans also like").expect("the likeness head is still drawn");
    assert!(
        (moved.top() - fans.top()).abs() < 0.01,
        "the page moved while a row was focused under a released band: {} against {}",
        moved.top(),
        fans.top()
    );
    assert!(
        ctx.read_response(focused)
            .is_none_or(|row| row.rect.top() >= 0.0),
        "the focused row was dragged above the viewport"
    );
}

#[test]
fn a_three_line_band_at_a_narrow_pane_covers_the_last_row() {
    let ctx = context();
    // Narrow enough that the five chips need three lines: the room a 240-wide
    // cluster leaves beside them on a rail-width window, where the band's own
    // height is the trailing step of the last block and then some.
    let size = Vec2::new(200.0, 380.0);
    let mut library = with_fans(eight());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = released_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let band = band_rect(&output).expect("the chip row rides the end");

    let mut tops: Vec<f32> = [
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
    tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    assert!(
        tops.len() >= 3,
        "the band wraps to three lines at {size:?}: {tops:?}"
    );

    // The last row of the last block sits wholly inside the band's rect: a
    // wrapped band is taller than the step it rides, so it covers the row
    // that would otherwise be the last thing on the page.
    let end = band.bottom();
    let row_top = end - space::S6 - extent::ROW_H;
    let row_bottom = end - space::S6;
    assert!(
        band.top() <= row_top && row_bottom <= band.bottom(),
        "the wrapped band covers the last row: {band:?} against {row_top}..{row_bottom}"
    );
}
