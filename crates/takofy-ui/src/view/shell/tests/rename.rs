use super::super::sidebar;
use super::super::*; // the module under test — what `use super::*` gave the flat module
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn apply_rename_opens_the_field_on_the_row_or_the_band() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");

    assert_eq!(
        try_rename(
            &mut window,
            "spotify:playlist:a",
            crate::naming::Surface::Row
        ),
        None
    );
    assert!(
        window
            .library
            .renaming
            .is_open_on("spotify:playlist:a", crate::naming::Surface::Row)
    );

    window.library.renaming.force_close();
    assert_eq!(
        try_rename(
            &mut window,
            "spotify:playlist:a",
            crate::naming::Surface::Band
        ),
        None
    );
    assert!(
        window
            .library
            .renaming
            .is_open_on("spotify:playlist:a", crate::naming::Surface::Band)
    );
}

#[test]
fn apply_rename_drops_the_intent_when_the_field_cannot_be_seen() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");

    window.drawer = Drawer::Open;
    assert_eq!(
        try_rename(
            &mut window,
            "spotify:playlist:a",
            crate::naming::Surface::Row
        ),
        None
    );
    assert!(
        !window.library.renaming.is_open(),
        "the drawer hid the field"
    );
    window.drawer = Drawer::Closed;

    carry::lift(&window.ctx, carried_rows(egui::Id::unique("list"), vec![0]));
    assert_eq!(
        try_rename(
            &mut window,
            "spotify:playlist:a",
            crate::naming::Surface::Row
        ),
        None
    );
    assert!(
        !window.library.renaming.is_open(),
        "a carried block held the field shut"
    );
    egui::DragAndDrop::clear_payload(&window.ctx);
}

#[test]
fn apply_rename_leaves_a_typed_field_alone() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    creatable_sidebar(&mut window);
    open_naming(&mut window, &mut transport, &mut queue);
    assert!(window.ctx.text_edit_focused());

    assert_eq!(
        try_rename(&mut window, "playlist:a", crate::naming::Surface::Row),
        None
    );
    assert!(
        !window.library.renaming.is_open(),
        "a typed field was replaced"
    );
}

/// Opens a sidebar row's menu with Rename on it.
fn open_rename_menu(window: &mut Window, row: LibraryItem) {
    let request = crate::menu::Request {
        door: crate::menu::Door {
            anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                egui::Pos2::new(60.0, 300.0),
                Vec2::new(28.0, 28.0),
            )),
            opener: egui::Id::unique("library-row"),
            device: crate::menu::Device::Keyboard,
        },
        playlists: Vec::new(),
        items: crate::verbs::library_items(
            &row,
            crate::verbs::LibraryRights {
                renamable: true,
                removable: true,
                saved: Some(true),
            },
        ),
        targets: Vec::new(),
        rows: Vec::new(),
        listing: Listing::Library(row),
        page: window.library.page().cloned(),
    };
    assert!(
        window.menu.open(request, Gate::default()),
        "the menu opened"
    );
}

/// Opens a row's rename field from its menu and waits for the keyboard.
fn open_row_rename(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    uri: &str,
    name: &str,
) {
    open_rename_menu(window, library_row(SidebarKind::Playlists, uri, name));
    settle_menu(window, transport, queue);
    let _ = press(window, transport, queue, egui::Key::Enter);
    let _ = window.frame(transport, queue, Vec::new());
    let _ = window.frame(transport, queue, Vec::new());
    assert!(
        window
            .library
            .renaming
            .is_open_on(uri, crate::naming::Surface::Row),
        "the Rename row opened no field"
    );
    assert!(
        window.ctx.text_edit_focused(),
        "the keyboard did not follow the field"
    );
}

#[test]
fn a_row_menu_rename_opens_the_field_with_the_current_name() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");

    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );

    assert_eq!(window.library.renaming.text(), "A");
    assert!(!window.library.write_in_flight());
}

#[test]
fn typing_and_enter_sends_the_trimmed_name() {
    for (typed, expected) in [
        ("  Night É  ".to_owned(), "Night É".to_owned()),
        ("é".repeat(100), "é".repeat(100)),
    ] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        window.library.renaming.edit().clear();
        type_text(&mut window, &mut transport, &mut queue, &typed);
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(
            command,
            Some(Command::RenamePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                name: expected,
            })
        );
        assert!(window.library.renaming.writing());
    }
}

#[test]
fn escape_closes_the_row_field_and_sends_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );

    window.library.renaming.edit().clear();
    type_text(&mut window, &mut transport, &mut queue, "New");
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(!window.library.renaming.is_open());
    assert!(!window.library.write_in_flight());
}

