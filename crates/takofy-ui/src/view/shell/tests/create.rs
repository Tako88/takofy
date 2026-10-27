use super::super::*;
use super::test_support::*;

/// The rail filter's box, read from the hairline under it inside `area`.
fn rail_filter_rect(output: &egui::FullOutput, area: Rect) -> Rect {
    let tokens = Tokens::DARK;
    // The lowest match: the drawer is drawn over the topbar, and the
    // topbar's search field can fall inside the drawer's own box.
    let (hairline, _) = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.width() > 80.0
                    && shape.rect.width() < 200.0
                    && area.contains_rect(shape.rect)
                    && (shape.fill == tokens.palette.border
                        || shape.fill == tokens.palette.text) =>
            {
                Some((shape.rect, shape.fill))
            }
            _ => None,
        })
        .max_by(|left, right| left.0.bottom().total_cmp(&right.0.bottom()))
        .expect("the rail filter paints its hairline");
    Rect::from_min_max(
        egui::pos2(hairline.left(), hairline.bottom() - extent::BUTTON),
        egui::pos2(hairline.right(), hairline.bottom()),
    )
}

/// The rect the widget holding the keyboard was laid out in.
fn focused_stop(window: &Window) -> Option<Rect> {
    let focused = window.ctx.memory(|memory| memory.focused())?;
    window.ctx.read_response(focused).map(|drawn| drawn.rect)
}

#[test]
fn plus_opens_the_field_and_the_keyboard_follows_next_frame() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    let _ = window.click(&mut transport, &mut queue, sidebar_plus(&window));
    assert!(window.library.naming.is_open());
    assert!(
        !window.ctx.text_edit_focused(),
        "the field took the keyboard on the frame it opened"
    );

    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(window.ctx.text_edit_focused());
}

#[test]
fn enter_submits_the_trimmed_name_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    assert_eq!(
        submit_name(&mut window, &mut transport, &mut queue, "  Night  Drive "),
        Some(Command::CreatePlaylist {
            name: "Night  Drive".to_owned(),
            revision: revision("root-1"),
            tracks: Vec::new(),
        })
    );
    assert!(window.library.write_in_flight());
    assert!(window.library.naming.writing());

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, false)],
    );
    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Enter, true)]
            )
            .0,
        None,
        "a second Enter sent a second create"
    );
}

#[test]
fn a_held_enter_submits_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    assert!(submit_name(&mut window, &mut transport, &mut queue, "Mix").is_some());
    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key_repeat(egui::Key::Enter)]
            )
            .0,
        None,
        "a held Enter replayed the submit"
    );
}

#[test]
fn enter_on_blank_text_sends_nothing_and_keeps_the_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    assert_eq!(
        submit_name(&mut window, &mut transport, &mut queue, "   "),
        None
    );
    assert!(window.library.naming.is_open(), "a blank submit closed it");
    assert!(
        window.ctx.text_edit_focused(),
        "a blank submit dropped the keyboard"
    );
    assert!(!window.library.write_in_flight());
    assert_eq!(window.library.sidebar_notice, None, "a blank submit spoke");
}

#[test]
fn typing_beyond_the_cap_is_truncated() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);
    open_naming(&mut window, &mut transport, &mut queue);

    type_text(&mut window, &mut transport, &mut queue, &"a".repeat(140));

    assert_eq!(
        window.library.naming.text().chars().count(),
        takofy_model::PLAYLIST_NAME_MAX,
        "the field took more than a name"
    );
}

#[test]
fn blur_with_blank_text_cancels_and_blur_with_text_stays() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    open_naming(&mut window, &mut transport, &mut queue);
    let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
    assert!(
        !window.library.naming.is_open(),
        "a blank field stayed open after a click elsewhere"
    );

    open_naming(&mut window, &mut transport, &mut queue);
    type_text(&mut window, &mut transport, &mut queue, "Night Drive");
    let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
    assert!(
        window.library.naming.is_open(),
        "a click elsewhere threw a typed name away"
    );
    assert_eq!(window.library.naming.text(), "Night Drive");
    assert!(
        !window.ctx.text_edit_focused(),
        "the field kept the keyboard"
    );
}

#[test]
fn the_close_button_cancels() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);
    open_naming(&mut window, &mut transport, &mut queue);
    type_text(&mut window, &mut transport, &mut queue, "Night Drive");

    let _ = window.click(&mut transport, &mut queue, sidebar_plus(&window));

    assert!(!window.library.naming.is_open(), "the × did not close it");
    assert_eq!(window.library.naming.text(), "");
    assert!(!window.library.write_in_flight());
}

#[test]
fn while_writing_the_field_is_read_only_and_cancel_is_inert() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    assert!(submit_name(&mut window, &mut transport, &mut queue, "Night Drive").is_some());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !window.ctx.text_edit_focused(),
        "a field mid-write still took the keyboard"
    );

    let _ = window.click(&mut transport, &mut queue, sidebar_plus(&window));
    assert!(
        window.library.naming.is_open(),
        "a cancel ended a create in flight"
    );
    assert!(window.library.naming.writing());

    type_text(&mut window, &mut transport, &mut queue, "x");
    assert_eq!(
        window.library.naming.text(),
        "Night Drive",
        "a read-only field took a keystroke"
    );
}

