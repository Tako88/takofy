use super::super::*;
use super::test_support::*;
use crate::message::{Failure, Update};
use egui::{Context, Event, Modifiers, Pos2, Rect, Vec2};
use takofy_model::{PlaylistAccess, Revision};

/// A playlist page whose sidebar entry says whether the user may write it.
fn band_library(name: &str, editable: bool, titles: &[&str]) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: "spotify:playlist:a".to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: Some(titles.len()),
        access: access(editable),
    }]));
    library.apply(&Update::RootlistAccess(PlaylistAccess {
        revision: Revision::from_wire(b"root-1").expect("a revision"),
        editable: true,
    }));
    library.select("spotify:playlist:a");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:a".to_owned(),
        tracks: titles.iter().map(|title| track(title, title)).collect(),
        cover: None,
        access: access(editable),
    });
    library
}

/// One frame of the tracklist with `events`, reporting its command.
fn band_frame(
    ctx: &Context,
    library: &mut Library,
    events: Vec<Event>,
) -> (Option<Command>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut command = None;
    let output = run(ctx, input(events), |ui| {
        command = tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    });
    (command, output)
}

/// Clicks the band's title, returning the command the view reported.
fn click_band_title(ctx: &Context, library: &mut Library, name: &str) -> Option<Command> {
    let output = tracklist_frame(ctx, Table::Full, library);
    let at = galley_at(&output, name) + Vec2::new(20.0, 20.0);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    click(ctx, at, library, &mut transport, &mut queue)
}

/// The "…" in the band's action row, from the icons it paints beside play.
fn band_more_point(ctx: &Context, library: &mut Library, name: &str) -> Pos2 {
    let output = tracklist_frame(ctx, Table::Full, library);
    let play = play_tile_rect_for(ctx, name);
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Path(_) => Some(clipped.shape.visual_bounding_rect()),
            _ => None,
        })
        .filter(|rect| rect.center().y < play.center().y + 20.0)
        .filter(|rect| rect.center().x > play.right())
        .max_by(|left, right| left.center().x.total_cmp(&right.center().x))
        .map(|rect| rect.center())
        .expect("the band paints its `⋯`")
}

#[test]
fn a_title_click_opens_the_band_field_only_on_a_list_of_ours() {
    let ctx = context();
    let mut editable = band_library("Night Drive", true, &["t0"]);
    assert_eq!(click_band_title(&ctx, &mut editable, "Night Drive"), None);
    assert!(
        editable
            .renaming
            .is_open_on("spotify:playlist:a", Surface::Band)
    );
    assert_eq!(editable.renaming.text(), "Night Drive");

    let mut read_only = band_library("Night Drive", false, &["t0"]);
    assert_eq!(click_band_title(&ctx, &mut read_only, "Night Drive"), None);
    assert!(!read_only.renaming.is_open());
}

#[test]
fn a_title_click_waits_while_a_block_is_carried_or_a_write_is_pending() {
    let ctx = context();
    let mut carried = band_library("Night Drive", true, &["t0"]);
    egui::DragAndDrop::set_payload(&ctx, ());
    assert_eq!(click_band_title(&ctx, &mut carried, "Night Drive"), None);
    assert!(!carried.renaming.is_open(), "a carry opened the field");
    egui::DragAndDrop::clear_payload(&ctx);

    let mut writing = band_library("Night Drive", true, &["t0", "t1"]);
    let first = match &writing.tracks {
        Load::Ready(tracks) => tracks.tracks[0].clone(),
        Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
    };
    assert!(
        writing
            .remove_tracks("spotify:playlist:a", &[0], &[first])
            .is_some()
    );
    assert_eq!(click_band_title(&ctx, &mut writing, "Night Drive"), None);
    assert!(!writing.renaming.is_open(), "a write opened the field");
}