#[test]
fn a_blank_blur_cancels_but_a_named_blur_stays_open() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );

    window.library.renaming.edit().clear();
    let id = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("a focus");
    window.ctx.memory_mut(|memory| memory.surrender_focus(id));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(!window.library.renaming.is_open(), "a blank blur stayed");

    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    let id = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("a focus");
    window.ctx.memory_mut(|memory| memory.surrender_focus(id));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.library.renaming.is_open(),
        "a named blur lost the name"
    );
    assert_eq!(window.library.renaming.text(), "A");
}

#[test]
fn delete_inside_the_row_field_edits_text_and_removes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );

    type_text(&mut window, &mut transport, &mut queue, "B");
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Home);
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None, "Del started a removal");
    assert_eq!(window.library.renaming.text(), "B", "Del left the text");
    assert_eq!(sidebar_order(&window), ["A"], "Del removed the row");
    assert!(!window.library.write_in_flight());
}

#[test]
fn a_drag_from_the_open_row_begins_no_carry() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = sidebar_line(&frame, &window, "A");
    let to = from + Vec2::new(0.0, sidebar::ROW_H + 8.0);

    let command = window.drag(&mut transport, &mut queue, from, to);

    assert_eq!(command, None);
    assert!(
        !carry::carrying(&window.ctx),
        "a drag out of the field began a carry"
    );
    assert!(window.library.renaming.is_open());
}

#[test]
fn no_menu_opens_on_another_row_while_the_rename_writes() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", Some(0)), ("B", Some(0))],
        Some(rootlist_access(true)),
    );
    open_row_rename(&mut window, &mut transport, &mut queue, "playlist:A", "A");
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
    assert!(window.library.write_in_flight());

    right_click_sidebar_row(&mut window, &mut transport, &mut queue, "B");

    assert_eq!(
        crate::view::context_menu::take_request(&window.ctx),
        None,
        "a menu opened over a rename in flight"
    );
}

#[test]
fn a_confirmed_row_rename_paints_the_new_name() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    type_text(&mut window, &mut transport, &mut queue, "New Name");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());

    window.library.apply(&Update::PlaylistRenamed {
        playlist: "spotify:playlist:a".to_owned(),
        name: "New Name".to_owned(),
        revision: revision("rev-2"),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        text_in(&frame, "New Name", sidebar_rect(&window)).is_some(),
        "the row still paints its old name"
    );
    assert!(!window.library.renaming.is_open());
}

#[test]
fn a_failed_row_rename_keeps_the_field_and_shows_the_foot_sentence() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    window.library.renaming.edit().clear();
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());

    window.library.apply(&Update::PlaylistRenameFailed {
        playlist: "spotify:playlist:a".to_owned(),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(window.library.renaming.is_open());
    assert_eq!(window.library.renaming.text(), "New");
    assert!(
        text_in(
            &frame,
            crate::signin::failure_message(Failure::Rename),
            sidebar_rect(&window)
        )
        .is_some(),
        "the foot did not say why the field stayed"
    );
}

/// The row's own widget id, taken from a press on it: a press on the row
/// takes the keyboard, which is the place Tab continues from.
fn press_sidebar_row(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    name: &str,
) -> egui::Id {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = sidebar_line(&frame, window, name);
    for event in crate::view::test_support::click_events(at) {
        let _ = window.frame(transport, queue, vec![event]);
    }
    window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the row took no keyboard")
}

/// The band title's widget id, taken from the press that opens its field:
/// a press on the title takes the keyboard before the field draws.
fn press_band_title(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    name: &str,
) -> egui::Id {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = text_in(&frame, name, view_rect(window))
        .unwrap_or_else(|| panic!("{name:?} is painted in the band"))
        .center();
    for event in crate::view::test_support::click_events(at) {
        let _ = window.frame(transport, queue, vec![event]);
    }
    assert!(
        window
            .library
            .renaming
            .is_open_on("playlist:Mix", crate::naming::Surface::Band),
        "the title opened no field"
    );
    window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the title took no keyboard")
}

#[test]
fn escape_on_the_row_field_hands_the_keyboard_back_to_its_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    let row = press_sidebar_row(&mut window, &mut transport, &mut queue, "A");

    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(!window.library.renaming.is_open());
    assert!(
        window.ctx.memory(|memory| memory.has_focus(row)),
        "Escape left the keyboard nowhere"
    );
}

#[test]
fn a_confirmed_row_rename_hands_the_keyboard_back_to_its_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    let row = press_sidebar_row(&mut window, &mut transport, &mut queue, "A");

    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some(),
        "Enter sent nothing"
    );
    window.library.apply(&Update::PlaylistRenamed {
        playlist: "spotify:playlist:a".to_owned(),
        name: "New".to_owned(),
        revision: revision("rev-2"),
    });
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(!window.library.renaming.is_open());
    assert!(
        window.ctx.memory(|memory| memory.has_focus(row)),
        "a confirmed rename left the keyboard nowhere"
    );
}

