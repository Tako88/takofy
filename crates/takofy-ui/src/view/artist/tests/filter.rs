use super::super::*; // the module under test
use super::*; // the shared fixtures, from tests/mod.rs

#[test]
fn a_page_with_no_band_leaves_the_bar_inset_alone() {
    let ctx = context();
    let size = Vec2::new(900.0, 600.0);
    // A genuinely empty discography draws no row and no band; a tall
    // likeness grid gives the page enough after it to scroll the bar's row
    // past the floor.
    let mut detail = detail(Vec::new(), Vec::new());
    detail.related = (0..12)
        .map(|index| format!("spotify:artist:other{index}"))
        .collect();
    let mut library = library_with(detail);
    let _ = library.related_request();
    library.apply(&Update::Related {
        artist: ARTIST.to_owned(),
        cards: (0..12)
            .map(|index| FollowedArtist {
                uri: format!("spotify:artist:other{index}"),
                name: format!("Other Artist {index}"),
                named: true,
                portrait: None,
                releases: None,
            })
            .collect(),
    });
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
    // To the foot of the page, where an empty discography's row would hold
    // the floor if it had one.
    for _ in 0..200 {
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
    let bar = bar_rect(&output).expect("the action row pins without a band under it");
    assert!(
        band_rect(&output).is_none(),
        "an empty discography draws no row, so nothing holds the floor"
    );
    assert_eq!(
        pin::inset(&ctx),
        crate::theme::actionbar::STUCK_H,
        "a page with no band leaves the seam to the bar"
    );
    let _ = bar;
}

/// The field's box in the page: flush with the row's far edge, on the chip
/// line the cluster shares.
fn field_rect(output: &egui::FullOutput, size: Vec2) -> Rect {
    let chip = chip_rect(output, Chip::All).expect("the All chip is drawn");
    Rect::from_min_size(
        pos2(
            size.x - space::GUTTER - crate::theme::cluster::FIELD_W,
            chip.center().y - crate::theme::cluster::FIELD_H / 2.0,
        ),
        vec2(
            crate::theme::cluster::FIELD_W,
            crate::theme::cluster::FIELD_H,
        ),
    )
}

/// Types `text` into the page's release field, returning the frame the
/// keystrokes landed on.
fn type_into_field(
    ctx: &Context,
    size: Vec2,
    text: &str,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let (_, output) = frame(ctx, size, Vec::new(), library, transport, queue, art);
    let field = field_rect(&output, size);
    for event in click_events(field.center()) {
        let _ = frame(ctx, size, vec![event], library, transport, queue, art);
    }
    // The field's own key filter is stored for the next pass, so the text
    // event belongs to a frame that already knows the field has the
    // keyboard.
    let _ = frame(ctx, size, Vec::new(), library, transport, queue, art);
    frame(
        ctx,
        size,
        vec![Event::Text(text.to_owned())],
        library,
        transport,
        queue,
        art,
    )
    .1
}

fn repaint_delay(output: &egui::FullOutput) -> std::time::Duration {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay)
}

#[test]
fn the_field_sits_flush_right_in_the_chip_row_240_wide() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let size = PAGE;
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let field = field_rect(&output, size);
    assert_eq!(field.width(), crate::theme::cluster::FIELD_W);
    assert_eq!(field.height(), crate::theme::cluster::FIELD_H);
    assert!(
        (field.right() - (size.x - space::GUTTER)).abs() < 0.01,
        "the field is flush with the row's far edge: {field:?}"
    );
    assert!(drawn(&output, "FILTER"), "{:?}", drawn_texts(&output));
}

#[test]
fn typing_narrows_by_name_on_top_of_the_chip() {
    let ctx = context();
    let size = PAGE;
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = type_into_field(
        &ctx,
        size,
        "album",
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(library.filter.needle().as_deref(), Some("album"));
    assert!(drawn(&output, "Album One"), "{:?}", drawn_texts(&output));
    assert!(drawn(&output, "Album Two"), "{:?}", drawn_texts(&output));
    assert!(
        !drawn(&output, "An EP"),
        "the name axis composes with the chip: {:?}",
        drawn_texts(&output)
    );

    library.filter.choose(Chip::Kind(ReleaseKind::Ep));
    let (_, after) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        !drawn(&after, "An EP"),
        "the album query still narrows the EP chip: {:?}",
        drawn_texts(&after)
    );
}