#[test]
fn a_make_failure_refocuses_the_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    assert!(submit_name(&mut window, &mut transport, &mut queue, "Night Drive").is_some());
    window.library.apply(&Update::PlaylistCreateFailed);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(window.library.naming.is_open());
    assert_eq!(window.library.naming.text(), "Night Drive");
    assert!(
        window.ctx.text_edit_focused(),
        "the retry had no keyboard in the field"
    );
    assert_eq!(
        window.library.sidebar_notice,
        Some(crate::library::SidebarWrite::Create)
    );
}

#[test]
fn typing_in_the_rail_field_reaches_neither_the_transport_nor_a_cut() {
    let mut transport = playing(cover("a"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    sidebar_playlists(
        &mut window,
        &[("a", Some(0)), ("b", Some(1))],
        Some(rootlist_access(true)),
    );
    liked(&mut window, &[]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let field = rail_filter_rect(&frame, sidebar_rect(&window));
    let _ = window.click(&mut transport, &mut queue, field.center());
    // The field's own key filter is stored a frame after the focus lands.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the click did not focus the rail's field"
    );
    type_text(&mut window, &mut transport, &mut queue, "a");
    assert_eq!(
        window.library.rail_filter.query(),
        "a",
        "the typed character did not reach the rail's query"
    );

    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Space, true)]
            )
            .0,
        None,
        "a space reached the transport through the rail's field"
    );
    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Delete, true)]
            )
            .0,
        None,
        "Del cut a row while the rail's field had the keyboard"
    );
    crate::view::heart::post(
        &window.ctx,
        crate::view::heart::Ask {
            tracks: vec!["spotify:track:a".to_owned()],
            liked: true,
        },
    );
    assert_eq!(
        window.frame(&mut transport, &mut queue, Vec::new()).0,
        None,
        "a heart wrote while the rail's field had the keyboard"
    );
    assert_eq!(window.library.likes.state("spotify:track:a"), Some(false));
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_drop_on_a_narrowed_rail_row_adds_to_that_playlist() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[
            ("Alpha", Some(0)),
            ("Bravo", Some(1)),
            ("Midnight City", Some(2)),
            ("Midnight Sun", Some(3)),
        ],
        Some(rootlist_access(true)),
    );
    window.library.rail_filter.edit().push_str("midnight");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    // The first drawn row stands for the third playlist.
    let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist · 2 tracks");
    let (_, held) = window.hold_drag(&mut transport, &mut queue, from, row.center());
    assert!(marker(&held).is_none(), "an add draws no rule");

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![window.release(row.center())],
    );

    assert_eq!(
        command,
        Some(Command::AddTracks {
            playlist: "playlist:Midnight City".to_owned(),
            revision: revision("root-1"),
            from_index: 2,
            tracks: vec![TRACK_A.to_owned()],
        }),
        "the drop names the playlist the drawn row stands for"
    );
}

#[test]
fn a_drop_between_narrowed_rows_reorders_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[
            ("Alpha", Some(0)),
            ("Midnight City", Some(1)),
            ("Midnight Sun", Some(2)),
        ],
        Some(rootlist_access(true)),
    );

    // The rail row lifts while the column is whole, which is the only way a
    // reorder can be started at all. Row 0 is not next to the gap the drop
    // below lands on, so nothing but the narrowing refuses the move.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let lifted = sidebar_row_rect(&window, &frame, "Alpha", "Playlist · 1 track");
    let (_, _) = window.hold_drag(
        &mut transport,
        &mut queue,
        lifted.center(),
        lifted.center() + Vec2::new(0.0, 20.0),
    );
    let carry = carry::carried(&window.ctx).expect("the rail row lifts");
    assert_eq!(carry.source, Source::Playlist);
    assert_eq!(carry.rows, vec![0]);

    // The query narrows the list under the carry: the two drawn rows stand
    // for playlists 1 and 2, so the gap below them is a boundary of a list
    // the user is no longer looking at.
    window.library.rail_filter.edit().push_str("midnight");
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let boundary = sidebar_boundary(
        &window,
        &frame,
        &["Midnight City", "Midnight Sun"],
        "Playlist · 1 track",
        2,
    );

    let (_, held) = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(boundary)],
    );
    let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(boundary)]);

    assert!(
        marker(&held).is_none(),
        "a narrowed column draws no reorder rule"
    );
    assert_eq!(
        command, None,
        "a narrowed column moved the playlist its slot happened to name"
    );
}

// ---- the rail's field and the page's field, both on screen ----

#[test]
fn the_rail_query_survives_a_page_change_while_the_page_filter_clears() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, page) = both_fields(&mut window, &mut transport, &mut queue);

    focus_field(&mut window, &mut transport, &mut queue, rail);
    type_text(&mut window, &mut transport, &mut queue, "mid");
    focus_field(&mut window, &mut transport, &mut queue, page);
    type_text(&mut window, &mut transport, &mut queue, "keep");
    assert_eq!(window.library.rail_filter.query(), "mid");
    assert_eq!(window.library.track_filter.edit(PLAYLIST).as_str(), "keep");

    // A page change is the page's reset and only the page's: the rail keeps
    // what was typed in it.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist");
    let command = window.click(&mut transport, &mut queue, row.center());

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "playlist:Midnight City".to_owned(),
        })
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist("playlist:Midnight City".to_owned()))
    );
    assert_eq!(
        window.library.track_filter.edit(PLAYLIST).as_str(),
        "",
        "the page change left the page's query behind"
    );
    assert_eq!(
        window.library.rail_filter.query(),
        "mid",
        "the page change cleared the rail's query too"
    );
}