#[test]
fn the_band_menu_offers_rename_first_and_never_on_a_read_only_list() {
    let ctx = context();
    let mut editable = band_library("Night Drive", true, &["t0"]);
    let at = band_more_point(&ctx, &mut editable, "Night Drive");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert_eq!(
        click(&ctx, at, &mut editable, &mut transport, &mut queue),
        None
    );
    let request = crate::view::context_menu::take_request(&ctx).expect("the band posted a menu");
    assert_eq!(
        request.listing,
        crate::menu::Listing::Band(LibraryItem {
            kind: SidebarKind::Playlists,
            uri: "spotify:playlist:a".to_owned(),
            name: "Night Drive".to_owned(),
        })
    );
    assert_eq!(
        request
            .items
            .iter()
            .map(|item| item.verb)
            .collect::<Vec<_>>(),
        [
            crate::menu::Verb::Rename,
            crate::menu::Verb::CopyLink,
            crate::menu::Verb::RemoveFromLibrary,
        ]
    );

    let mut read_only = band_library("Night Drive", false, &["t0"]);
    let at = band_more_point(&ctx, &mut read_only, "Night Drive");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert_eq!(
        click(&ctx, at, &mut read_only, &mut transport, &mut queue),
        None
    );
    let request = crate::view::context_menu::take_request(&ctx).expect("the band posted a menu");
    assert!(
        request
            .items
            .iter()
            .all(|item| item.verb != crate::menu::Verb::Rename),
        "a list we cannot write offered Rename"
    );
}

#[test]
fn the_band_starts_flush_at_the_top_of_the_page() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &["t0"]);
    let output = tracklist_frame_in(&ctx, TALL, &mut library);
    let band = band_rect(&output);
    assert_eq!(
        band.top(),
        0.0,
        "the page starts {}pt below the topbar",
        band.top()
    );
    assert_eq!(band.width(), TALL.x, "the band is no longer full-bleed");
}

#[test]
fn the_band_field_keeps_the_floor_still() {
    let ctx = context();
    let mut closed = band_library("Night Drive", true, &["t0"]);
    let closed_output = tracklist_frame_in(&ctx, TALL, &mut closed);
    let closed_top = drawn_galleys(&closed_output)
        .into_iter()
        .find(|(text, _, _)| text == "t0")
        .map(|(_, top, _)| top)
        .expect("the row is painted");

    let mut open = band_library("Night Drive", true, &["t0"]);
    assert!(open.begin_rename("spotify:playlist:a", Surface::Band));
    let open_output = tracklist_frame_in(&ctx, TALL, &mut open);
    let open_top = drawn_galleys(&open_output)
        .into_iter()
        .find(|(text, _, _)| text == "t0")
        .map(|(_, top, _)| top)
        .expect("the row is painted");

    assert_eq!(closed_top, open_top, "the field moved the tracks");
    // The floor holding still is not the whole claim: the band itself keeps
    // the box it had, and the title keeps the baseline it stood on, so the
    // field is drawn in the title's own place rather than beside it.
    let closed_band = band_rect(&closed_output);
    let open_band = band_rect(&open_output);
    assert_eq!(
        closed_band, open_band,
        "the field resized or moved the band"
    );

    let closed_title = text_rect(&closed_output, "Night Drive").expect("the title is painted");
    let open_title = text_rect(&open_output, "Night Drive").expect("the field paints the name");
    // The field's own text layout rounds its baseline to a fraction of a
    // point, so the claim is that the title keeps its row, not its subpixel.
    assert!(
        (closed_title.top() - open_title.top()).abs() < 0.1,
        "the field moved the title: {} then {}",
        closed_title.top(),
        open_title.top()
    );
    assert_eq!(
        closed_title.height(),
        open_title.height(),
        "the field resized the title"
    );
}

/// The band's own rect, read from the wash it paints behind itself.
fn band_rect(output: &egui::FullOutput) -> Rect {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) if shape.fill == tokens.palette.hero_wash => {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the band paints its wash")
}

/// The title's widget id, taken from the press that opens its field: a press
/// on the title takes the keyboard before the field draws.
fn band_title_focus_id(ctx: &Context, library: &mut Library, name: &str) -> egui::Id {
    assert_eq!(click_band_title(ctx, library, name), None);
    ctx.memory(|memory| memory.focused())
        .expect("the title did not take the keyboard")
}