#[test]
fn the_count_reads_n_of_m_over_all_releases_and_only_while_typing() {
    let ctx = context();
    let size = PAGE;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let mut albums = library_with(detail(Vec::new(), mixed()));
    albums.filter.choose(Chip::Kind(ReleaseKind::Album));
    albums.filter.edit().push_str("album");
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut albums,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        drawn(&output, "2 OF 3 RELEASES"),
        "the count is shown over every release, not the chip's subset: {:?}",
        drawn_texts(&output)
    );

    let mut eps = library_with(detail(Vec::new(), mixed()));
    eps.filter.choose(Chip::Kind(ReleaseKind::Ep));
    eps.filter.edit().push_str("album");
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut eps,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        drawn(&output, "0 OF 3 RELEASES"),
        "an empty slice of the whole still counts the whole: {:?}",
        drawn_texts(&output)
    );

    let mut quiet = library_with(detail(Vec::new(), mixed()));
    let (_, output) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut quiet,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.contains(" OF 3 RELEASES")),
        "no query, no count: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn the_count_paints_inside_the_band_while_it_holds() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let mut library = with_fans(eight());
    library.filter.edit().push_str("album");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let mut output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    // Scroll on while the band keeps the floor: the count has to follow the
    // painted band, not the slot it left behind well above it.
    let bar = bar_rect(&output).expect("the action row is pinned");
    for _ in 0..8 {
        let next = wheel(
            &ctx,
            size,
            -space::S4,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let Some(band) = band_rect(&next) else {
            break;
        };
        if (band.top() - bar.bottom()).abs() > 0.5 {
            break;
        }
        output = next;
    }
    let band = band_rect(&output).expect("the chip row holds under the bar");
    let count = text_rect(&output, "3 OF 8 RELEASES").expect("the count is painted");
    assert!(
        band.contains_rect(count),
        "the count moves with the held band: {count:?} outside {band:?}"
    );
}

#[test]
fn the_count_paints_inside_the_band_while_it_releases() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let mut library = with_fans(eight());
    library.filter.edit().push_str("album");
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
    let band = band_rect(&output).expect("the chip row rides the end of the discography");
    let count = text_rect(&output, "3 OF 8 RELEASES").expect("the count is painted");
    assert!(
        band.contains_rect(count),
        "the count rides the released band: {count:?} outside {band:?}"
    );
}

#[test]
fn the_status_node_is_polite_and_exists_only_while_typing() {
    let ctx = context();
    ctx.enable_accesskit();
    let size = PAGE;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let tokens = Tokens::DARK;

    let mut library = library_with(detail(Vec::new(), mixed()));
    library.filter.edit().push_str("album");
    let mut read = None;
    let _ = run(&ctx, input(size, Vec::new()), |ui| {
        crate::scroll::instant(ui.ctx());
        ui.add_space(space::S3);
        let _ = artist_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            Instant::now(),
            &tokens,
        );
        read = ui
            .ctx()
            .accesskit_node_builder(egui::Id::unique("release-count"), |node| {
                (node.role(), node.live(), node.label().map(str::to_owned))
            });
    });
    let read = read.expect("the count has a status node");
    assert_eq!(read.0, egui::Role::Status);
    assert_eq!(read.1, Some(egui::accesskit::Live::Polite));
    assert_eq!(read.2.as_deref(), Some("2 of 3 releases"));

    let mut quiet = library_with(detail(Vec::new(), mixed()));
    let mut stray = false;
    let _ = run(&ctx, input(size, Vec::new()), |ui| {
        crate::scroll::instant(ui.ctx());
        ui.add_space(space::S3);
        let _ = artist_view(
            ui,
            Table::Full,
            &mut quiet,
            &mut transport,
            &mut queue,
            &mut art,
            Instant::now(),
            &tokens,
        );
        stray = ui
            .ctx()
            .has_accesskit_node(egui::Id::unique("release-count"));
    });
    assert!(!stray, "an unfiltered discography announces nothing");
}