#[test]
fn escape_in_the_rail_field_clears_the_rail_and_leaves_the_page_query() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, _page) = both_fields(&mut window, &mut transport, &mut queue);

    focus_field(&mut window, &mut transport, &mut queue, rail);
    type_text(&mut window, &mut transport, &mut queue, "mid");
    window.library.track_filter.edit(PLAYLIST).push_str("keep");

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(window.library.rail_filter.query(), "");
    assert_eq!(
        window.library.track_filter.edit(PLAYLIST).as_str(),
        "keep",
        "an Escape in the rail reached the page's field"
    );
    assert!(
        !window.ctx.text_edit_focused(),
        "Escape gave the keyboard back"
    );
}

#[test]
fn escape_in_the_page_field_clears_the_page_and_leaves_the_rail_query() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_rail, page) = both_fields(&mut window, &mut transport, &mut queue);

    focus_field(&mut window, &mut transport, &mut queue, page);
    type_text(&mut window, &mut transport, &mut queue, "keep");
    window.library.rail_filter.edit().push_str("mid");

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(window.library.track_filter.edit(PLAYLIST).as_str(), "");
    assert_eq!(
        window.library.rail_filter.query(),
        "mid",
        "an Escape in the page reached the rail's field"
    );
    assert!(
        !window.ctx.text_edit_focused(),
        "Escape gave the keyboard back"
    );
}

#[test]
fn escape_with_focus_on_a_row_clears_neither() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_rail, _page) = both_fields(&mut window, &mut transport, &mut queue);
    window.library.rail_filter.edit().push_str("mid");
    window.library.track_filter.edit(PLAYLIST).push_str("keep");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, view_rect(&window), "keep0");
    let _ = window.click(&mut transport, &mut queue, at);
    assert!(focused_stop(&window).is_some(), "the row took the keyboard");

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(
        window.library.rail_filter.query(),
        "mid",
        "an Escape on a row cleared the rail's field"
    );
    assert_eq!(
        window.library.track_filter.edit(PLAYLIST).as_str(),
        "keep",
        "an Escape on a row cleared the page's field"
    );
}

#[test]
fn typing_in_one_field_does_not_move_the_other_fields_text() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, page) = both_fields(&mut window, &mut transport, &mut queue);

    focus_field(&mut window, &mut transport, &mut queue, rail);
    type_text(&mut window, &mut transport, &mut queue, "mid");
    focus_field(&mut window, &mut transport, &mut queue, page);
    type_text(&mut window, &mut transport, &mut queue, "keep");

    assert_eq!(window.library.rail_filter.query(), "mid");
    assert_eq!(window.library.track_filter.edit(PLAYLIST).as_str(), "keep");
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "mid", sidebar_rect(&window)).is_some(),
        "the rail's field still draws its own text"
    );
    assert!(
        text_in(&frame, "keep", view_rect(&window)).is_some(),
        "the page's field still draws its own text"
    );
}

#[test]
fn the_rail_field_is_a_tab_stop_before_the_rows_and_before_the_page_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, page) = both_fields(&mut window, &mut transport, &mut queue);
    let stops = tab_stops_until_page_field(&mut window, &mut transport, &mut queue, rail, page);

    let at = |kind: &str| {
        stops
            .iter()
            .position(|stop| *stop == kind)
            .unwrap_or_else(|| panic!("Tab never reached {kind}: {stops:?}"))
    };
    let chips = at("rail-chips");
    let rail_at = at("rail-field");
    let head = at("rail-head");
    let row = at("rail-row");
    let top = at("topbar");
    let page_at = at("page-field");

    assert_eq!(
        stops.first(),
        Some(&"rail-chips"),
        "Tab starts in the rail's chips: {stops:?}"
    );
    assert!(
        chips < rail_at,
        "the chips must come before the rail's field: {stops:?}"
    );
    assert!(
        rail_at < head,
        "the rail's field must come before the create button: {stops:?}"
    );
    assert!(
        head < row,
        "the create button must come before the rail's rows: {stops:?}"
    );
    assert!(
        row < top,
        "the rail's rows must come before the topbar: {stops:?}"
    );
    assert!(
        top < page_at,
        "the topbar must come before the page's field: {stops:?}"
    );
}

#[test]
fn without_a_live_create_button_the_rail_field_still_precedes_the_rows_and_the_page_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, page) = both_fields_without_create(&mut window, &mut transport, &mut queue);
    assert!(
        !window.library.creatable(),
        "the fixture can create a playlist, so its create button is a Tab stop"
    );

    let stops = tab_stops_until_page_field(&mut window, &mut transport, &mut queue, rail, page);

    let at = |kind: &str| {
        stops
            .iter()
            .position(|stop| *stop == kind)
            .unwrap_or_else(|| panic!("Tab never reached {kind}: {stops:?}"))
    };
    let chips = at("rail-chips");
    let rail_at = at("rail-field");
    let row = at("rail-row");
    let top = at("topbar");
    let page_at = at("page-field");

    // The inert `+` is no stop at all: with it gone the chain the creatable
    // fixture checks must still hold, and nothing may stand in the head.
    assert!(
        !stops.contains(&"rail-head"),
        "an inert create button is still a Tab stop: {stops:?}"
    );
    assert!(
        chips < rail_at,
        "the chips must come before the rail's field: {stops:?}"
    );
    assert!(
        rail_at < row,
        "the rail's field must come before the rail's rows: {stops:?}"
    );
    assert!(
        row < top,
        "the rail's rows must come before the topbar: {stops:?}"
    );
    assert!(
        top < page_at,
        "the topbar must come before the page's field: {stops:?}"
    );
}