#[test]
fn escape_on_the_band_field_hands_the_keyboard_back_to_the_title() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    band_page(&mut window);
    let title = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the band field has no keyboard"
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(!window.library.renaming.is_open());
    assert!(
        window.ctx.memory(|memory| memory.has_focus(title)),
        "Escape left the keyboard nowhere"
    );
}

#[test]
fn a_confirmed_band_rename_hands_the_keyboard_back_to_the_title() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    band_page(&mut window);
    let title = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some(),
        "Enter sent nothing"
    );
    window.library.apply(&Update::PlaylistRenamed {
        playlist: "playlist:Mix".to_owned(),
        name: "New".to_owned(),
        revision: revision("rev-2"),
    });
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(!window.library.renaming.is_open());
    assert!(
        window.ctx.memory(|memory| memory.has_focus(title)),
        "a confirmed rename left the keyboard nowhere"
    );
}

#[test]
fn a_row_field_closes_when_the_sidebar_leaves_the_column() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );

    // Below the rail width the column is gone: the drawer is the sidebar,
    // and it is closed.
    window.size = Vec2::new(900.0, 900.0);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        !window.library.renaming.is_open(),
        "the rail kept a field no row draws"
    );
}

#[test]
fn escape_after_a_failed_row_rename_takes_the_sentence_with_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    window.library.renaming.edit().clear();
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
    window.library.apply(&Update::PlaylistRenameFailed {
        playlist: "spotify:playlist:a".to_owned(),
    });
    // The reply asks for the keyboard back. The field owns the Escape
    // after that, even though egui stores a filter only for a widget that
    // had focus last frame.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        window.library.sidebar_notice,
        Some(crate::library::SidebarWrite::Rename)
    );
    assert!(
        text_in(
            &frame,
            crate::signin::failure_message(Failure::Rename),
            sidebar_rect(&window)
        )
        .is_some(),
        "the foot never said why"
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(!window.library.renaming.is_open());
    assert_eq!(
        window.library.sidebar_notice, None,
        "the failed rename's sentence outlived its field"
    );
}

#[test]
fn escape_after_a_failed_band_rename_takes_both_sentences_with_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    band_page(&mut window);
    let _ = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
    window.library.apply(&Update::PlaylistRenameFailed {
        playlist: "playlist:Mix".to_owned(),
    });
    // The reply asks for the keyboard back, and the field owns the Escape
    // after that.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        window.library.sidebar_notice,
        Some(crate::library::SidebarWrite::Rename)
    );
    assert_eq!(page_notice(&window), Some(Failure::Rename));

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(!window.library.renaming.is_open());
    assert_eq!(
        window.library.sidebar_notice, None,
        "the foot's sentence outlived the field"
    );
    assert_eq!(
        page_notice(&window),
        None,
        "the page's sentence outlived the field"
    );
}

/// A failed rename asks for the keyboard back in the reply's own frame.
/// The first Escape after that must close the field: a second press is not
/// the user's first answer, and the field may not have one coming.
#[test]
fn one_escape_closes_a_row_field_after_a_failed_rename() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    window.library.renaming.edit().clear();
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
    window.library.apply(&Update::PlaylistRenameFailed {
        playlist: "spotify:playlist:a".to_owned(),
    });
    // The reply's frame is the one that asks for the keyboard back.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the failed rename left the field without the keyboard"
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(
        !window.library.renaming.is_open(),
        "one Escape after the failure was swallowed"
    );
}

/// The band field's half of the same claim: the failed reply hands the
/// keyboard back, and the very next Escape is the one that closes it.
#[test]
fn one_escape_closes_a_band_field_after_a_failed_rename() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    band_page(&mut window);
    let _ = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
    window.library.apply(&Update::PlaylistRenameFailed {
        playlist: "playlist:Mix".to_owned(),
    });
    // The reply's frame is the one that asks for the keyboard back.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the failed rename left the band field without the keyboard"
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert_eq!(command, None);
    assert!(
        !window.library.renaming.is_open(),
        "one Escape after the failure was swallowed"
    );
}