#[test]
fn a_closed_band_field_hands_the_keyboard_back_to_the_title() {
    let ctx = context();
    let mut escaping = band_library("Night Drive", true, &["t0"]);
    let title = band_title_focus_id(&ctx, &mut escaping, "Night Drive");
    let _ = band_frame(&ctx, &mut escaping, Vec::new());
    let _ = band_frame(&ctx, &mut escaping, Vec::new());
    assert!(ctx.text_edit_focused(), "the field has no keyboard");

    let _ = band_frame(&ctx, &mut escaping, vec![key(egui::Key::Escape)]);

    assert!(!escaping.renaming.is_open());
    assert!(
        ctx.memory(|memory| memory.has_focus(title)),
        "Escape left the keyboard nowhere"
    );

    let mut confirming = band_library("Night Drive", true, &["t0"]);
    let title = band_title_focus_id(&ctx, &mut confirming, "Night Drive");
    let _ = band_frame(&ctx, &mut confirming, Vec::new());
    let _ = band_frame(&ctx, &mut confirming, Vec::new());
    confirming.renaming.edit().clear();
    confirming.renaming.edit().push_str("New");
    let _ = band_frame(&ctx, &mut confirming, vec![key(egui::Key::Enter)]);
    assert!(crate::view::name_field::take_submit(&ctx));
    assert!(confirming.rename_playlist().is_some());
    confirming.apply(&Update::PlaylistRenamed {
        playlist: "spotify:playlist:a".to_owned(),
        name: "New".to_owned(),
        revision: Revision::from_wire(b"rev-2").expect("a revision"),
    });
    let _ = band_frame(&ctx, &mut confirming, Vec::new());

    assert!(
        ctx.memory(|memory| memory.has_focus(title)),
        "a confirmed rename left the keyboard nowhere"
    );
}

#[test]
fn escape_while_the_band_field_writes_closes_nothing() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &["t0"]);
    click_band_title(&ctx, &mut library, "Night Drive");
    let _ = band_frame(&ctx, &mut library, Vec::new());
    let _ = band_frame(&ctx, &mut library, Vec::new());
    library.renaming.edit().clear();
    library.renaming.edit().push_str("New");
    let _ = band_frame(&ctx, &mut library, vec![key(egui::Key::Enter)]);
    assert!(crate::view::name_field::take_submit(&ctx));
    assert!(library.rename_playlist().is_some());

    let _ = band_frame(&ctx, &mut library, vec![key(egui::Key::Escape)]);

    assert!(
        library.renaming.is_open(),
        "Escape closed a field that was writing"
    );
    assert!(library.renaming.writing());
    assert!(!library.renaming.wants_return());
}

#[test]
fn a_row_fields_return_belongs_to_the_sidebar_row() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &["t0"]);
    assert!(library.begin_rename("spotify:playlist:a", Surface::Row));
    library.renaming.cancel();
    assert!(library.renaming.wants_return());

    let _ = band_frame(&ctx, &mut library, Vec::new());

    assert!(
        library.renaming.wants_return(),
        "the band took a row field's hand-back"
    );
    assert_eq!(
        library.renaming.uri(),
        "spotify:playlist:a",
        "the band cleared the row's target"
    );
}

#[test]
fn a_band_fields_return_waits_for_the_page_that_opened_it() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &["t0"]);
    if let Load::Ready(playlists) = &mut library.playlists {
        playlists.push(PlaylistEntry {
            saved_at: None,
            composed: Vec::new(),
            uri: "spotify:playlist:b".to_owned(),
            name: "Other".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: Some(1),
            access: access(true),
        });
    }
    assert!(library.begin_rename("spotify:playlist:a", Surface::Band));
    *library.renaming.edit() = "New".to_owned();
    assert!(library.rename_playlist().is_some());

    // The reply lands after the user moved to another playlist, where this
    // field's title is not drawn at all.
    library.select("spotify:playlist:b");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:b".to_owned(),
        tracks: vec![track("t9", "t9")],
        cover: None,
        access: access(true),
    });
    library.apply(&Update::PlaylistRenamed {
        playlist: "spotify:playlist:a".to_owned(),
        name: "New".to_owned(),
        revision: Revision::from_wire(b"rev-2").expect("a revision"),
    });
    assert!(library.renaming.wants_return());

    let _ = band_frame(&ctx, &mut library, Vec::new());

    assert!(
        library.renaming.wants_return(),
        "another page spent the field's hand-back"
    );
    assert_eq!(
        library.renaming.uri(),
        "spotify:playlist:a",
        "another page took the field's target"
    );
    assert!(!ctx.text_edit_focused());
}