#[test]
fn shift_tab_from_the_first_rail_row_returns_to_the_create_button() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, _page) = both_fields(&mut window, &mut transport, &mut queue);
    let rail_area = sidebar_rect(&window);
    let create = head_create(rail_area);

    let mut row = None;
    for _ in 0..48 {
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
        if let Some(stop) = focused_stop(&window)
            && rail_area.contains_rect(stop)
            && stop.center().y > rail.bottom()
            && (stop.height() - sidebar::ROW_H).abs() < 0.6
        {
            row = Some(stop);
            break;
        }
    }
    assert!(row.is_some(), "Tab reached no rail row");

    let _ = chord(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::Tab,
        egui::Modifiers::SHIFT,
    );

    let stop = focused_stop(&window).expect("Shift+Tab put the keyboard somewhere");
    assert!(
        (stop.min - create.min).length() < 0.5 && (stop.max - create.max).length() < 0.5,
        "Shift+Tab from the first rail row landed on {stop:?}, not the create button at \
         {create:?}"
    );
}

#[test]
fn the_rail_offers_its_surviving_rows_as_add_targets_while_a_page_row_is_carried() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[
            ("Alpha", Some(0)),
            ("Bravo", Some(1)),
            ("Midnight City", Some(2)),
            ("Midnight Sun", Some(3)),
        ],
        Some(rootlist_access(true)),
    );
    window.library.rail_filter.edit().push_str("midnight");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist · 2 tracks");
    let _ = window.hold_drag(&mut transport, &mut queue, from, row.center());
    let carry = carry::carried(&window.ctx).expect("the page row is carried");
    assert!(
        matches!(carry.source, Source::List(_)),
        "the carried block is the page's row"
    );

    // The shell's own pass read and spent the frame's offer, so the rail is
    // drawn once more over the same carried block to read what it offers.
    let _ = column_frame(
        &window.ctx,
        &mut window.library,
        &mut window.art,
        Vec::new(),
        true,
    );
    let offer = carry::take_column_offer(&window.ctx).expect("the rail offers itself");

    assert_eq!(offer.zone.count, 2, "only the survivors are offered");
    assert_eq!(
        offer.rows,
        Some(vec![2, 3]),
        "the offer names the playlists the drawn slots stand for"
    );
    assert_eq!(offer.source_row(0), Some(2));
    assert_eq!(offer.source_row(1), Some(3));
}

#[test]
fn a_filtered_rail_still_opens_the_page_for_a_clicked_row_and_keeps_the_query() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (rail, _page) = both_fields(&mut window, &mut transport, &mut queue);
    focus_field(&mut window, &mut transport, &mut queue, rail);
    type_text(&mut window, &mut transport, &mut queue, "mid");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist");
    let command = window.click(&mut transport, &mut queue, row.center());

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "playlist:Midnight City".to_owned(),
        }),
        "the click opened the playlist the drawn row stands for"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist("playlist:Midnight City".to_owned()))
    );
    assert_eq!(
        window.library.rail_filter.query(),
        "mid",
        "opening a page cleared the query that found it"
    );
}

#[test]
fn a_created_playlist_is_visible_after_the_create_reply() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);
    window.library.rail_filter.edit().push_str("zzz");

    // The create field takes the filter field's slot, so opening it clears
    // the query: the name about to be typed would be hidden by it.
    open_naming(&mut window, &mut transport, &mut queue);
    assert_eq!(
        window.library.rail_filter.query(),
        "",
        "opening the create field left a query that hides the new row"
    );

    type_text(&mut window, &mut transport, &mut queue, "Night Drive");
    let command = window
        .frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        )
        .0;
    assert!(matches!(command, Some(Command::CreatePlaylist { .. })));
    window.library.apply(&Update::PlaylistCreated {
        entry: new_playlist("Night Drive", page_access(true)),
        revision: revision("root-2"),
    });

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        text_in(&frame, "Night Drive", sidebar_rect(&window)).is_some(),
        "the created row is hidden by the query the create field should have cleared"
    );
}

#[test]
fn space_del_and_hearts_do_nothing_while_the_field_has_the_keyboard() {
    let mut transport = playing(cover("a"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
    liked(&mut window, &[]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
    open_naming(&mut window, &mut transport, &mut queue);
    type_text(&mut window, &mut transport, &mut queue, "Mix");

    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Space, true)]
            )
            .0,
        None,
        "a space reached the transport through the field"
    );
    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Delete, true)]
            )
            .0,
        None,
        "Del cut a row while the field had the keyboard"
    );
    crate::view::heart::post(
        &window.ctx,
        crate::view::heart::Ask {
            tracks: vec!["spotify:track:a".to_owned()],
            liked: true,
        },
    );
    assert_eq!(
        window.frame(&mut transport, &mut queue, Vec::new()).0,
        None,
        "a heart wrote while the field had the keyboard"
    );
    assert_eq!(window.library.likes.state("spotify:track:a"), Some(false));
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn the_count_label_returns_when_naming_closes() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_some(),
        "the field's resting line is missing"
    );

    open_naming(&mut window, &mut transport, &mut queue);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_none(),
        "the count stayed beside the field"
    );

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Escape, true)],
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_some(),
        "the count did not come back"
    );
}