#[test]
fn a_dismissed_row_field_takes_the_failed_rename_sentence_with_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );
    window.library.renaming.edit().clear();
    type_text(&mut window, &mut transport, &mut queue, "New");
    assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
    window.library.apply(&Update::PlaylistRenameFailed {
        playlist: "spotify:playlist:a".to_owned(),
    });
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        window.library.sidebar_notice,
        Some(crate::library::SidebarWrite::Rename)
    );

    // A chip change dismisses the row's field rather than cancelling it.
    assert!(window.library.choose_kind(SidebarKind::Podcasts));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(!window.library.renaming.is_open());
    assert_eq!(
        window.library.sidebar_notice, None,
        "a dismissed field kept the sentence it explained"
    );
}

/// The sentence a playlist page carries, when it carries one.
fn page_notice(window: &Window) -> Option<Failure> {
    match &window.library.tracks {
        Load::Ready(tracks) => tracks.notice,
        Load::Idle | Load::Loading | Load::Failed(_) => None,
    }
}

#[test]
fn a_chip_change_dismisses_an_open_row_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_row(&mut window, "spotify:playlist:a", "A");
    open_row_rename(
        &mut window,
        &mut transport,
        &mut queue,
        "spotify:playlist:a",
        "A",
    );

    assert!(window.library.choose_kind(SidebarKind::Podcasts));

    assert!(!window.library.renaming.is_open());
}

/// Posts a band menu the way the band's "…" does.
fn open_band_menu(window: &mut Window, row: LibraryItem, rights: crate::verbs::LibraryRights) {
    let request = crate::menu::Request {
        door: crate::menu::Door {
            anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                egui::Pos2::new(60.0, 300.0),
                Vec2::new(28.0, 28.0),
            )),
            opener: egui::Id::unique("band-more"),
            device: crate::menu::Device::Keyboard,
        },
        playlists: Vec::new(),
        items: crate::verbs::library_items(&row, rights),
        targets: Vec::new(),
        rows: Vec::new(),
        listing: Listing::Band(row),
        page: window.library.page().cloned(),
    };
    assert!(
        window.menu.open(request, Gate::default()),
        "the menu opened"
    );
}

/// A playlist page whose sidebar row is the same playlist.
fn band_page(window: &mut Window) {
    sidebar_playlists(window, &[("Mix", Some(0))], Some(rootlist_access(true)));
    window.library.select("playlist:Mix");
    window.library.apply(&Update::Tracks {
        playlist: "playlist:Mix".to_owned(),
        tracks: vec![TrackEntry {
            uri: "list:p0".to_owned(),
            title: "p0".to_owned(),
            ..track(0, None)
        }],
        cover: None,
        access: page_access(true),
    });
}

#[test]
fn removing_from_the_band_menu_unlinks_and_takes_the_page_home() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    band_page(&mut window);
    let row = library_row(SidebarKind::Playlists, "playlist:Mix", "Mix");
    open_band_menu(
        &mut window,
        row,
        crate::verbs::LibraryRights {
            renamable: true,
            removable: true,
            saved: Some(true),
        },
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command,
        Some(Command::RemovePlaylist {
            playlist: "playlist:Mix".to_owned(),
            revision: revision("root-1"),
            index: 0,
        })
    );
    window.library.apply(&Update::PlaylistRemoved {
        playlist: "playlist:Mix".to_owned(),
        revision: revision("root-2"),
    });
    assert!(
        window.library.page().is_none(),
        "the page stayed after its list left"
    );
}

#[test]
fn copy_link_on_the_band_menu_writes_the_playlist_url() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
    sidebar_row(&mut window, uri, "Deep Focus");
    window.library.select(uri);
    let row = library_row(SidebarKind::Playlists, uri, "Deep Focus");
    open_band_menu(
        &mut window,
        row,
        crate::verbs::LibraryRights {
            renamable: false,
            removable: false,
            saved: Some(true),
        },
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let (_, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    let copied = frame
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        });
    assert_eq!(
        copied,
        Some("https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC")
    );
}

// -----------------------------------------------------------------------
// The seam under the view: a page's pinned height belongs to its page.
// -----------------------------------------------------------------------

/// Applies a rename the way the menu's Rename row would.
fn try_rename(window: &mut Window, uri: &str, surface: crate::naming::Surface) -> Option<Command> {
    let ctx = window.ctx.clone();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let item = library_row(SidebarKind::Playlists, uri, uri);
    let mut command = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, window.size)),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        let mut screen = Screen {
            state: &mut window.state,
            library: &mut window.library,
            transport: &mut transport,
            queue: &mut queue,
            devices: &mut window.devices,
            list: &mut window.list,
            drawer: &mut window.drawer,
            menu: &mut window.menu,
            position_ms: 0,
            art: &mut window.art,
            now: window.now,
            now_ms: 0,
            pane: &mut window.pane,
        };
        command = apply_rename(ui, &mut screen, &item, surface);
    });
    output.textures_delta.clear();
    command
}