#[test]
fn the_band_field_takes_enter_and_escape() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &["t0"]);
    click_band_title(&ctx, &mut library, "Night Drive");
    let _ = band_frame(&ctx, &mut library, Vec::new());
    let _ = band_frame(&ctx, &mut library, Vec::new());
    assert!(ctx.text_edit_focused(), "the band field has no keyboard");

    library.renaming.edit().clear();
    let _ = band_frame(&ctx, &mut library, vec![Event::Text("New".to_owned())]);
    let (command, _) = band_frame(&ctx, &mut library, vec![key(egui::Key::Enter)]);
    assert_eq!(command, None, "the view never sends a command itself");
    assert!(
        crate::view::name_field::take_submit(&ctx),
        "Enter posted no submit for the shell"
    );
    assert_eq!(
        library.rename_playlist(),
        Some(Command::RenamePlaylist {
            playlist: "spotify:playlist:a".to_owned(),
            name: "New".to_owned(),
        })
    );
    assert!(library.renaming.writing());

    let mut escaping = band_library("Night Drive", true, &["t0"]);
    click_band_title(&ctx, &mut escaping, "Night Drive");
    let _ = band_frame(&ctx, &mut escaping, Vec::new());
    let _ = band_frame(&ctx, &mut escaping, Vec::new());
    let (command, _) = band_frame(&ctx, &mut escaping, vec![key(egui::Key::Escape)]);
    assert_eq!(command, None);
    assert!(!escaping.renaming.is_open());
}

#[test]
fn a_held_enter_after_a_band_rename_does_not_reopen_the_field() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &["t0"]);
    click_band_title(&ctx, &mut library, "Night Drive");
    let _ = band_frame(&ctx, &mut library, Vec::new());
    let _ = band_frame(&ctx, &mut library, Vec::new());
    library.renaming.edit().clear();
    let _ = band_frame(&ctx, &mut library, vec![Event::Text("New".to_owned())]);
    let _ = band_frame(&ctx, &mut library, vec![key(egui::Key::Enter)]);
    assert!(crate::view::name_field::take_submit(&ctx));
    assert!(library.rename_playlist().is_some());
    library.apply(&Update::PlaylistRenamed {
        playlist: "spotify:playlist:a".to_owned(),
        name: "New".to_owned(),
        revision: Revision::from_wire(b"rev-2").expect("a revision"),
    });
    let _ = band_frame(&ctx, &mut library, Vec::new());

    let repeat = Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: true,
        modifiers: Modifiers::NONE,
    };
    let _ = band_frame(&ctx, &mut library, vec![repeat]);

    assert!(
        !library.renaming.is_open(),
        "a held Enter reopened the field it just closed"
    );
}

#[test]
fn a_band_rename_failure_on_an_empty_playlist_says_why() {
    let ctx = context();
    let mut library = band_library("Night Drive", true, &[]);
    assert!(library.begin_rename("spotify:playlist:a", Surface::Band));
    library.renaming.edit().clear();
    library.renaming.edit().push_str("New");
    assert!(library.rename_playlist().is_some());
    library.apply(&Update::PlaylistRenameFailed {
        playlist: "spotify:playlist:a".to_owned(),
    });

    let painted = painted_lines(&ctx, Table::Full, &mut library);

    assert!(
        painted.contains(&crate::signin::failure_message(Failure::Rename).to_owned()),
        "an empty playlist hid the failure: {painted:?}"
    );
    assert!(painted.contains(&"No tracks in this playlist.".to_owned()));
}