// ---- the head drawn alone, where its own widget ids are nameable ----

/// One frame of the sidebar alone, so a test can name the head's ids.
struct Column {
    command: Option<Command>,
    slot: egui::Id,
    field: egui::Id,
}

/// Draws one frame of the sidebar at the column's own width.
fn column_frame(
    ctx: &egui::Context,
    library: &mut Library,
    art: &mut Art,
    events: Vec<egui::Event>,
    drag: bool,
) -> Column {
    let tokens = Tokens::DARK;
    let band = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(254.0, 800.0));
    let input = egui::RawInput {
        screen_rect: Some(band),
        events,
        ..Default::default()
    };
    let mut slot = egui::Id::NULL;
    let mut field = egui::Id::NULL;
    let mut command = None;
    art.begin_frame();
    let mut output = ctx.run_ui(input, |ui| {
        let control = crate::availability::Control::LibraryCreate;
        slot = ui.scope_id().with((control, ()));
        field = ui.scope_id().with((control, "name"));
        command = if drag {
            sidebar::column(ui, library, art, Instant::now(), &tokens)
        } else {
            sidebar::show(ui, library, art, Instant::now(), &tokens, true).command
        };
    });
    output.textures_delta.clear();
    Column {
        command,
        slot,
        field,
    }
}

/// A styled context for the sidebar's own frames.
fn column_context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

/// Clicks `at` over the passes egui's hit test needs.
fn column_click(
    ctx: &egui::Context,
    library: &mut Library,
    art: &mut Art,
    drag: bool,
    at: egui::Pos2,
) -> Column {
    let _ = column_frame(ctx, library, art, Vec::new(), drag);
    let mut last = column_frame(ctx, library, art, Vec::new(), drag);
    for event in crate::view::test_support::click_events(at) {
        last = column_frame(ctx, library, art, vec![event], drag);
    }
    last
}

/// A library whose sidebar holds `names`, with a rootlist revision.
fn column_library(names: &[&str], rootlist: bool) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(
        names
            .iter()
            .map(|name| PlaylistEntry {
                composed: Vec::new(),
                uri: format!("playlist:{name}"),
                name: (*name).to_owned(),
                named: true,
                cover: None,
                description: None,
                owner: None,
                tracks: None,
                access: None,
            })
            .collect(),
    ));
    if rootlist {
        library.apply(&Update::RootlistAccess(rootlist_access(true)));
    }
    library
}

#[test]
fn escape_on_the_frame_the_field_opens() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["a"], true);
    library.naming.open();
    library.naming.edit().push_str("Night Drive");

    // The field's key filter is stored only on the frame after it first
    // holds the keyboard, so this Escape belongs to nobody: the frame opens
    // the field and the key is dropped rather than closing it.
    let opened = column_frame(
        &ctx,
        &mut library,
        &mut art,
        vec![key(egui::Key::Escape, true)],
        true,
    );

    assert!(
        library.naming.is_open(),
        "the opening frame's Escape closed the field"
    );
    assert_eq!(library.naming.text(), "Night Drive");
    assert!(
        ctx.memory(|memory| memory.has_focus(opened.field)),
        "the opening frame's Escape took the keyboard off the field"
    );
    assert_eq!(opened.command, None);
}

#[test]
fn escape_cancels_and_returns_focus_to_plus() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["a"], true);
    library.naming.open();
    library.naming.edit().push_str("Night Drive");

    let opened = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);
    assert!(
        ctx.memory(|memory| memory.has_focus(opened.field)),
        "the field never took the keyboard"
    );
    // One frame for the field's own key filter to be the stored one.
    let _ = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);

    let escaped = column_frame(
        &ctx,
        &mut library,
        &mut art,
        vec![key(egui::Key::Escape, true)],
        true,
    );
    assert!(!library.naming.is_open(), "Escape did not close the field");
    assert_eq!(library.naming.text(), "");
    assert!(
        ctx.memory(|memory| memory.has_focus(escaped.slot)),
        "the keyboard did not go back to the +"
    );
}

#[test]
fn escape_does_nothing_when_the_field_is_closed() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["a"], true);

    let closed = column_frame(
        &ctx,
        &mut library,
        &mut art,
        vec![key(egui::Key::Escape, true)],
        true,
    );

    assert!(!library.naming.is_open());
    assert_eq!(closed.command, None);
    assert_eq!(library.naming.text(), "");
}

#[test]
fn tab_leaves_the_field() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["a"], true);
    library.naming.open();
    library.naming.edit().push_str("Night Drive");
    let opened = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);
    assert!(ctx.memory(|memory| memory.has_focus(opened.field)));
    // One frame for the field's own key filter to be the stored one.
    let _ = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);

    let _ = column_frame(
        &ctx,
        &mut library,
        &mut art,
        vec![key(egui::Key::Tab, true)],
        true,
    );
    let after = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);

    assert!(
        !ctx.memory(|memory| memory.has_focus(after.field)),
        "Tab left the keyboard in the field"
    );
    assert!(library.naming.is_open(), "Tab closed the field");
    assert_eq!(library.naming.text(), "Night Drive");
}