#[test]
fn the_section_head_keeps_its_totals_while_typing() {
    let ctx = context();
    let size = PAGE;
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let output = type_into_field(
        &ctx,
        size,
        "album",
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let aside = discography_meta(3, 3).to_uppercase();
    assert!(
        drawn(&output, &aside),
        "the section head describes the artist, not the query: {aside}"
    );
}

#[test]
fn the_field_is_a_tab_stop_after_the_chips_and_before_the_blocks() {
    let ctx = context();
    let size = PAGE;
    let mut library = library_with(detail(Vec::new(), eight()));
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
    let field = field_rect(&output, size);
    for event in click_events(field.center()) {
        let _ = frame(
            &ctx,
            size,
            vec![event],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(ctx.text_edit_focused(), "the field took the keyboard");

    // Forward, the next stop is a row below the field: the field comes
    // before the blocks.
    let [press, release] = crate::view::test_support::tab();
    let _ = frame(
        &ctx,
        size,
        vec![press.clone()],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let _ = frame(
        &ctx,
        size,
        vec![release.clone()],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("Tab left the field for a control");
    let row = ctx
        .read_response(focused)
        .expect("the focused control is drawn")
        .rect;
    assert!(
        !ctx.text_edit_focused() && row.top() >= field.bottom() - 0.01,
        "the field comes before the blocks: {row:?} against {field:?}"
    );

    // Backwards, the next stop is a chip on the field's own line: the field
    // comes after the chips.
    for event in click_events(field.center()) {
        let _ = frame(
            &ctx,
            size,
            vec![event],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(ctx.text_edit_focused(), "the field took the keyboard");
    let back = |pressed| Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    };
    let _ = frame(
        &ctx,
        size,
        vec![back(true)],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let _ = frame(
        &ctx,
        size,
        vec![back(false)],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("Shift+Tab left the field for a control");
    let chip = ctx
        .read_response(focused)
        .expect("the focused control is drawn")
        .rect;
    assert!(
        chip.top() < field.bottom() && chip.bottom() > field.top(),
        "the field comes after the chips: {chip:?} against {field:?}"
    );
}

#[test]
fn the_field_keeps_its_focus_and_text_across_the_pin() {
    let ctx = context();
    let size = Vec2::new(900.0, 500.0);
    let mut library = library_with(detail(Vec::new(), eight()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let _ = type_into_field(
        &ctx,
        size,
        "album",
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(ctx.text_edit_focused(), "the field took the keyboard");
    let output = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        drawn(&output, "album"),
        "the field's text survives the pin: {:?}",
        drawn_texts(&output)
    );
    assert!(
        ctx.text_edit_focused(),
        "the field keeps the keyboard across the pin"
    );
}

#[test]
fn the_rule_follows_the_row() {
    let ctx = context();
    let size = Vec2::new(900.0, 500.0);
    let mut library = with_fans(eight());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let tokens = Tokens::DARK;

    let (_, resting) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let field = field_rect(&resting, size);
    let line = |output: &egui::FullOutput, field: Rect| {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (shape.rect.width() - field.width()).abs() < 0.01
                    && (shape.rect.bottom() - field.bottom()).abs() < 0.01 =>
            {
                shape.fill == tokens.palette.border || shape.fill == tokens.palette.text
            }
            _ => false,
        })
    };
    assert!(line(&resting, field), "a resting row underlines its field");

    let held = held_frame(
        &ctx,
        size,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let chip = chip_rect(&held, Chip::All).expect("the All chip is drawn at the floor");
    let held_field = Rect::from_min_size(
        pos2(
            size.x - space::GUTTER - crate::theme::cluster::FIELD_W,
            chip.center().y - crate::theme::cluster::FIELD_H / 2.0,
        ),
        vec2(
            crate::theme::cluster::FIELD_W,
            crate::theme::cluster::FIELD_H,
        ),
    );
    assert!(
        !line(&held, held_field),
        "a held row gives the field's rule to its own hairline"
    );

    let released = {
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
        let chip = chip_rect(&output, Chip::All).expect("the All chip rides the end");
        let field = Rect::from_min_size(
            pos2(
                size.x - space::GUTTER - crate::theme::cluster::FIELD_W,
                chip.center().y - crate::theme::cluster::FIELD_H / 2.0,
            ),
            vec2(
                crate::theme::cluster::FIELD_W,
                crate::theme::cluster::FIELD_H,
            ),
        );
        (output, field)
    };
    assert!(
        !line(&released.0, released.1),
        "a riding row also gives the field's rule to its own hairline"
    );
}

#[test]
fn the_field_does_not_move_when_the_count_arrives() {
    let ctx = context();
    let size = PAGE;
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
    let before = field_rect(&before, size);
    library.filter.edit().push_str("album");
    let (_, after) = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let after = field_rect(&after, size);
    assert_eq!(before, after, "the count arrives beside the field");
}

#[test]
fn a_steady_page_stops_asking_for_frames_after_an_edit() {
    let ctx = context();
    let size = PAGE;
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    // A quiet page: nothing has been typed, so nothing asks for a frame.
    let mut steady = std::time::Duration::ZERO;
    for _ in 0..8 {
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        steady = repaint_delay(&output);
    }
    assert_ne!(
        steady,
        std::time::Duration::ZERO,
        "a steady page must not look like an edited one"
    );

    // The keystroke narrows the blocks on the frame it lands: the post-edit
    // set is what is drawn. The end and the count were reserved from the
    // pre-edit state, so the page asks for one more frame to settle them.
    let typed = type_into_field(
        &ctx,
        size,
        "album",
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(drawn(&typed, "Album One"), "{:?}", drawn_texts(&typed));
    assert!(
        !drawn(&typed, "An EP"),
        "the blocks use the post-edit set: {:?}",
        drawn_texts(&typed)
    );

    // Escape is the clean test of the page's own request: the field handles
    // the key itself and gives the keyboard back, so nothing else in the row
    // has a reason to ask for the next frame.
    let _ = frame(
        &ctx,
        size,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let escape = Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let (_, cleared) = frame(
        &ctx,
        size,
        vec![escape],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(library.filter.needle(), None, "Escape cleared the query");
    assert!(
        drawn(&cleared, "An EP"),
        "the cleared list draws its hidden block again: {:?}",
        drawn_texts(&cleared)
    );
    // The page's own request for the settling frame is deliberately not
    // pinned here: Escape and the focus change request frames on their own,
    // whatever `request_repaint` did, so a ZERO delay on this frame would
    // prove neither.

    // The focus ring and the field's own state take a few frames to stop
    // moving; the page must not keep asking once they have.
    let mut quiet = false;
    for _ in 0..24 {
        let (_, settled) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        if repaint_delay(&settled) != std::time::Duration::ZERO {
            quiet = true;
            break;
        }
    }
    assert!(quiet, "the page never stopped asking for frames");
}

#[test]
fn typing_reaches_no_other_control() {
    let ctx = context();
    let size = PAGE;
    let mut library = library_with(detail(Vec::new(), mixed()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let page = library.page().cloned();

    let (command, output) = {
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let field = field_rect(
            &frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1,
            size,
        );
        for event in click_events(field.center()) {
            let _ = frame(
                &ctx,
                size,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        frame(
            &ctx,
            size,
            vec![Event::Text("album".to_owned())],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
    };
    assert_eq!(command, None, "typing asks for nothing");
    assert_eq!(library.page(), page.as_ref(), "typing navigates nowhere");
    assert_eq!(library.filter.chosen(), Chip::All);
    assert!(drawn(&output, "Album One"));
    assert!(!drawn(&output, "An EP"));
}

#[test]
fn the_page_paints_no_unaligned_marker_with_a_query() {
    let ctx = context();
    let mut library = library_with(detail(
        Vec::new(),
        vec![
            release("spotify:album:r1", "Slow Tide", &["spotify:track:1"]),
            release("spotify:album:r2", "Kestrel", &["spotify:track:1"]),
        ],
    ));
    library.filter.edit().push_str("slow");
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
    assert!(
        !drawn_texts(&output).contains(&"Unaligned"),
        "a queried page paints an unaligned widget: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn the_query_sentence_wins() {
    let ctx = context();
    let size = PAGE;
    let mut library = library_with(detail(Vec::new(), mixed()));
    library.filter.edit().push_str("zzz");
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
    assert!(
        texts.contains(&"Nothing in this list matches “zzz”"),
        "the query sentence: {texts:?}"
    );
    assert!(
        !texts.contains(&"Nothing in this format yet"),
        "only one of the two sentences is ever on screen: {texts:?}"
    );
    assert!(!texts.contains(&"Album One"), "{texts:?}");
    assert!(!texts.contains(&"An EP"), "{texts:?}");
}

#[test]
fn the_format_empty_state_names_the_artist() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let output = run(&ctx, input(PAGE, Vec::new()), |ui| {
        ui.add_space(space::S3);
        draw_empty(
            ui,
            crate::discography::Emptied::Format,
            "Vela Moreno",
            "",
            &tokens,
        );
    });
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"Nothing in this format yet"),
        "the format's heading: {texts:?}"
    );
    assert!(
        texts.contains(&"Vela Moreno has no releases of that type."),
        "the format's sentence names the artist: {texts:?}"
    );
}

#[test]
fn the_band_still_releases_over_an_empty_body() {
    let ctx = context();
    let size = Vec2::new(900.0, 420.0);
    let mut library = with_fans(eight());
    library.filter.edit().push_str("zzz");
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
    assert!(
        band_rect(&output).is_some(),
        "the row still rides the empty body"
    );
    assert!(
        drawn(&output, "Nothing in this list matches “zzz”"),
        "the sentence is drawn after the body clip is restored: {:?}",
        drawn_texts(&output)
    );
}