#[test]
fn the_plus_is_inert_in_the_drawer_form() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["a"], true);
    let band = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(254.0, 800.0));

    let _ = column_click(&ctx, &mut library, &mut art, false, head_plus(band));

    assert!(
        !library.naming.is_open(),
        "the drawer's + opened a field whose command is discarded"
    );
}

#[test]
fn a_naming_session_does_not_clear_the_query_in_the_drawer() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["Midnight City", "Outro"], true);
    library.rail_filter.edit().push_str("midnight");
    // The drawer's create button is inert, so the head keeps the filter
    // field on screen even while a naming session is open.
    library.naming.open();

    let _ = column_frame(&ctx, &mut library, &mut art, Vec::new(), false);

    assert!(library.naming.is_open());
    assert_eq!(
        library.rail_filter.query(),
        "midnight",
        "the drawer's naming session cleared the query with the column's slot"
    );
}

// ---- the drawer's copy of the rail field ----

/// A narrow window with the drawer open and a few playlists to narrow.
fn drawer_with_rows(transport: &mut Transport, queue: &mut Queue) -> Window {
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[
            ("Midnight City", None),
            ("Outro", None),
            ("Midnight Sun", None),
        ],
        None,
    );
    open_drawer(&mut window, transport, queue);
    window
}

#[test]
fn a_pick_in_the_drawer_loads_the_page_it_names() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    let drawer = drawer_rect(&window);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    // The page behind draws the same playlist as a card, so the drawer's own
    // copy is the last one painted.
    let row = frame
        .shapes
        .iter()
        .rev()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text)
                if text.galley.text() == "Midnight City" && drawer.contains(text.pos) =>
            {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("the drawer drew the row");

    let command = window.click(&mut transport, &mut queue, row.center());

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "playlist:Midnight City".to_owned()
        }),
        "the drawer's pick never reached the frame: {command:?}"
    );
    assert_eq!(window.drawer, Drawer::Closed, "a pick closes the drawer");
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist("playlist:Midnight City".to_owned())),
        "the page the row named did not open"
    );
}

/// A page that is already open asks for nothing, so `settle` returns no
/// command. The press is still the user picking a row, so the drawer closes
/// rather than staying open over a page the press did nothing to.
#[test]
fn a_pick_of_the_page_already_open_closes_the_drawer_without_a_command() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    removable_rows(&mut window);
    assert!(window.library.choose_kind(SidebarKind::Artists));
    open_drawer(&mut window, &mut transport, &mut queue);
    let drawer = drawer_rect(&window);
    let row = |window: &mut Window, transport: &mut Transport, queue: &mut Queue| {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        frame
            .shapes
            .iter()
            .rev()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.text() == "Vela Moreno" && drawer.contains(text.pos) =>
                {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("the drawer drew the row")
            .center()
    };

    let first = row(&mut window, &mut transport, &mut queue);
    assert!(window.click(&mut transport, &mut queue, first).is_some());
    assert_eq!(window.drawer, Drawer::Closed);

    open_drawer(&mut window, &mut transport, &mut queue);
    let again = row(&mut window, &mut transport, &mut queue);
    assert_eq!(
        window.click(&mut transport, &mut queue, again),
        None,
        "a page already open asks for nothing"
    );
    assert_eq!(
        window.drawer,
        Drawer::Closed,
        "the press was still a pick, so the drawer closes"
    );
}

/// The drawer draws after every region, and a `ScrollArea` takes the wheel
/// only when its own layer holds the pointer. A drawer whose area is
/// zero-sized never holds it, so the page behind scrolled and the drawer's
/// rows did not.
#[test]
fn the_wheel_scrolls_the_drawer_and_leaves_the_page_behind() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    let tracks: Vec<String> = (0..40).map(|index| format!("t{index}")).collect();
    let titles: Vec<&str> = tracks.iter().map(String::as_str).collect();
    playlist_page(&mut window, &titles);
    let names: Vec<String> = (0..40).map(|index| format!("Row {index}")).collect();
    let rows: Vec<(&str, Option<usize>)> = names.iter().map(|name| (name.as_str(), None)).collect();
    sidebar_playlists(&mut window, &rows, None);
    open_drawer(&mut window, &mut transport, &mut queue);
    let drawer = drawer_rect(&window);

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(drawer.center())],
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let page_before = painted_at(&frame, "t3").expect("the page drew its tracks");
    let drawer_before = text_in(&frame, "Row 6", drawer).expect("the drawer drew its rows");

    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    for _ in 0..4 {
        let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
    }
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let page_after = painted_at(&frame, "t3").expect("the page still draws its tracks");
    let drawer_after = text_in(&frame, "Row 6", drawer).expect("the drawer still draws its rows");

    assert!(
        drawer_after.top() < drawer_before.top() - 20.0,
        "the wheel did not scroll the drawer: {} then {}",
        drawer_before.top(),
        drawer_after.top()
    );
    assert!(
        (page_after.y - page_before.y).abs() < 0.01,
        "the page behind the drawer moved: {} then {}",
        page_before.y,
        page_after.y
    );
}

/// A failed list asks for itself on the frame it is drawn. That is a
/// command, not a pick: the drawer must stay open, or a list that failed
/// while the rail was narrow could never be retried from it.
#[test]
fn a_failed_list_retries_in_the_drawer_without_closing_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    window.library.playlists = Load::Failed(Failure::Load);

    open_drawer(&mut window, &mut transport, &mut queue);

    assert_eq!(
        window.drawer,
        Drawer::Open,
        "a retry read as a picked row and closed the drawer"
    );
    assert!(
        matches!(window.library.playlists, Load::Loading),
        "the failed list did not ask for itself"
    );
}

#[test]
fn the_drawer_draws_the_same_field_and_it_is_live() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    let field = drawer_field_rect(&mut window, &mut transport, &mut queue);

    focus_field(&mut window, &mut transport, &mut queue, field);
    type_text(&mut window, &mut transport, &mut queue, "midnight");

    assert_eq!(window.library.rail_filter.query(), "midnight");
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let drawer = drawer_rect(&window);
    assert!(
        text_in(&frame, "Midnight City", drawer).is_some()
            && text_in(&frame, "Midnight Sun", drawer).is_some(),
        "the drawer's field did not narrow its rows"
    );
    assert!(
        text_in(&frame, "Outro", drawer).is_none(),
        "the drawer drew a row the query excludes"
    );
}

#[test]
fn a_click_on_the_main_pane_still_closes_the_drawer() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    // Below and right of the drawer, where the root grid has drawn nothing.
    let outside = egui::pos2(window.size.x - 50.0, window.size.y - 200.0);

    let _ = window.click(&mut transport, &mut queue, outside);

    assert_eq!(
        window.drawer,
        Drawer::Closed,
        "a click outside the drawer left it open"
    );
}

#[test]
fn a_click_on_the_drawers_own_panel_leaves_it_open_and_moves_no_focus() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
    let drawer = drawer_rect(&window);
    // The band below the chips and above the head's rule: the drawer's own
    // panel, with no control drawn on it.
    let chips = 2.0 * extent::BUTTON + space::S1;
    let spot = egui::pos2(
        drawer.center().x,
        drawer.top() + space::S4 + chips + space::S3 / 2.0,
    );
    assert!(
        drawer.contains(spot) && !field.contains(spot),
        "the click lands on the drawer's panel, not its field: {spot:?} in {drawer:?}, \
         field {field:?}"
    );

    let _ = window.click(&mut transport, &mut queue, spot);

    assert_eq!(
        window.drawer,
        Drawer::Open,
        "a click on the drawer's own panel closed it"
    );
    assert!(
        !window.ctx.text_edit_focused(),
        "a click on the drawer's own panel focused a field"
    );
}

#[test]
fn escape_in_the_drawer_field_clears_before_it_closes_the_drawer() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
    focus_field(&mut window, &mut transport, &mut queue, field);
    type_text(&mut window, &mut transport, &mut queue, "midnight");

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(
        window.library.rail_filter.query(),
        "",
        "the first Escape did not clear the drawer's field"
    );
    assert_eq!(
        window.drawer,
        Drawer::Open,
        "the Escape that cleared the query also closed the drawer"
    );

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    assert_eq!(
        window.drawer,
        Drawer::Closed,
        "the second Escape left the drawer open"
    );
}

#[test]
fn an_empty_drawer_field_surrenders_focus_on_the_first_escape_and_the_second_closes_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
    focus_field(&mut window, &mut transport, &mut queue, field);

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert!(
        !window.ctx.text_edit_focused(),
        "the first Escape left the empty field holding the keyboard"
    );
    assert_eq!(
        window.drawer,
        Drawer::Open,
        "the field's Escape closed the drawer"
    );

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    assert_eq!(
        window.drawer,
        Drawer::Closed,
        "the second Escape left the drawer open"
    );
}

#[test]
fn the_query_typed_in_the_drawer_is_there_after_widening_to_the_column() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = drawer_with_rows(&mut transport, &mut queue);
    let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
    focus_field(&mut window, &mut transport, &mut queue, field);
    type_text(&mut window, &mut transport, &mut queue, "midnight");

    window.size = Vec2::new(1400.0, 900.0);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(
        window.library.rail_filter.query(),
        "midnight",
        "widening to the column lost the drawer's query"
    );
    assert_eq!(
        window.drawer,
        Drawer::Closed,
        "the column kept the drawer open"
    );
    let area = sidebar_rect(&window);
    assert!(
        rail_filter_rect(&frame, area).width() > 0.0,
        "the column's own field is painted"
    );
    assert!(
        text_in(&frame, "midnight", area).is_some(),
        "the column's field does not draw the drawer's text"
    );
    assert!(
        text_in(&frame, "Outro", area).is_none(),
        "the column drew a row the drawer's query excludes"
    );
}

#[test]
fn the_plus_is_inert_when_creatable_is_false() {
    let ctx = column_context();
    let mut art = Art::default();
    let mut library = column_library(&["a"], false);
    let band = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(254.0, 800.0));

    let _ = column_click(&ctx, &mut library, &mut art, true, head_plus(band));

    assert!(!library.creatable());
    assert!(
        !library.naming.is_open(),
        "a + with no rootlist to link against opened a field"
    );
}

// ---- the created row in the sidebar's list ----

#[test]
fn a_created_row_is_drawn_first() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let area = sidebar_rect(&window);

    window.library.naming.open();
    window.library.naming.edit().push_str("Night Drive");
    assert!(window.library.create_playlist().is_some());
    window.library.apply(&Update::PlaylistCreated {
        entry: new_playlist("Night Drive", page_access(true)),
        revision: revision("root-2"),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    let created = text_in(&frame, "Night Drive", area).expect("the new row is painted");
    let first = text_in(&frame, "a", area).expect("the old first row is painted");
    assert!(
        created.top() < first.top(),
        "the created row is not the first one drawn"
    );
}

#[test]
fn the_reveal_scrolls_to_the_top_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let names: Vec<String> = (0..30).map(|index| format!("r{index}")).collect();
    let rows: Vec<(&str, Option<usize>)> =
        names.iter().map(|name| (name.as_str(), Some(0))).collect();
    sidebar_playlists(&mut window, &rows, Some(rootlist_access(true)));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let area = sidebar_rect(&window);

    // The pointer has to be over the list for the wheel to reach it.
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(egui::pos2(
            area.center().x,
            area.bottom() - 40.0,
        ))],
    );
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -600.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    for _ in 0..3 {
        let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
    }
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "r0", area).is_none(),
        "the list never scrolled away from its first row"
    );

    window.library.naming.open();
    window.library.naming.edit().push_str("Night Drive");
    assert!(window.library.create_playlist().is_some());
    window.library.apply(&Update::PlaylistCreated {
        entry: new_playlist("Night Drive", page_access(true)),
        revision: revision("root-2"),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "Night Drive", area).is_some(),
        "the created row is not on screen"
    );

    // The reveal is one frame's: the wheel still moves the list afterwards.
    for _ in 0..3 {
        let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
    }
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "Night Drive", area).is_none(),
        "the list was forced back to the top on every frame"
    );
}

/// Submits the open field with `name` typed into it, and answers the frame.
fn submit_name(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    name: &str,
) -> Option<Command> {
    open_naming(window, transport, queue);
    type_text(window, transport, queue, name);
    window
        .frame(transport, queue, vec![key(egui::Key::Enter, true)])
        .0
}

/// Points the keyboard at `field`, settling the frame egui needs before the
/// field owns a key: it stores a widget's key filter only for a widget that
/// had focus last frame, so the Escape immediately after a click is not yet
/// the field's.
fn focus_field(window: &mut Window, transport: &mut Transport, queue: &mut Queue, field: Rect) {
    let _ = window.click(transport, queue, field.center());
    let _ = window.frame(transport, queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the click focused a filter field"
    );
}

/// A playlist page open with rows to filter, and playlists in the rail, at a
/// width where both fields are drawn. The returned rects are the two fields.
fn both_fields(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> (Rect, Rect) {
    playlist_page(window, &["keep0", "p1"]);
    sidebar_playlists(
        window,
        &[
            ("Midnight City", None),
            ("Outro", None),
            ("Midnight Sun", None),
        ],
        // The create button is a Tab stop only while it is live, and it is
        // live only when the rootlist allows a write.
        Some(rootlist_access(true)),
    );
    field_rects(window, transport, queue)
}

/// The same two fields in a library that cannot create a playlist: with no
/// rootlist revision the head's `+` is inert, so it is no Tab stop.
fn both_fields_without_create(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
) -> (Rect, Rect) {
    playlist_page(window, &["keep0", "p1"]);
    sidebar_playlists(
        window,
        &[
            ("Midnight City", None),
            ("Outro", None),
            ("Midnight Sun", None),
        ],
        None,
    );
    field_rects(window, transport, queue)
}

/// Every Tab stop from the first press until the page's field takes the
/// keyboard, named by the part of the shell it sits in.
fn tab_stops_until_page_field(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    rail: Rect,
    page: Rect,
) -> Vec<&'static str> {
    let rail_area = sidebar_rect(window);
    let topbar = topbar_rect(window);
    let mut stops: Vec<&'static str> = Vec::new();
    for _ in 0..48 {
        let _ = press(window, transport, queue, egui::Key::Tab);
        let Some(stop) = focused_stop(window) else {
            continue;
        };
        let kind = if rail.contains(stop.center()) {
            "rail-field"
        } else if page.contains(stop.center()) {
            "page-field"
        } else if topbar.contains_rect(stop) {
            "topbar"
        } else if rail_area.contains_rect(stop) {
            if stop.center().y < rail.top() {
                "rail-chips"
            } else if stop.center().y <= rail.bottom() {
                "rail-head"
            } else {
                "rail-row"
            }
        } else {
            "other"
        };
        stops.push(kind);
        if kind == "page-field" {
            break;
        }
    }
    stops
}

/// Opens the drawer with the topbar's toggle and settles its first frame.
fn open_drawer(window: &mut Window, transport: &mut Transport, queue: &mut Queue) {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let toggle = topbar_buttons(&frame)[0];
    let _ = window.click(transport, queue, toggle.center());
    assert_eq!(window.drawer, Drawer::Open, "the toggle opened no drawer");
    let _ = window.frame(transport, queue, Vec::new());
}

/// The drawer's copy of the rail field, read from the hairline under it.
fn drawer_field_rect(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> Rect {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    rail_filter_rect(&frame, drawer_rect(window))
}

/// The two fields' boxes, read from one frame at a width where both are
/// drawn.
fn field_rects(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> (Rect, Rect) {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let rail = rail_filter_rect(&frame, sidebar_rect(window));
    // `page_filter_field` answers the field's bottom hairline; the field is
    // one button tall above it.
    let hairline =
        page_filter_field(&frame, view_rect(window)).expect("the page's filter field is painted");
    let page = Rect::from_min_max(
        egui::pos2(hairline.left(), hairline.bottom() - extent::BUTTON),
        egui::pos2(hairline.right(), hairline.bottom()),
    );
    (rail, page)
}
